//! Insights: local usage metrics computed from History. Nothing leaves the computer.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use super::Db;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// Typing speed used for "time saved" (a common average for typing).
pub const TYPING_WPM: f64 = 40.0;
/// Days covered by the activity heatmap.
pub const HEATMAP_DAYS: i64 = 26 * 7;
/// Sessions shorter than this are left out of words per minute (too noisy).
const MIN_WPM_MS: i64 = 3_000;

/// One completed dictation, as the aggregation needs it.
#[derive(Debug, Clone)]
pub struct Row {
  pub created_at: i64,
  pub words: i64,
  pub duration_ms: Option<i64>,
  pub app: Option<String>,
  pub refined: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AppUsage {
  pub app: String,
  pub words: i64,
  pub sessions: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Insights {
  pub total_words: i64,
  pub sessions: i64,
  pub words_per_minute: Option<f64>,
  pub words_this_month: i64,
  pub words_last_month: i64,
  /// Minutes saved compared with typing at [`TYPING_WPM`].
  pub minutes_saved: f64,
  pub refined_sessions: i64,
  pub dictionary_fixes: i64,
  pub snippets_used: i64,
  pub apps: Vec<AppUsage>,
  pub current_streak: i64,
  pub longest_streak: i64,
  /// Local day number (days since the Unix epoch in local time) of today.
  pub today: i64,
  /// Words per active local day within the heatmap window: (day number, words).
  pub days: Vec<(i64, i64)>,
}

fn local_day(ms: i64, tz_offset_min: i64) -> i64 {
  (ms - tz_offset_min * 60_000).div_euclid(DAY_MS)
}

/// Month index (year * 12 + month) of a local day number, via the civil-from-days algorithm.
fn month_index(day: i64) -> i64 {
  let z = day + 719_468;
  let era = z.div_euclid(146_097);
  let doe = z - era * 146_097;
  let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
  let y = yoe + era * 400;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let m = if mp < 10 { mp + 3 } else { mp - 9 };
  let y = if m <= 2 { y + 1 } else { y };
  y * 12 + (m - 1)
}

/// `tz_offset_min` follows JavaScript's `getTimezoneOffset()` (UTC minus local, in minutes).
pub fn aggregate(rows: &[Row], now: i64, tz_offset_min: i64, fixes: i64, snippets: i64) -> Insights {
  let today = local_day(now, tz_offset_min);
  let this_month = month_index(today);
  let mut ins = Insights {
    today,
    dictionary_fixes: fixes,
    snippets_used: snippets,
    ..Default::default()
  };
  let (mut wpm_words, mut wpm_ms) = (0i64, 0i64);
  let mut per_day: BTreeMap<i64, i64> = BTreeMap::new();
  let mut apps: HashMap<String, (i64, i64)> = HashMap::new();
  let mut spoken_ms = 0i64;
  for r in rows.iter().filter(|r| r.words > 0) {
    ins.total_words += r.words;
    ins.sessions += 1;
    if r.refined {
      ins.refined_sessions += 1;
    }
    if let Some(d) = r.duration_ms.filter(|d| *d >= MIN_WPM_MS) {
      wpm_words += r.words;
      wpm_ms += d;
    }
    spoken_ms += r.duration_ms.unwrap_or(0).max(0);
    let day = local_day(r.created_at, tz_offset_min);
    *per_day.entry(day).or_default() += r.words;
    let m = month_index(day);
    if m == this_month {
      ins.words_this_month += r.words;
    } else if m == this_month - 1 {
      ins.words_last_month += r.words;
    }
    let app = r
      .app
      .clone()
      .filter(|a| !a.is_empty())
      .unwrap_or_else(|| "Other".into());
    let e = apps.entry(app).or_default();
    e.0 += r.words;
    e.1 += 1;
  }
  if wpm_ms > 0 {
    ins.words_per_minute = Some(wpm_words as f64 / (wpm_ms as f64 / 60_000.0));
  }
  ins.minutes_saved = (ins.total_words as f64 / TYPING_WPM - spoken_ms as f64 / 60_000.0).max(0.0);
  let mut apps: Vec<AppUsage> = apps
    .into_iter()
    .map(|(app, (words, sessions))| AppUsage { app, words, sessions })
    .collect();
  apps.sort_by(|a, b| b.words.cmp(&a.words).then(a.app.cmp(&b.app)));
  ins.apps = apps;

  // Streaks over active days. The current streak survives until the end of today.
  let days: Vec<i64> = per_day.keys().copied().collect();
  let mut run = 0i64;
  let mut prev: Option<i64> = None;
  for &d in &days {
    run = if prev == Some(d - 1) { run + 1 } else { 1 };
    ins.longest_streak = ins.longest_streak.max(run);
    prev = Some(d);
  }
  if let Some(&last) = days.last() {
    if last == today || last == today - 1 {
      let mut n = 0;
      let mut d = last;
      while per_day.contains_key(&d) {
        n += 1;
        d -= 1;
      }
      ins.current_streak = n;
    }
  }
  ins.days = per_day.into_iter().filter(|(d, _)| *d > today - HEATMAP_DAYS).collect();
  ins
}

impl Db {
  pub fn insight_rows(&self) -> rusqlite::Result<Vec<Row>> {
    let conn = self.conn();
    let mut stmt = conn.prepare(
      "SELECT created_at, COALESCE(word_count, 0), duration_ms, target_app, refine_status = 'ok'
       FROM sessions WHERE status = 'completed' AND COALESCE(word_count, 0) > 0",
    )?;
    let rows = stmt.query_map([], |r| {
      Ok(Row {
        created_at: r.get(0)?,
        words: r.get(1)?,
        duration_ms: r.get(2)?,
        app: r.get(3)?,
        refined: r.get::<_, Option<bool>>(4)?.unwrap_or(false),
      })
    })?;
    rows.collect()
  }

  /// Uses of dictionary corrections and of snippets.
  pub fn fix_counts(&self) -> rusqlite::Result<(i64, i64)> {
    self.conn().query_row(
      "SELECT (SELECT COALESCE(SUM(uses), 0) FROM dictionary WHERE heard IS NOT NULL),
              (SELECT COALESCE(SUM(uses), 0) FROM snippets)",
      [],
      |r| Ok((r.get(0)?, r.get(1)?)),
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // 2026-10-18 12:00 UTC.
  const NOW: i64 = 1_792_324_800_000;

  fn row(days_ago: i64, words: i64, ms: i64, app: &str) -> Row {
    Row {
      created_at: NOW - days_ago * DAY_MS,
      words,
      duration_ms: Some(ms),
      app: Some(app.into()),
      refined: words % 2 == 0,
    }
  }

  #[test]
  fn months_are_civil() {
    // 1970-01-01 is month 1970*12; 2026-10-18 is October.
    assert_eq!(month_index(0), 1970 * 12);
    assert_eq!(month_index(local_day(NOW, 0)), 2026 * 12 + 9);
    assert_eq!(month_index(local_day(NOW, 0) - 18), 2026 * 12 + 8, "September 30");
  }

  #[test]
  fn totals_speed_apps_and_saved_time() {
    let rows = vec![
      row(0, 120, 60_000, "Slack"),
      row(1, 60, 30_000, "Notepad"),
      row(1, 5, 1_000, "Notepad"),
      row(40, 30, 20_000, "Slack"),
      Row {
        words: 0,
        ..row(2, 0, 9_000, "X")
      },
    ];
    let i = aggregate(&rows, NOW, 0, 7, 3);
    assert_eq!(i.total_words, 215);
    assert_eq!(i.sessions, 4);
    // Sessions under 3 s are left out of the speed: (120 + 60 + 30) / 1.8333 min.
    assert!((i.words_per_minute.unwrap() - 210.0 / (110_000.0 / 60_000.0)).abs() < 1e-9);
    assert_eq!(i.apps[0].app, "Slack");
    assert_eq!(i.apps[0].words, 150);
    assert_eq!(i.words_this_month, 185);
    assert_eq!(i.words_last_month, 30);
    assert_eq!((i.dictionary_fixes, i.snippets_used), (7, 3));
    assert!(i.minutes_saved > 0.0);
  }

  #[test]
  fn streaks_and_timezone() {
    let rows = vec![
      row(0, 1, 5_000, "a"),
      row(1, 1, 5_000, "a"),
      row(2, 1, 5_000, "a"),
      row(5, 1, 5_000, "a"),
      row(6, 1, 5_000, "a"),
    ];
    let i = aggregate(&rows, NOW, 0, 0, 0);
    assert_eq!((i.current_streak, i.longest_streak), (3, 3));
    // Nothing today yet: yesterday's streak still counts.
    let i = aggregate(&rows[1..], NOW, 0, 0, 0);
    assert_eq!(i.current_streak, 2);
    // A gap of a full day ends it.
    let i = aggregate(&rows[2..], NOW, 0, 0, 0);
    assert_eq!(i.current_streak, 0);
    assert_eq!(i.longest_streak, 2);
    // 23:30 UTC is already tomorrow in UTC+6 (offset -360).
    let late = NOW + DAY_MS / 2 - 30 * 60_000;
    assert_eq!(local_day(late, -360), local_day(NOW, 0) + 1);
    assert_eq!(i.days.len(), 3);
  }

  #[test]
  fn empty_history() {
    let i = aggregate(&[], NOW, 0, 0, 0);
    assert_eq!(i.total_words, 0);
    assert_eq!(i.words_per_minute, None);
    assert_eq!(i.current_streak, 0);
    assert!(i.apps.is_empty());
  }
}

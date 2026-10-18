// Pure helpers for the Insights page: heatmap grid, intensity levels, labels.

const DAY_MS = 86_400_000;
export const HEATMAP_WEEKS = 26;

export type Cell = { day: number; date: Date; words: number; future: boolean };

/** Local calendar date for a local day number (days since the epoch in local time). */
export function dateOfDay(day: number): Date {
  const utc = new Date(day * DAY_MS);
  return new Date(utc.getUTCFullYear(), utc.getUTCMonth(), utc.getUTCDate());
}

/** Weeks (columns) of 7 days (Sun..Sat), ending with the week that contains `today`. */
export function weeksGrid(today: number, days: [number, number][]): Cell[][] {
  const words = new Map(days);
  const todayDow = dateOfDay(today).getDay();
  const lastSaturday = today + (6 - todayDow);
  const first = lastSaturday - HEATMAP_WEEKS * 7 + 1;
  const weeks: Cell[][] = [];
  for (let w = 0; w < HEATMAP_WEEKS; w++) {
    const week: Cell[] = [];
    for (let d = 0; d < 7; d++) {
      const day = first + w * 7 + d;
      week.push({ day, date: dateOfDay(day), words: words.get(day) ?? 0, future: day > today });
    }
    weeks.push(week);
  }
  return weeks;
}

/** 0 (none) to 4 (most), relative to the busiest day. */
export function heatLevel(words: number, max: number): number {
  if (words <= 0) return 0;
  return Math.min(4, Math.max(1, Math.ceil((words / Math.max(1, max)) * 4)));
}

/** Month name above the first column whose week starts a new month. */
export function monthLabels(grid: Cell[][]): Record<number, string> {
  const out: Record<number, string> = {};
  let last = -1;
  grid.forEach((week, i) => {
    const m = week[0].date.getMonth();
    if (m !== last) {
      if (i > 0 || week[0].date.getDate() <= 7) out[i] = week[0].date.toLocaleDateString('en-US', { month: 'short' });
      last = m;
    }
  });
  return out;
}

export function savedLabel(minutes: number): string {
  if (minutes < 1) return '< 1 min';
  if (minutes < 60) return `${Math.round(minutes)} min`;
  const h = Math.floor(minutes / 60);
  const m = Math.round(minutes % 60);
  return m ? `${h} h ${m} min` : `${h} h`;
}

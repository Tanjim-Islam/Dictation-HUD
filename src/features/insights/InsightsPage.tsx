import { useEffect, useMemo, useState } from 'react';
import { ArrowDownRight, ArrowUpRight, Info } from 'lucide-react';
import { api, onHistoryChange, type Insights } from '@/lib/api';
import { PageHeader } from '@/components/page';
import { Spinner } from '@/components/ui/misc';
import { Tooltip } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { heatLevel, monthLabels, savedLabel, weeksGrid } from './model';

const TYPING_WPM = 40;
const nf = new Intl.NumberFormat('en-US');

function Card({ children, className }: { children: React.ReactNode; className?: string }) {
  return <section className={cn('rounded-2xl border border-border/70 bg-surface p-5', className)}>{children}</section>;
}

function Label({ children, hint }: { children: React.ReactNode; hint?: string }) {
  return (
    <div className="mt-1 flex items-center gap-1.5 text-xs font-medium uppercase tracking-[0.08em] text-ink-muted">
      {children}
      {hint && (
        <Tooltip content={hint}>
          <button type="button" aria-label={hint} className="text-ink-faint hover:text-ink-muted">
            <Info className="size-3.5" />
          </button>
        </Tooltip>
      )}
    </div>
  );
}

/** Half-circle meter: dictation speed relative to typing (full at 4x). */
function SpeedGauge({ wpm }: { wpm: number }) {
  const ratio = wpm / TYPING_WPM;
  const frac = Math.max(0, Math.min(1, ratio / 4));
  const r = 62;
  const len = Math.PI * r;
  return (
    <div className="relative mx-auto mt-3 h-[86px] w-[156px]">
      <svg viewBox="0 0 156 86" className="h-full w-full" aria-hidden>
        <path d={`M 16 78 A ${r} ${r} 0 0 1 140 78`} fill="none" className="stroke-track/50" strokeWidth="14" strokeLinecap="round" />
        <path d={`M 16 78 A ${r} ${r} 0 0 1 140 78`} fill="none" className="stroke-accent" strokeWidth="14" strokeLinecap="round" strokeDasharray={`${len * frac} ${len}`} />
      </svg>
      <div className="absolute inset-x-0 bottom-0 text-center">
        <div className="text-[19px] font-semibold tabular-nums leading-6">{ratio.toFixed(1)}×</div>
        <div className="text-xs text-ink-muted">faster than typing</div>
      </div>
    </div>
  );
}

function MonthTrend({ thisMonth, lastMonth }: { thisMonth: number; lastMonth: number }) {
  if (lastMonth <= 0) return null;
  const pct = Math.round(((thisMonth - lastMonth) / lastMonth) * 100);
  const up = pct >= 0;
  return (
    <span className={cn('inline-flex h-6 items-center gap-1 rounded-full px-2 text-xs font-medium', up ? 'bg-success-soft text-success' : 'bg-surface-strong text-ink-muted')}>
      {up ? <ArrowUpRight className="size-3.5" /> : <ArrowDownRight className="size-3.5" />}
      {Math.abs(pct)}% this month
    </span>
  );
}

function AppUsage({ data }: { data: Insights }) {
  const top = data.apps.slice(0, 6);
  const rest = data.apps.slice(6).reduce((s, a) => s + a.words, 0);
  const rows = rest > 0 ? [...top, { app: 'Other apps', words: rest, sessions: data.apps.slice(6).reduce((s, a) => s + a.sessions, 0) }] : top;
  const total = Math.max(1, data.total_words);
  return (
    <Card>
      <div className="flex items-baseline justify-between">
        <h2 className="font-display text-[28px] leading-9">Where you dictate</h2>
        <span className="text-xs font-medium uppercase tracking-[0.08em] text-ink-muted">Apps used | {data.apps.length}</span>
      </div>
      {rows.length === 0 ? (
        <p className="mt-6 text-[14px] text-ink-muted">No dictations yet.</p>
      ) : (
        <ul className="mt-5 space-y-3">
          {rows.map((a) => {
            const pct = Math.round((a.words / total) * 100);
            return (
              <li key={a.app} className="grid grid-cols-[minmax(0,1fr)_minmax(0,140px)] items-center gap-3" title={`${a.app}: ${nf.format(a.words)} words in ${a.sessions} dictations`}>
                <div className="h-7 overflow-hidden rounded-[4px] bg-track/25">
                  <div className="flex h-full min-w-9 items-center justify-end rounded-[4px] bg-accent px-2 text-xs font-medium tabular-nums text-white" style={{ width: `${Math.max(pct, 6)}%` }}>
                    {pct}%
                  </div>
                </div>
                <div className="min-w-0 text-[13px]">
                  <div className="truncate font-medium text-ink">{a.app}</div>
                  <div className="text-xs tabular-nums text-ink-muted">{nf.format(a.words)} words</div>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </Card>
  );
}

const LEVELS = ['bg-track/25', 'bg-accent/25', 'bg-accent/50', 'bg-accent/75', 'bg-accent'];

function Streak({ data }: { data: Insights }) {
  const grid = useMemo(() => weeksGrid(data.today, data.days), [data]);
  const max = useMemo(() => Math.max(1, ...data.days.map((d) => d[1])), [data]);
  const months = useMemo(() => monthLabels(grid), [grid]);
  const streakStart = data.current_streak > 0 ? data.days[data.days.length - 1][0] - data.current_streak + 1 : null;
  const streakEnd = data.current_streak > 0 ? data.days[data.days.length - 1][0] : null;
  return (
    <Card>
      <div className="flex items-baseline justify-between">
        <h2 className="font-display text-[28px] leading-9">
          {data.current_streak} day streak
        </h2>
        <span className="text-xs font-medium uppercase tracking-[0.08em] text-ink-muted">Longest streak | {data.longest_streak} days</span>
      </div>
      <div className="mt-4 overflow-x-auto">
        <div className="inline-grid gap-[3px]" style={{ gridTemplateColumns: `28px repeat(${grid.length}, 13px)` }}>
          <div />
          {grid.map((_, i) => (
            <div key={`m${i}`} className="h-4 whitespace-nowrap text-[11px] text-ink-muted">
              {months[i] ?? ''}
            </div>
          ))}
          {['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'].map((name, row) => (
            <div key={name} className="contents">
              <div className="h-[13px] text-[11px] leading-[13px] text-ink-muted">{row % 2 === 1 ? name : ''}</div>
              {grid.map((week, col) => {
                const cell = week[row];
                if (!cell) return <div key={col} className="size-[13px]" />;
                const inStreak = streakStart !== null && streakEnd !== null && cell.day >= streakStart && cell.day <= streakEnd;
                return (
                  <div
                    key={col}
                    title={`${cell.date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}: ${cell.words ? `${nf.format(cell.words)} words` : 'no dictation'}`}
                    className={cn('size-[13px] rounded-[3px]', LEVELS[heatLevel(cell.words, max)], cell.future && 'opacity-0', inStreak && 'ring-1 ring-inset ring-ink/60')}
                  />
                );
              })}
            </div>
          ))}
        </div>
      </div>
      <div className="mt-3 flex items-center justify-between text-xs text-ink-muted">
        <div className="flex items-center gap-1.5">
          Less
          {LEVELS.map((c) => (
            <span key={c} className={cn('size-[11px] rounded-[3px]', c)} />
          ))}
          More
        </div>
        <span className="flex items-center gap-1.5">
          <span className="size-[11px] rounded-[3px] ring-1 ring-inset ring-ink/60" /> Current streak
        </span>
      </div>
    </Card>
  );
}

export function InsightsPage() {
  const [data, setData] = useState<Insights | null>(null);
  useEffect(() => {
    const load = () =>
      api
        .insights(new Date().getTimezoneOffset())
        .then(setData)
        .catch(() => {});
    load();
    const un = onHistoryChange(load);
    return () => {
      un.then((f) => f());
    };
  }, []);

  if (!data) {
    return (
      <div className="flex justify-center py-24">
        <Spinner />
      </div>
    );
  }
  const fixes = data.dictionary_fixes + data.snippets_used + data.refined_sessions;
  return (
    <div className="mx-auto max-w-[1040px]">
      <PageHeader title="Insights" />
      <p className="mt-1 text-[13px] text-ink-muted">Calculated on this computer from your History. Nothing is uploaded.</p>
      <div className="mt-7 grid grid-cols-[1fr_1fr_1.6fr] gap-4">
        <Card>
          <div className="text-[30px] font-semibold tabular-nums leading-9">{data.words_per_minute ? Math.round(data.words_per_minute) : '–'}</div>
          <Label hint={`Words divided by speaking time, compared with typing at ${TYPING_WPM} words per minute.`}>Words per minute</Label>
          {data.words_per_minute ? <SpeedGauge wpm={data.words_per_minute} /> : <p className="mt-6 text-[13px] text-ink-muted">Dictate a few times to see your speed.</p>}
        </Card>
        <Card>
          <div className="text-[30px] font-semibold tabular-nums leading-9">{nf.format(fixes)}</div>
          <Label>Fixes made for you</Label>
          <ul className="mt-4 space-y-2 border-t border-border pt-4 text-[14px]">
            <li>
              <span className="font-semibold tabular-nums">{nf.format(data.dictionary_fixes)}</span> dictionary fixes
            </li>
            <li>
              <span className="font-semibold tabular-nums">{nf.format(data.snippets_used)}</span> snippets inserted
            </li>
            <li>
              <span className="font-semibold tabular-nums">{nf.format(data.refined_sessions)}</span> dictations refined by AI
            </li>
          </ul>
        </Card>
        <Card>
          <div className="flex items-start justify-between gap-3">
            <div>
              <div className="text-[30px] font-semibold tabular-nums leading-9">{nf.format(data.total_words)}</div>
              <Label>Total words dictated</Label>
            </div>
            <MonthTrend thisMonth={data.words_this_month} lastMonth={data.words_last_month} />
          </div>
          <div className="mt-4 grid grid-cols-2 gap-4 border-t border-border pt-4 text-[14px]">
            <div>
              <div className="font-semibold tabular-nums">{savedLabel(data.minutes_saved)}</div>
              <div className="text-xs text-ink-muted">saved vs. typing</div>
            </div>
            <div>
              <div className="font-semibold tabular-nums">{nf.format(data.sessions)}</div>
              <div className="text-xs text-ink-muted">dictations</div>
            </div>
          </div>
        </Card>
      </div>
      <div className="mt-4 grid grid-cols-2 gap-4">
        <AppUsage data={data} />
        <Streak data={data} />
      </div>
    </div>
  );
}

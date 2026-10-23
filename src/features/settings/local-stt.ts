import type { LocalSttModel, LocalSttState } from '@/lib/api';

const RANK: Record<LocalSttState, number> = { installed: 0, linked: 0, downloading: 1, missing: 2, not_installed: 3 };

export const isReady = (s: LocalSttState) => s === 'installed' || s === 'linked';

/** Ready models first; catalog order inside each group. */
export function sortSttModels(models: LocalSttModel[]): LocalSttModel[] {
  return models
    .map((m, i) => ({ m, i }))
    .sort((a, b) => RANK[a.m.state] - RANK[b.m.state] || a.i - b.i)
    .map(({ m }) => m);
}

/** Models worth a row right away (on this PC, downloading, or missing) and the rest of the catalog.
 * With nothing installed yet, the default model is shown so there is always one to download. */
export function splitSttModels(models: LocalSttModel[], defaultId = 'parakeet-v3'): { shown: LocalSttModel[]; more: LocalSttModel[] } {
  const sorted = sortSttModels(models);
  const shown = sorted.filter((m) => m.state !== 'not_installed');
  if (shown.length === 0) shown.push(...sorted.filter((m) => m.id === defaultId));
  return { shown, more: sorted.filter((m) => !shown.includes(m)) };
}

/** Short hint next to a model in the list; nothing when it is ready. */
export function modelHint(m: LocalSttModel): string | undefined {
  if (isReady(m.state)) return undefined;
  if (m.state === 'downloading') return `${Math.round((m.pct ?? 0) * 100)}%`;
  return m.state === 'missing' ? 'Missing' : 'Not installed';
}

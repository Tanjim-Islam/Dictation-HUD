import { describe, expect, it } from 'vitest';
import type { LocalSttModel } from '@/lib/api';
import { isReady, modelHint, sortSttModels, splitSttModels } from './local-stt';

const m = (id: string, state: LocalSttModel['state'], pct: number | null = null): LocalSttModel => ({
  id,
  name: id,
  size_bytes: 1,
  languages: ['en'],
  state,
  pct,
  path: null,
});

describe('local speech helpers', () => {
  it('lists ready models first and keeps catalog order inside each group', () => {
    const sorted = sortSttModels([m('a', 'not_installed'), m('b', 'linked'), m('c', 'downloading', 0.4), m('d', 'installed'), m('e', 'missing')]);
    expect(sorted.map((x) => x.id)).toEqual(['b', 'd', 'c', 'e', 'a']);
  });

  it('shows models on this PC and keeps the rest behind More models', () => {
    const { shown, more } = splitSttModels([m('a', 'not_installed'), m('b', 'installed'), m('c', 'missing'), m('d', 'not_installed')]);
    expect(shown.map((x) => x.id)).toEqual(['b', 'c']);
    expect(more.map((x) => x.id)).toEqual(['a', 'd']);
    const fresh = splitSttModels([m('x', 'not_installed'), m('parakeet-v3', 'not_installed')]);
    expect(fresh.shown.map((x) => x.id)).toEqual(['parakeet-v3']);
    expect(fresh.more.map((x) => x.id)).toEqual(['x']);
  });

  it('hints only when a model is not ready', () => {
    expect(modelHint(m('a', 'installed'))).toBeUndefined();
    expect(modelHint(m('a', 'not_installed'))).toBe('Not installed');
    expect(modelHint(m('a', 'missing'))).toBe('Missing');
    expect(modelHint(m('a', 'downloading', 0.426))).toBe('43%');
    expect(isReady('linked')).toBe(true);
    expect(isReady('downloading')).toBe(false);
  });
});

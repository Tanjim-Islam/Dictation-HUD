import { useEffect, useRef, useSyncExternalStore } from 'react';
import type { Theme } from './api';

export type Resolved = 'light' | 'dark';

const CACHE = 'dictation-hud.theme';
const darkQuery = () => window.matchMedia('(prefers-color-scheme: dark)');

export function resolveTheme(pref: Theme): Resolved {
  if (pref === 'system') return darkQuery().matches ? 'dark' : 'light';
  return pref;
}

export function applyTheme(resolved: Resolved) {
  const root = document.documentElement;
  root.classList.toggle('dark', resolved === 'dark');
  root.style.colorScheme = resolved;
  try {
    localStorage.setItem(CACHE, resolved);
  } catch {
    // Storage can be unavailable; the theme still applies for this run.
  }
}

/** Changes theme with a short crossfade (as Wispr Flow does), unless motion is reduced. */
function switchTheme(resolved: Resolved) {
  const root = document.documentElement;
  if (root.classList.contains('dark') === (resolved === 'dark')) return applyTheme(resolved);
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  if (reduced || typeof document.startViewTransition !== 'function') return applyTheme(resolved);
  root.classList.add('theme-switching');
  const t = document.startViewTransition(() => applyTheme(resolved));
  void t.finished.finally(() => root.classList.remove('theme-switching'));
}

/** Applies the last theme before React renders, so the window never flashes the wrong one. */
export function applyCachedTheme() {
  let cached: string | null = null;
  try {
    cached = localStorage.getItem(CACHE);
  } catch {
    cached = null;
  }
  applyTheme(cached === 'dark' || cached === 'light' ? cached : resolveTheme('system'));
}

/** Keeps the document in sync with the setting, following the OS while it is `system`. */
export function useThemeSync(pref: Theme | undefined) {
  const first = useRef(true);
  useEffect(() => {
    if (!pref) return;
    // The first value only confirms the cached theme; later changes crossfade.
    (first.current ? applyTheme : switchTheme)(resolveTheme(pref));
    first.current = false;
    if (pref !== 'system') return;
    const q = darkQuery();
    const on = () => switchTheme(resolveTheme('system'));
    q.addEventListener('change', on);
    return () => q.removeEventListener('change', on);
  }, [pref]);
}

function subscribeSystem(on: () => void) {
  const q = darkQuery();
  q.addEventListener('change', on);
  return () => q.removeEventListener('change', on);
}

/** The theme in effect for a preference, updating when the OS theme changes. */
export function useResolvedTheme(pref: Theme | undefined): Resolved {
  const systemDark = useSyncExternalStore(subscribeSystem, () => darkQuery().matches);
  if (pref === 'light' || pref === 'dark') return pref;
  return systemDark ? 'dark' : 'light';
}

import { useSyncExternalStore } from 'react';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

export type State =
  | { status: 'idle' | 'checking' | 'uptodate' }
  | { status: 'available'; version: string }
  | { status: 'downloading'; progress: number }
  | { status: 'ready' }
  | { status: 'error'; message: string };

// Module state so the tray "Check for updates" and Settings share one updater flow.
let state: State = { status: 'idle' };
let pending: Update | null = null;
const listeners = new Set<() => void>();
const set = (s: State) => {
  state = s;
  listeners.forEach((l) => l());
};

/** `silent` (background checks): offline or other failures leave the state unchanged. */
export async function checkForUpdates({ silent = false }: { silent?: boolean } = {}) {
  if (state.status === 'checking' || state.status === 'downloading' || state.status === 'ready') return;
  // A background check never hides an update that was already found.
  if (silent && state.status === 'available') return;
  const before = state;
  set({ status: 'checking' });
  try {
    pending = await check();
    set(pending ? { status: 'available', version: pending.version } : { status: 'uptodate' });
  } catch (e) {
    set(silent ? before : { status: 'error', message: e instanceof Error ? e.message : String(e) });
  }
}

const FIRST_CHECK_MS = 10_000;
const RECHECK_MS = 6 * 60 * 60 * 1000;
let started = false;

/** Checks shortly after launch and every few hours, silently. Returns a stop function. */
export function startBackgroundChecks() {
  if (started) return () => {};
  started = true;
  const first = window.setTimeout(() => void checkForUpdates({ silent: true }), FIRST_CHECK_MS);
  const every = window.setInterval(() => void checkForUpdates({ silent: true }), RECHECK_MS);
  return () => {
    started = false;
    window.clearTimeout(first);
    window.clearInterval(every);
  };
}

export async function installUpdate() {
  if (!pending) return;
  let total = 0;
  let got = 0;
  set({ status: 'downloading', progress: 0 });
  try {
    await pending.downloadAndInstall((ev) => {
      if (ev.event === 'Started') total = ev.data.contentLength ?? 0;
      if (ev.event === 'Progress') {
        got += ev.data.chunkLength;
        set({ status: 'downloading', progress: total ? Math.round((got / total) * 100) : 0 });
      }
    });
    set({ status: 'ready' });
    await relaunch();
  } catch (e) {
    set({ status: 'error', message: e instanceof Error ? e.message : String(e) });
  }
}

export function useUpdateState() {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => state,
  );
}

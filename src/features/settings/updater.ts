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

export async function checkForUpdates() {
  if (state.status === 'checking' || state.status === 'downloading') return;
  set({ status: 'checking' });
  try {
    pending = await check();
    set(pending ? { status: 'available', version: pending.version } : { status: 'uptodate' });
  } catch (e) {
    set({ status: 'error', message: e instanceof Error ? e.message : String(e) });
  }
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

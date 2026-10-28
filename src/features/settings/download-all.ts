import { useSyncExternalStore } from 'react';
import { toast } from 'sonner';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { api, errorMessage, onSttDownload } from '@/lib/api';

// Module state so "Download all" keeps going when Settings is closed and reopened.
// One model at a time: parallel downloads would split bandwidth and each check disk space alone.
let queue: string[] = [];
let current: string | null = null;
let unlisten: UnlistenFn | null = null;
const listeners = new Set<() => void>();
const notify = () => listeners.forEach((l) => l());

function next() {
  current = queue.shift() ?? null;
  notify();
  if (!current) {
    unlisten?.();
    unlisten = null;
    return;
  }
  const id = current;
  api.localStt.download(id).catch((e) => {
    toast.error('Download failed', { description: errorMessage(e) });
    if (current !== id) return;
    queue = [];
    next();
  });
}

/** Downloads `ids` in order. A failed download stops the rest (often no disk space or no network);
 * cancelling one model's download skips to the next. */
export async function downloadAll(ids: string[]) {
  if (current || queue.length > 0 || ids.length === 0) return;
  queue = [...ids];
  unlisten = await onSttDownload((e) => {
    if (e.id !== current || e.state === 'downloading') return;
    if (e.state === 'failed') queue = [];
    next();
  });
  next();
}

export function stopAll() {
  queue = [];
  if (current) void api.localStt.cancel(current);
  else notify();
}

export function useDownloadAllRunning() {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => void listeners.delete(l);
    },
    () => current !== null,
  );
}

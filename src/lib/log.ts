import { invoke } from '@tauri-apps/api/core';

// Diagnostics only. Never pass transcripts, keys, context, or clipboard text here.
function send(prefix: string, args: unknown[]) {
  const message = prefix + args.map((arg) => (typeof arg === 'object' ? JSON.stringify(arg) : String(arg))).join(' ');
  invoke('log_to_terminal', { message }).catch(() => {});
}

export function log(...args: unknown[]) {
  console.log(...args);
  send('', args);
}

export function error(...args: unknown[]) {
  console.error(...args);
  send('error: ', args);
}

export function warn(...args: unknown[]) {
  console.warn(...args);
  send('warn: ', args);
}

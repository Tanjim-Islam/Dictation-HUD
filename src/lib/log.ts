import { invoke } from '@tauri-apps/api/core';

// Send logs to both console and terminal
export function log(...args: any[]) {
  const message = args.map(arg =>
    typeof arg === 'object' ? JSON.stringify(arg) : String(arg)
  ).join(' ');

  console.log(...args);
  invoke('log_to_terminal', { message }).catch(() => {});
}

export function error(...args: any[]) {
  const message = '❌ ' + args.map(arg =>
    typeof arg === 'object' ? JSON.stringify(arg) : String(arg)
  ).join(' ');

  console.error(...args);
  invoke('log_to_terminal', { message }).catch(() => {});
}

export function warn(...args: any[]) {
  const message = '⚠️ ' + args.map(arg =>
    typeof arg === 'object' ? JSON.stringify(arg) : String(arg)
  ).join(' ');

  console.warn(...args);
  invoke('log_to_terminal', { message }).catch(() => {});
}

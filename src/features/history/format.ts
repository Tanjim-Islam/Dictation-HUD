import type { HistoryItem } from '@/lib/api';

const DAY = 24 * 60 * 60 * 1000;

function startOfDay(ms: number) {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** "9:25pm" */
export function timeLabel(ms: number) {
  const d = new Date(ms);
  const h = d.getHours();
  const m = String(d.getMinutes()).padStart(2, '0');
  return `${h % 12 || 12}:${m}${h < 12 ? 'am' : 'pm'}`;
}

/** Group key and heading for a session's day. */
export function dayKey(ms: number) {
  return String(startOfDay(ms));
}

export function dayLabel(ms: number, now = Date.now()) {
  const diff = Math.round((startOfDay(now) - startOfDay(ms)) / DAY);
  if (diff === 0) return 'Today';
  if (diff === 1) return 'Yesterday';
  const d = new Date(ms);
  const sameYear = d.getFullYear() === new Date(now).getFullYear();
  return d.toLocaleDateString('en-US', sameYear ? { weekday: 'long', month: 'long', day: 'numeric' } : { month: 'long', day: 'numeric', year: 'numeric' });
}

export function fullDate(ms: number) {
  return `${new Date(ms).toLocaleDateString('en-US', { month: 'long', day: 'numeric', year: 'numeric' })}, ${timeLabel(ms)}`;
}

/** "0:07", "12:05" */
export function clock(ms: number) {
  const total = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

export function bytesLabel(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function displayText(item: HistoryItem) {
  return (item.final_text ?? item.raw_text ?? '').trim();
}

export function hasAudio(item: HistoryItem) {
  return item.audio_state === 'encoded';
}

export function needsAttention(item: HistoryItem) {
  return item.status === 'interrupted' || item.status === 'failed' || item.delivery_status === 'failed';
}

const FAILURES: Record<string, string> = {
  network: 'network problem',
  auth: 'API key rejected',
  missing_key: 'no API key',
  rate_limited: 'rate limited',
  microphone: 'microphone unavailable',
  timeout: 'provider timed out',
  app_closed: 'app closed before it finished',
  recovered_audio: 'recovered after a crash',
  engine_lost: 'the recorder stopped',
};

export function failureText(code: string | null) {
  if (!code) return 'unknown error';
  if (code.startsWith('provider:')) return 'speech provider error';
  return FAILURES[code] ?? code.replace(/_/g, ' ');
}

/** Shown in place of text when a session has none. */
export function emptyLine(item: HistoryItem) {
  switch (item.status) {
    case 'in_progress':
      return 'Listening…';
    case 'no_speech':
      return 'No speech detected';
    case 'cancelled':
      return 'Cancelled';
    case 'interrupted':
      return `Interrupted: ${item.failure ? failureText(item.failure) : 'did not finish'}`;
    case 'failed':
      return `Failed: ${failureText(item.failure)}`;
    default:
      return 'No text';
  }
}

const COPY_REASONS: Record<string, string> = {
  auto_paste_off: 'Copied (auto paste is off)',
  no_target: 'Copied: no text field was active',
  target_closed: 'Copied: the window was closed',
  target_changed: 'Copied: the window was closed',
  switched_app: 'Copied: you switched apps',
  elevated_target: 'Copied: the app runs as administrator',
  focus_failed: 'Copied: the window could not be focused',
  keystroke_failed: 'Copied: the paste keystroke was blocked',
};

export function deliveryLabel(item: HistoryItem) {
  switch (item.delivery_status) {
    case 'pasted':
      return item.target_app ? `Pasted into ${item.target_app}` : 'Pasted';
    case 'copied':
      return COPY_REASONS[item.delivery_reason ?? ''] ?? 'Copied to clipboard';
    case 'failed':
      return 'Not delivered';
    case 'none':
      return 'Not delivered (recovered later)';
    case 'pending':
      return 'Delivery did not finish';
    default:
      return null;
  }
}

export type Tag = { label: string; tone: 'neutral' | 'warning' | 'danger' | 'accent' };

/** A small status tag for the row, if the session is not a plain success. */
export function rowTag(item: HistoryItem): Tag | null {
  if (item.recovered_from) return { label: 'Recovered', tone: 'accent' };
  if (item.status === 'interrupted') return { label: 'Interrupted', tone: 'warning' };
  if (item.status === 'failed') return { label: 'Failed', tone: 'danger' };
  if (item.delivery_status === 'failed') return { label: 'Not delivered', tone: 'danger' };
  if (item.delivery_status === 'copied' && item.delivery_reason && item.delivery_reason !== 'auto_paste_off') return { label: 'Copied', tone: 'neutral' };
  return null;
}

import { describe, expect, it } from 'vitest';
import type { HistoryItem } from '@/lib/api';
import { bytesLabel, clock, dayLabel, deliveryLabel, emptyLine, needsAttention, rowTag, timeLabel } from './format';

const item = (over: Partial<HistoryItem>): HistoryItem => ({
  id: 'x',
  created_at: 0,
  ended_at: null,
  trigger: 'hotkey',
  status: 'completed',
  failure: null,
  provider: 'deepgram',
  model: 'nova-3',
  language: 'en',
  raw_text: null,
  formatted_text: null,
  final_text: null,
  refine_status: null,
  delivery_status: null,
  delivery_reason: null,
  target_app: null,
  duration_ms: null,
  word_count: null,
  retry_of: null,
  audio_state: null,
  audio_ms: null,
  audio_bytes: null,
  recovered_from: null,
  attempts: 0,
  ...over,
});

describe('history format', () => {
  it('formats times and durations', () => {
    expect(timeLabel(new Date(2026, 0, 1, 21, 5).getTime())).toBe('9:05pm');
    expect(timeLabel(new Date(2026, 0, 1, 0, 30).getTime())).toBe('12:30am');
    expect(clock(65_400)).toBe('1:05');
    expect(bytesLabel(1536)).toBe('2 KB');
    expect(bytesLabel(5 * 1024 * 1024)).toBe('5.0 MB');
  });

  it('labels days relative to now', () => {
    const now = new Date(2026, 9, 18, 10).getTime();
    expect(dayLabel(new Date(2026, 9, 18, 1).getTime(), now)).toBe('Today');
    expect(dayLabel(new Date(2026, 9, 17, 23).getTime(), now)).toBe('Yesterday');
    expect(dayLabel(new Date(2026, 9, 12).getTime(), now)).toBe('Monday, October 12');
    expect(dayLabel(new Date(2025, 9, 12).getTime(), now)).toBe('October 12, 2025');
  });

  it('describes status and delivery', () => {
    expect(emptyLine(item({ status: 'failed', failure: 'provider:http 500' }))).toBe('Failed: speech provider error');
    expect(emptyLine(item({ status: 'interrupted', failure: 'app_closed' }))).toBe('Interrupted: app closed before it finished');
    expect(needsAttention(item({ status: 'interrupted' }))).toBe(true);
    expect(needsAttention(item({ status: 'completed', recovered_from: 'interrupted' }))).toBe(false);
    expect(rowTag(item({ recovered_from: 'failed' }))?.label).toBe('Recovered');
    expect(rowTag(item({ delivery_status: 'copied', delivery_reason: 'auto_paste_off' }))).toBeNull();
    expect(deliveryLabel(item({ delivery_status: 'pasted', target_app: 'Notepad' }))).toBe('Pasted into Notepad');
    expect(deliveryLabel(item({ delivery_status: 'copied', delivery_reason: 'switched_app' }))).toBe('Copied: you switched apps');
  });
});

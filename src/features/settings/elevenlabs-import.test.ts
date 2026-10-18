import { describe, expect, it } from 'vitest';
import { looksLikeKeyBlock, __test } from './elevenlabs-import';

describe('ElevenLabs key paste', () => {
  it('detects blocks of keys but not a single bare key', () => {
    expect(looksLikeKeyBlock('sk_abcdefghijklmnopqrstuvwxyz0123')).toBe(false);
    expect(looksLikeKeyBlock('  sk_abcdefghijklmnopqrstuvwxyz0123\n')).toBe(false);
    expect(looksLikeKeyBlock('ELEVENLABS_API_KEY_1=sk_a\nELEVENLABS_API_KEY_2=sk_b')).toBe(true);
    expect(looksLikeKeyBlock('ELEVENLABS_API_KEY=sk_a')).toBe(true);
    expect(looksLikeKeyBlock('sk_a, sk_b')).toBe(true);
  });

  it('summarizes skipped keys', () => {
    const base = { found: 5, added: 2, duplicates: 1, rejected: 1, unverified: 1, failed: 0, multi: true };
    expect(__test.summary(base)).toBe('1 key already saved, 1 key rejected by ElevenLabs, 1 key could not be verified (network)');
    expect(__test.summary({ ...base, duplicates: 0, rejected: 0, unverified: 0 })).toBe('');
  });
});

import { describe, expect, it } from 'vitest';
import { BAR_COUNT, MIN_BAR, busyBars, levelFromDb, restingBars, stepBars } from './waveform';

describe('flow bar waveform', () => {
  it('maps dB to a bounded level', () => {
    expect(levelFromDb(-90)).toBe(0);
    expect(levelFromDb(-Infinity)).toBe(0);
    expect(levelFromDb(0)).toBe(1);
    expect(levelFromDb(-30)).toBeGreaterThan(levelFromDb(-40));
  });

  it('rises fast, falls slowly, and stays in range', () => {
    let bars = restingBars();
    for (let t = 0; t < 200; t += 16) bars = stepBars(bars, 1, t, 16);
    const loud = Math.max(...bars);
    expect(loud).toBeGreaterThan(0.6);
    let after = stepBars(bars, 0, 216, 16);
    expect(Math.max(...after)).toBeGreaterThan(loud * 0.7);
    for (let t = 232; t < 2000; t += 16) after = stepBars(after, 0, t, 16);
    expect(Math.max(...after)).toBeLessThan(MIN_BAR + 0.02);
    for (const h of [...bars, ...after]) {
      expect(h).toBeGreaterThanOrEqual(MIN_BAR - 1e-9);
      expect(h).toBeLessThanOrEqual(1);
    }
  });

  it('center bars are taller than edges and silence rests', () => {
    const bars = stepBars(restingBars(), 0.8, 0, 16, true);
    expect(bars).toHaveLength(BAR_COUNT);
    expect(bars[4]).toBeGreaterThan(bars[0]);
    expect(stepBars(restingBars(), 0, 0, 16, true).every((h) => h === MIN_BAR)).toBe(true);
  });

  it('busy animation moves unless reduced motion', () => {
    expect(busyBars(0)).not.toEqual(busyBars(300));
    expect(busyBars(0, true)).toEqual(busyBars(300, true));
  });
});

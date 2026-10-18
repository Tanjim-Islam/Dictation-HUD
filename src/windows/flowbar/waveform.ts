// Ten-bar waveform model for the Flow Bar. Pure functions so the motion is testable; the
// component only feeds native level frames in and paints the heights (0..1).

export const BAR_COUNT = 10;
/** Resting height: a short dash, so the bar never looks empty. */
export const MIN_BAR = 0.14;
/** Center bars move more than the outer ones. */
const SHAPE = [0.42, 0.58, 0.74, 0.9, 1, 1, 0.9, 0.74, 0.58, 0.42];
const FRAME_MS = 1000 / 60;

/** Maps an RMS level in dBFS to 0..1: silent below -55 dB, full at -12 dB. */
export function levelFromDb(rmsDb: number): number {
  if (!Number.isFinite(rmsDb)) return 0;
  const x = Math.min(1, Math.max(0, (rmsDb + 55) / 43));
  return Math.pow(x, 0.8);
}

export function restingBars(): number[] {
  return Array.from({ length: BAR_COUNT }, () => MIN_BAR);
}

/**
 * One animation step toward the current level. Bars rise fast and fall slowly (like a VU
 * meter). `still` drops the per-bar wobble and smoothing (reduced motion): heights then
 * follow the level directly.
 */
export function stepBars(prev: readonly number[], level: number, tMs: number, dtMs: number, still = false): number[] {
  return prev.map((h, i) => {
    const wobble = still ? 1 : 0.72 + 0.28 * Math.sin(tMs / (110 + i * 17) + i * 1.7);
    const target = MIN_BAR + (1 - MIN_BAR) * level * SHAPE[i] * wobble;
    if (still) return target;
    const rate = target > h ? 0.55 : 0.16;
    const k = 1 - Math.pow(1 - rate, dtMs / FRAME_MS);
    return h + (target - h) * k;
  });
}

/** Finalizing / processing: a pulse travelling across the bars. Static when `still`. */
export function busyBars(tMs: number, still = false): number[] {
  return Array.from({ length: BAR_COUNT }, (_, i) => {
    if (still) return MIN_BAR + 0.2;
    const s = Math.max(0, Math.sin(tMs / 140 - i * 0.55));
    return MIN_BAR + 0.42 * s * s;
  });
}

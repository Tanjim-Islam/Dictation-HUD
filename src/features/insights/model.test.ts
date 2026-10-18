import { describe, expect, it } from 'vitest';
import { dateOfDay, heatLevel, monthLabels, savedLabel, weeksGrid, HEATMAP_WEEKS } from './model';

// 2026-10-18 (a Sunday) as a local day number.
const TODAY = Math.floor(Date.UTC(2026, 9, 18) / 86_400_000);

describe('insights model', () => {
  it('maps day numbers to local dates', () => {
    const d = dateOfDay(TODAY);
    expect([d.getFullYear(), d.getMonth(), d.getDate(), d.getDay()]).toEqual([2026, 9, 18, 0]);
  });

  it('builds Sunday-first weeks ending with this week', () => {
    const grid = weeksGrid(TODAY, [[TODAY, 50], [TODAY - 1, 10]]);
    expect(grid).toHaveLength(HEATMAP_WEEKS);
    const last = grid[grid.length - 1];
    expect(last[0].day).toBe(TODAY);
    expect(last[0].words).toBe(50);
    expect(last[1].future).toBe(true);
    expect(grid[grid.length - 2][6].words).toBe(10);
    expect(grid.every((w) => w[0].date.getDay() === 0)).toBe(true);
  });

  it('levels and labels', () => {
    expect(heatLevel(0, 100)).toBe(0);
    expect(heatLevel(1, 100)).toBe(1);
    expect(heatLevel(100, 100)).toBe(4);
    expect(savedLabel(0.5)).toBe('< 1 min');
    expect(savedLabel(42)).toBe('42 min');
    expect(savedLabel(125)).toBe('2 h 5 min');
    const labels = Object.values(monthLabels(weeksGrid(TODAY, [])));
    expect(labels).toContain('Oct');
    expect(new Set(labels).size).toBe(labels.length);
  });
});

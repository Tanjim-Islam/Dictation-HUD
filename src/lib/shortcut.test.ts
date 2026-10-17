import { describe, expect, it } from 'vitest';
import { eventToKeyArray, normalizeKey } from './shortcut';

const ev = (key: string, mods: Partial<Record<'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey', boolean>> = {}) => ({
  key,
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  altKey: false,
  ...mods,
});

describe('shortcut capture', () => {
  it('orders modifiers consistently and upper-cases letters', () => {
    expect(eventToKeyArray(ev('h', { altKey: true, ctrlKey: true, shiftKey: true }))).toEqual(['Ctrl', 'Shift', 'Alt', 'H']);
  });

  it('does not add a modifier key as the main key', () => {
    expect(eventToKeyArray(ev('Control', { ctrlKey: true }))).toEqual(['Ctrl']);
    expect(eventToKeyArray(ev('AltGraph', { altKey: true }))).toEqual(['Alt']);
  });

  it('normalizes special key names', () => {
    expect(normalizeKey(' ')).toBe('Space');
    expect(eventToKeyArray(ev(' ', { ctrlKey: true }))).toEqual(['Ctrl', 'Space']);
    expect(eventToKeyArray(ev('F9'))).toEqual(['F9']);
  });
});

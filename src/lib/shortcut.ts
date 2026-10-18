// Keyboard shortcut capture helpers shared by shortcut recorders.

const KEY_NAMES: Record<string, string> = {
  Control: 'Ctrl',
  Meta: 'Cmd',
  AltGraph: 'Alt',
  ' ': 'Space',
};

const MODIFIER_KEYS = new Set(['Control', 'Shift', 'Alt', 'Meta', 'Cmd', 'Ctrl', 'AltGraph']);

export function normalizeKey(key: string): string {
  return KEY_NAMES[key] || key;
}

type KeyLike = Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey'>;

/** Modifiers in a fixed order, then the main key (upper-cased when it is a single character). */
export function eventToKeyArray(e: KeyLike): string[] {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.metaKey) parts.push('Cmd');
  if (e.shiftKey) parts.push('Shift');
  if (e.altKey) parts.push('Alt');
  const key = normalizeKey(e.key);
  if (!MODIFIER_KEYS.has(e.key) && !MODIFIER_KEYS.has(key)) {
    parts.push(key.length === 1 ? key.toUpperCase() : key);
  }
  return parts;
}

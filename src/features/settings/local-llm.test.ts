import { describe, expect, it } from 'vitest';
import { providerHint, sortLocalModels, stateRow } from './local-llm';

describe('local refinement helpers', () => {
  it('hints only when something is missing', () => {
    expect(providerHint({ id: 'openrouter', name: 'OpenRouter', configured: true, key_source: 'saved', local: false, installed: true })).toBeUndefined();
    expect(providerHint({ id: 'megallm', name: 'MegaLLM', configured: false, key_source: 'none', local: false, installed: true })).toBe('No key');
    expect(providerHint({ id: 'ollama', name: 'Ollama', configured: false, key_source: 'none', local: true, installed: false })).toBe('Not installed');
    expect(providerHint({ id: 'lmstudio', name: 'LM Studio', configured: true, key_source: 'none', local: true, installed: true })).toBeUndefined();
  });

  it('puts usable models first, then by name', () => {
    const m = (id: string, usable: boolean) => ({ id, name: id, size_bytes: 1, usable, thinking: false });
    expect(sortLocalModels([m('b', false), m('c', true), m('a', true)]).map((x) => x.id)).toEqual(['a', 'c', 'b']);
  });

  it('shows a row only when there is something to say', () => {
    expect(stateRow('running', 'Ollama')).toBeNull();
    expect(stateRow('starting', 'LM Studio')).toEqual({ text: 'Starting LM Studio…', retry: false });
    expect(stateRow('failed', 'Ollama')).toEqual({ text: "Ollama didn't start", retry: true });
    expect(stateRow('not_local', 'Ollama')).toEqual({ text: 'Ollama is set to another computer', retry: false });
  });
});

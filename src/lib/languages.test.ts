import { describe, expect, it } from 'vitest';
import { languageLabel, languageName, sortLanguages } from './languages';

describe('languages', () => {
  it('names English, Bengali, and auto-detect', () => {
    expect(languageName('en')).toBe('English');
    expect(languageName('bn')).toBe('Bengali');
    expect(languageName('multi')).toMatch(/Auto-detect/);
    expect(languageLabel('bn')).toContain('বাংলা');
  });

  it('puts English and Bengali first', () => {
    expect(sortLanguages(['fr', 'bn', 'de', 'en', 'multi']).slice(0, 3)).toEqual(['en', 'bn', 'multi']);
  });
});

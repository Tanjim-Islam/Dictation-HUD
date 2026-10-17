// Display names for provider language codes. The codes come from the backend capability maps.

const PRIORITY = ['en', 'en-US', 'bn', 'multi'];

const english = new Intl.DisplayNames(['en'], { type: 'language', fallback: 'code' });

export function languageName(code: string): string {
  if (code === 'multi') return 'Auto-detect (multilingual)';
  const name = english.of(code) ?? code;
  if (code === 'bn') return 'Bengali';
  return name;
}

/** "Bengali (বাংলা)" style label with the native name when it differs. */
export function languageLabel(code: string): string {
  const name = languageName(code);
  if (code === 'multi') return name;
  try {
    const native = new Intl.DisplayNames([code], { type: 'language', fallback: 'none' }).of(code);
    if (native && native.toLowerCase() !== name.toLowerCase()) return `${name} (${native})`;
  } catch {
    // unknown locale
  }
  return name;
}

/** English and Bengali first, then alphabetical. */
export function sortLanguages(codes: string[]): string[] {
  return [...codes].sort((a, b) => {
    const pa = PRIORITY.indexOf(a);
    const pb = PRIORITY.indexOf(b);
    if (pa !== -1 || pb !== -1) return (pa === -1 ? 99 : pa) - (pb === -1 ? 99 : pb);
    return languageName(a).localeCompare(languageName(b));
  });
}

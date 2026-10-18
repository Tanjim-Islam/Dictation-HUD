// Pasting several ElevenLabs keys at once: detection and the toasts that report the result.
// The pasted text goes to Rust once (`elevenlabs_pool_import`); results are counts only.
import { toast } from 'sonner';
import { api, errorMessage, type ImportReport } from '@/lib/api';

/** True when pasted text holds more than a single bare key (several lines, NAME=value, lists). */
export function looksLikeKeyBlock(text: string) {
  const t = text.trim();
  return /[\r\n,;]/.test(t) || /[=:]/.test(t) || /\s/.test(t);
}

function plural(n: number, one: string, many = `${one}s`) {
  return `${n} ${n === 1 ? one : many}`;
}

function summary(r: ImportReport) {
  const parts = [
    r.duplicates && `${plural(r.duplicates, 'key')} already saved`,
    r.rejected && `${plural(r.rejected, 'key')} rejected by ElevenLabs`,
    r.unverified && `${plural(r.unverified, 'key')} could not be verified (network)`,
    r.failed && `${plural(r.failed, 'key')} could not be saved`,
  ].filter(Boolean);
  return parts.join(', ');
}

/** Toasts for a pasted block of keys. Offers to turn on Multiple APIs when it was off. */
export async function importPasted(text: string, onFinished?: () => void) {
  const { report } = await api.pool.importText(text);
  const extra = summary(report);
  if (report.multi) {
    if (report.added > 0) toast.success(`Added ${plural(report.added, 'API key')}`, { description: extra || 'Verified with ElevenLabs and stored in Windows Credential Manager.' });
    else toast.error('No new keys added', { description: extra });
  } else if (report.found > 1) {
    const first = report.added === 1 || report.duplicates === 1 ? 'The first key was added and is now active.' : `The first key was not added: ${extra}.`;
    toast.warning(`It looks like you pasted ${report.found} API keys, but Multiple APIs is off`, {
      description: `${first} Turn on Multiple APIs to use all of them.`,
      duration: 12_000,
      action: {
        label: 'Turn on and add all',
        onClick: () => {
          void (async () => {
            try {
              await api.pool.setMulti(true);
              await importPasted(text);
            } catch (e) {
              toast.error('Keys not added', { description: errorMessage(e) });
            }
          })();
        },
      },
    });
  } else if (report.added === 1 || report.duplicates === 1) {
    toast.success(report.added ? 'Key added' : 'Key already saved', { description: 'It is now the active ElevenLabs key.' });
  } else {
    toast.error('Key not added', { description: extra });
  }
  onFinished?.();
}


export const __test = { summary };

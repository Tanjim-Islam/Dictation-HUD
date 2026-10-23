import type { LocalLlmState, LocalModel, RefineProvider } from '@/lib/api';

/** Short hint shown next to a provider in the list; nothing when it is ready. */
export function providerHint(p: RefineProvider): string | undefined {
  if (p.local) return p.installed ? undefined : 'Not installed';
  return p.configured ? undefined : 'No key';
}

export function sortLocalModels(models: LocalModel[]): LocalModel[] {
  return [...models].sort((a, b) => Number(b.usable) - Number(a.usable) || a.name.localeCompare(b.name));
}

/** One status line under the model row, or null when the server is fine. */
export function stateRow(state: LocalLlmState, name: string): { text: string; retry: boolean } | null {
  switch (state) {
    case 'starting':
      return { text: `Starting ${name}…`, retry: false };
    case 'failed':
    case 'stopped':
      return { text: `${name} didn't start`, retry: true };
    case 'not_local':
      return { text: `${name} is set to another computer`, retry: false };
    default:
      return null;
  }
}

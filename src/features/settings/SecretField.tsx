import { useState } from 'react';
import { toast } from 'sonner';
import { Check, KeyRound, MoreHorizontal, RefreshCw, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge, Spinner } from '@/components/ui/misc';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { api, errorMessage, type KeySource } from '@/lib/api';

export function KeySourceBadge({ source }: { source: KeySource }) {
  if (source === 'saved')
    return (
      <Badge tone="success">
        <Check className="size-3" strokeWidth={2.5} />
        Saved
      </Badge>
    );
  if (source === 'dev_env') return <Badge tone="warning">From .env (dev build)</Badge>;
  return <Badge>Not set</Badge>;
}

/**
 * Write-only API key field. The typed key is sent to Rust once and stored in Windows
 * Credential Manager; the UI only ever learns whether a key exists.
 */
export function SecretField({
  provider,
  name,
  source,
  onChanged,
}: {
  provider: string;
  name: string;
  source: KeySource;
  onChanged: () => void;
}) {
  const [value, setValue] = useState('');
  const [busy, setBusy] = useState<'save' | 'test' | null>(null);

  async function save() {
    setBusy('save');
    try {
      await api.setProviderKey(provider, value);
      setValue('');
      toast.success(`${name} key saved`, { description: 'Stored in Windows Credential Manager.' });
      onChanged();
    } catch (e) {
      toast.error(`${name} key not saved`, { description: errorMessage(e) });
    } finally {
      setBusy(null);
    }
  }

  async function test() {
    setBusy('test');
    try {
      const r = await api.testProvider(provider);
      if (r.ok) {
        toast.success(r.message);
      } else {
        toast.error(r.message);
      }
    } catch (e) {
      toast.error(`${name} test failed`, { description: errorMessage(e) });
    } finally {
      setBusy(null);
    }
  }

  async function clear() {
    try {
      await api.clearProviderKey(provider);
      toast.success(`${name} key removed`);
      onChanged();
    } catch (e) {
      toast.error('Could not remove key', { description: errorMessage(e) });
    }
  }

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <div className="relative min-w-0 flex-1">
          <KeyRound className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-ink-faint" strokeWidth={1.75} />
          <Input
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && value.trim() && save()}
            placeholder={source === 'saved' ? 'Paste a new key to replace the saved one' : `Paste your ${name} API key`}
            aria-label={`${name} API key`}
            className="pl-9"
          />
        </div>
        <Button variant="primary" disabled={!value.trim() || busy !== null} onClick={save}>
          {busy === 'save' ? <Spinner className="text-panel" /> : 'Save'}
        </Button>
        <Button variant="secondary" disabled={source === 'none' || busy !== null} onClick={test}>
          {busy === 'test' ? <Spinner /> : <RefreshCw strokeWidth={1.75} />}
          Test
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon" aria-label={`More ${name} key actions`} disabled={source !== 'saved'}>
              <MoreHorizontal strokeWidth={1.75} />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem destructive onSelect={clear}>
              <Trash2 strokeWidth={1.75} />
              Remove saved key
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  );
}

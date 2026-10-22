import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Keyboard } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import { api, errorMessage } from '@/lib/api';
import { displayKey, eventToKeyArray } from '@/lib/shortcut';

export function Keycaps({ combo }: { combo: string }) {
  const keys = combo.split('+').filter(Boolean);
  return (
    <span className="inline-flex flex-wrap items-center gap-1">
      {keys.map((k) => (
        <kbd key={k} className="inline-flex h-6 min-w-6 items-center justify-center rounded-md border border-border bg-card px-1.5 font-sans text-xs font-medium text-ink shadow-[0_1px_0_rgb(0_0_0/0.06)]">
          {displayKey(k)}
        </kbd>
      ))}
    </span>
  );
}

/** Shows the current shortcut and records a new one. Rust registers it and saves it only if
 * registration succeeds. */
export function ShortcutField() {
  const [combo, setCombo] = useState('');
  const [open, setOpen] = useState(false);
  const [captured, setCaptured] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    api.getHotkey().then(setCombo).catch(() => {});
  }, []);

  useEffect(() => {
    if (!open) return;
    setCaptured([]);
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape' && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
        setOpen(false);
        return;
      }
      const keys = eventToKeyArray(e);
      if (keys.length) setCaptured(keys);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [open]);

  const modifiers = new Set(['Ctrl', 'Shift', 'Alt', 'Cmd']);
  const valid = captured.length >= 2 && captured.some((k) => modifiers.has(k)) && !modifiers.has(captured[captured.length - 1]);

  async function save() {
    const next = captured.join('+');
    setSaving(true);
    try {
      await api.setHotkey(next);
      setCombo(next);
      setOpen(false);
      toast.success('Shortcut updated');
    } catch (e) {
      toast.error('Shortcut not available', { description: errorMessage(e) });
    } finally {
      setSaving(false);
    }
  }

  return (
    <>
      <div className="flex items-center gap-3">
        {combo && <Keycaps combo={combo} />}
        <Button variant="secondary" className="w-[120px]" onClick={() => setOpen(true)}>
          Change
        </Button>
      </div>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="w-[min(420px,calc(100vw-48px))]">
          <DialogTitle>Change shortcut</DialogTitle>
          <DialogDescription>Press the keys you want to use. Use at least one modifier, like Ctrl or Alt.</DialogDescription>
          <div className="mt-5 flex h-20 items-center justify-center rounded-xl border border-dashed border-track bg-surface">
            {captured.length ? (
              <Keycaps combo={captured.join('+')} />
            ) : (
              <span className="flex items-center gap-2 text-[14px] text-ink-muted">
                <Keyboard className="size-4" strokeWidth={1.75} /> Waiting for keys
              </span>
            )}
          </div>
          <div className="mt-5 flex justify-end gap-2">
            <Button variant="secondary" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button variant="primary" disabled={!valid || saving} onClick={save}>
              Save shortcut
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}

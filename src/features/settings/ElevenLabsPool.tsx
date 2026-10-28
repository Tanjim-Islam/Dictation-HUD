import { useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { toast } from 'sonner';
import { Ban, KeyRound, Plus, RefreshCw, Trash2, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge, Spinner, type Tone } from '@/components/ui/misc';
import { Switch } from '@/components/ui/switch';
import { Tooltip } from '@/components/ui/tooltip';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { api, errorMessage, onPoolChange, type KeyStatus, type PoolKey, type PoolView } from '@/lib/api';
import { importPasted, looksLikeKeyBlock } from './elevenlabs-import';
import { transition } from '@/lib/motion';
import { cn, formatNumber } from '@/lib/utils';

const STATUS: Record<KeyStatus, { label: string; tone: Tone }> = {
  usable: { label: 'Active', tone: 'success' },
  unknown: { label: 'Checking', tone: 'neutral' },
  low_credit: { label: 'Low credit', tone: 'warning' },
  invalid: { label: 'Rejected', tone: 'danger' },
  cooldown: { label: 'Cooling down', tone: 'warning' },
  error: { label: 'Error', tone: 'danger' },
  no_credit_access: { label: 'Credits hidden', tone: 'neutral' },
  refused: { label: 'Refused', tone: 'warning' },
};

function credits(k: PoolKey) {
  if (k.remaining == null || k.limit == null) return '-';
  return `${formatNumber(k.remaining)} / ${formatNumber(k.limit)}`;
}

function useAgo(ms: number | null) {
  const [, tick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, []);
  if (!ms) return 'not checked yet';
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  return s < 5 ? 'just now' : s < 60 ? `${s}s ago` : `${Math.round(s / 60)}m ago`;
}

function KeyRow({ k, onRemove }: { k: PoolKey; onRemove: (id: string) => void }) {
  const [confirm, setConfirm] = useState(false);
  const status = STATUS[k.status];
  useEffect(() => {
    if (!confirm) return;
    const t = setTimeout(() => setConfirm(false), 3000);
    return () => clearTimeout(t);
  }, [confirm]);
  return (
    <motion.li
      layout
      initial={{ opacity: 0, y: -4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0, marginTop: 0, paddingTop: 0, paddingBottom: 0 }}
      transition={transition.base}
      className={cn('flex items-center gap-3 overflow-hidden px-3 py-2.5', !k.active && 'opacity-55')}
    >
      <KeyRound className="size-4 shrink-0 text-ink-faint" strokeWidth={1.75} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-[14px] font-medium text-ink">{k.label}</span>
          <span className="truncate font-mono text-xs tracking-wider text-ink-faint">••••••••</span>
        </div>
        {k.last_error && k.status !== 'usable' && <div className="truncate text-xs text-ink-muted">{k.last_error}</div>}
      </div>
      <span className="w-[132px] shrink-0 text-right text-[13px] tabular-nums text-ink-muted">{credits(k)}</span>
      <Badge tone={status.tone} className="w-[104px] shrink-0 justify-center">
        {status.label}
      </Badge>
      <Tooltip content={confirm ? 'Click again to delete' : 'Delete key'}>
        <Button
          variant={confirm ? 'danger' : 'subtle'}
          size="icon-sm"
          aria-label={`Delete ${k.label}`}
          onClick={() => (confirm ? onRemove(k.id) : setConfirm(true))}
        >
          <Trash2 strokeWidth={1.75} />
        </Button>
      </Tooltip>
    </motion.li>
  );
}

function AddKeyForm({ onDone, autoFocus, cancellable = true, multi }: { onDone: () => void; autoFocus?: boolean; cancellable?: boolean; multi: boolean }) {
  const [value, setValue] = useState('');
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (autoFocus) ref.current?.focus();
  }, [autoFocus]);
  async function add() {
    setBusy(true);
    try {
      await api.pool.add(value);
      setValue('');
      toast.success('Key added', { description: 'Verified with ElevenLabs and stored in Windows Credential Manager.' });
      onDone();
    } catch (e) {
      toast.error('Key not added', { description: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="flex items-center gap-2">
      <div className="relative min-w-0 flex-1">
        <KeyRound className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-ink-faint" strokeWidth={1.75} />
        <Input
          ref={ref}
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onPaste={(e) => {
            const text = e.clipboardData.getData('text');
            if (!looksLikeKeyBlock(text)) return;
            // A block of keys never lands in the field: it goes straight to Rust.
            e.preventDefault();
            setBusy(true);
            importPasted(text, onDone)
              .catch((err) => toast.error('Keys not added', { description: errorMessage(err) }))
              .finally(() => setBusy(false));
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && value.trim()) add();
            if (e.key === 'Escape') onDone();
          }}
          placeholder={multi ? 'Paste one or more ElevenLabs API keys' : 'Paste an ElevenLabs API key'}
          aria-label="New ElevenLabs API key"
          className="pl-9"
        />
      </div>
      <Button variant="primary" disabled={!value.trim() || busy} onClick={add} className="w-[84px]">
        {busy ? <Spinner className="text-panel" /> : 'Add'}
      </Button>
      {cancellable && (
        <Button variant="ghost" size="icon" aria-label="Cancel" onClick={onDone} disabled={busy}>
          <X strokeWidth={1.75} />
        </Button>
      )}
    </div>
  );
}

/** ElevenLabs keys: one key, or a pool of keys whose credits are combined and rotated. */
export function ElevenLabsPool() {
  const [pool, setPool] = useState<PoolView | null>(null);
  const [adding, setAdding] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);
  const [confirmRefused, setConfirmRefused] = useState(false);
  const lastChecked = Math.max(0, ...(pool?.keys.map((k) => k.last_checked_ms ?? 0) ?? [0])) || null;
  const ago = useAgo(lastChecked);

  useEffect(() => {
    api.pool.view().then(setPool).catch(() => {});
    // Live credits: Rust polls every few seconds while this view is on screen and pushes updates.
    api.pool.watch(true).catch(() => {});
    const un = onPoolChange(setPool);
    return () => {
      api.pool.watch(false).catch(() => {});
      un.then((f) => f());
    };
  }, []);

  if (!pool) return <Spinner />;

  async function remove(id: string) {
    try {
      setPool(await api.pool.remove(id));
    } catch (e) {
      toast.error('Could not delete key', { description: errorMessage(e) });
    }
  }

  async function clearAll() {
    try {
      setPool(await api.pool.clear());
      toast.success('All ElevenLabs keys deleted');
    } catch (e) {
      toast.error('Could not delete keys', { description: errorMessage(e) });
    }
  }

  async function removeRefused() {
    const ids = (pool?.keys ?? []).filter((k) => k.status === 'refused').map((k) => k.id);
    let removed = 0;
    try {
      for (const id of ids) {
        setPool(await api.pool.remove(id));
        removed += 1;
      }
      toast.success(`Removed ${removed} refused ${removed === 1 ? 'key' : 'keys'}`);
    } catch (e) {
      toast.error('Could not remove refused keys', { description: errorMessage(e) });
    }
  }

  async function refresh() {
    setRefreshing(true);
    try {
      setPool(await api.pool.refresh());
    } finally {
      setRefreshing(false);
    }
  }

  const hasKeys = pool.keys.length > 0;
  const refusedCount = pool.keys.filter((k) => k.status === 'refused').length;
  const pct = pool.total_limit > 0 ? Math.min(100, (pool.total_remaining / pool.total_limit) * 100) : 0;

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-6">
        <div className="min-w-0 flex-1">
          <div className="text-[15px] font-medium text-ink">Multiple APIs</div>
          <div className="mt-1 text-[14px] text-ink-muted">Pool several keys. Credits add up and dictation switches keys when one runs low.</div>
        </div>
        <Switch aria-label="Use multiple ElevenLabs API keys" checked={pool.multi} onCheckedChange={async (v) => setPool(await api.pool.setMulti(v))} />
      </div>

      {hasKeys && (
        <div className="rounded-xl border border-border bg-card px-4 py-3">
          <div className="flex items-baseline gap-2">
            <span className="text-[13px] font-medium text-ink-muted">{pool.multi ? 'Combined credits' : 'Credits'}</span>
            <span className="ml-auto flex items-center gap-1.5 text-xs text-ink-faint">
              <span className="relative flex size-1.5">
                <span className="absolute inline-flex size-full animate-ping rounded-full bg-success/60" />
                <span className="relative inline-flex size-1.5 rounded-full bg-success" />
              </span>
              Live, updated {ago}
            </span>
            <Tooltip content="Refresh now">
              <Button variant="subtle" size="icon-sm" aria-label="Refresh credits" onClick={refresh} disabled={refreshing}>
                <RefreshCw className={cn(refreshing && 'animate-spin')} strokeWidth={1.75} />
              </Button>
            </Tooltip>
          </div>
          <div className="mt-1 flex items-baseline gap-1.5">
            <motion.span key={pool.total_remaining} initial={{ opacity: 0.4 }} animate={{ opacity: 1 }} className="font-display text-[28px] leading-8 tabular-nums text-ink">
              {formatNumber(pool.total_remaining)}
            </motion.span>
            <span className="text-[15px] tabular-nums text-ink-muted">/ {formatNumber(pool.total_limit)}</span>
          </div>
          <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-surface-strong">
            <motion.div className="h-full rounded-full bg-accent" initial={false} animate={{ width: `${pct}%` }} transition={transition.slow} />
          </div>
        </div>
      )}

      {!pool.multi ? (
        <div className="flex flex-col gap-2">
          {hasKeys && (
            <ul className="divide-y divide-border rounded-xl border border-border bg-card">
              <KeyRow k={pool.keys[0]} onRemove={remove} />
            </ul>
          )}
          {!hasKeys || adding ? (
            <AddKeyForm onDone={() => setAdding(false)} autoFocus={adding} cancellable={hasKeys} multi={pool.multi} />
          ) : (
            <div className="flex items-center gap-2">
              <Button variant="secondary" onClick={() => setAdding(true)}>
                Replace key
              </Button>
              {pool.keys.length > 1 && <span className="text-[13px] text-ink-muted">{pool.keys.length - 1} more saved keys are used when Multiple APIs is on.</span>}
            </div>
          )}
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {hasKeys && (
            <ul className="max-h-[248px] divide-y divide-border overflow-y-auto overscroll-contain rounded-xl border border-border bg-card">
              <AnimatePresence initial={false}>
                {pool.keys.map((k) => (
                  <KeyRow key={k.id} k={k} onRemove={remove} />
                ))}
              </AnimatePresence>
            </ul>
          )}
          <AnimatePresence initial={false} mode="popLayout">
            {adding || !hasKeys ? (
              <motion.div key="form" initial={{ opacity: 0, y: -4 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={transition.base}>
                <AddKeyForm onDone={() => setAdding(false)} autoFocus={adding} cancellable={hasKeys} multi={pool.multi} />
              </motion.div>
            ) : (
              <motion.div key="actions" layout initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="flex items-center justify-between gap-2">
                <Button variant="secondary" onClick={() => setAdding(true)}>
                  <Plus strokeWidth={2} />
                  Add one more API key
                </Button>
                <div className="flex items-center gap-1">
                  {refusedCount > 0 && (
                    <Button variant="danger-ghost" onClick={() => setConfirmRefused(true)}>
                      <Ban strokeWidth={1.75} />
                      Remove refused
                    </Button>
                  )}
                  <Button variant="danger-ghost" onClick={() => setConfirmClear(true)}>
                    <Trash2 strokeWidth={1.75} />
                    Delete all
                  </Button>
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      )}

      {pool.using_dev_key && !hasKeys && <p className="text-[13px] text-warning">Development build: using the ElevenLabs key from .env until you add one here.</p>}

      <AlertDialog open={confirmClear} onOpenChange={setConfirmClear}>
        <AlertDialogContent>
          <AlertDialogTitle>Delete all ElevenLabs keys?</AlertDialogTitle>
          <AlertDialogDescription>
            This removes {pool.keys.length} {pool.keys.length === 1 ? 'key' : 'keys'} from Windows Credential Manager. ElevenLabs dictation stops working until you add a key again.
          </AlertDialogDescription>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction destructive onClick={clearAll}>
              Delete all
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={confirmRefused} onOpenChange={setConfirmRefused}>
        <AlertDialogContent>
          <AlertDialogTitle>
            Remove {refusedCount} refused {refusedCount === 1 ? 'key' : 'keys'}?
          </AlertDialogTitle>
          <AlertDialogDescription>
            ElevenLabs refused {refusedCount === 1 ? 'this key' : 'these keys'} although {refusedCount === 1 ? 'it shows' : 'they show'} credits, usually because a free account was flagged for unusual
            activity. {refusedCount === 1 ? 'It is' : 'They are'} deleted from Windows Credential Manager. Your other keys stay.
          </AlertDialogDescription>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction destructive onClick={removeRefused}>
              Remove refused
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

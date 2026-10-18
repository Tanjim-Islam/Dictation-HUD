import { useCallback, useEffect, useMemo, useState } from 'react';
import { toast } from 'sonner';
import { ArrowRight, Pencil, Trash2, X } from 'lucide-react';
import { api, errorMessage, onLexiconChange, type Snippet } from '@/lib/api';
import { Button } from '@/components/ui/button';
import { Input, Textarea } from '@/components/ui/input';
import { Spinner } from '@/components/ui/misc';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import { EmptyCard, ListCard, PageHeader, RowIconButton, SearchToggle, UnderlineTabs } from '@/components/page';

const INTRO_KEY = 'snippets-intro-dismissed';

function readDismissed() {
  try {
    return window.localStorage.getItem(INTRO_KEY) === '1';
  } catch {
    return false;
  }
}

function SnippetDialog({ snippet, open, onOpenChange }: { snippet: Snippet | null; open: boolean; onOpenChange: (v: boolean) => void }) {
  const [trigger, setTrigger] = useState('');
  const [expansion, setExpansion] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (open) {
      setTrigger(snippet?.trigger ?? '');
      setExpansion(snippet?.expansion ?? '');
      setError(null);
    }
  }, [open, snippet]);

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    setSaving(true);
    try {
      const input = { trigger, expansion };
      if (snippet) await api.snippets.update(snippet.id, input);
      else await api.snippets.add(input);
      onOpenChange(false);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[min(560px,calc(100vw-48px))]">
        <DialogTitle>{snippet ? 'Edit snippet' : 'New snippet'}</DialogTitle>
        <DialogDescription>Say the trigger while dictating and the text is inserted exactly as written.</DialogDescription>
        <form onSubmit={save} className="mt-5 space-y-4">
          <label className="block">
            <span className="mb-1.5 block text-[13px] font-medium text-ink">Trigger phrase</span>
            <Input value={trigger} onChange={(e) => setTrigger(e.target.value)} placeholder="e.g. my linkedin" autoFocus maxLength={60} aria-invalid={!!error} />
          </label>
          <label className="block">
            <span className="mb-1.5 flex items-baseline justify-between text-[13px] font-medium text-ink">
              Text to insert <span className="text-xs font-normal tabular-nums text-ink-muted">{expansion.length.toLocaleString()} / 4,000</span>
            </span>
            <Textarea value={expansion} onChange={(e) => setExpansion(e.target.value)} className="min-h-[140px]" placeholder="https://www.linkedin.com/in/your-name/" maxLength={4000} />
            <span className="mt-1.5 block text-xs text-ink-muted">Never changed by formatting or AI refinement.</span>
          </label>
          {error && <p className="text-[13px] text-danger">{error}</p>}
          <div className="flex justify-end gap-2 pt-1">
            <Button type="button" variant="secondary" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" disabled={saving || !trigger.trim() || !expansion.trim()}>
              {saving && <Spinner className="text-panel" />}
              {snippet ? 'Save' : 'Add snippet'}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function Intro({ onAdd, onDismiss }: { onAdd: () => void; onDismiss: () => void }) {
  const examples = [
    ['my linkedin', 'https://www.linkedin.com/in/your-name/'],
    ['intro email', 'Hi, great to meet you. Would you have time for a short call this week?'],
    ['sign off', 'Best regards,\nYour Name'],
  ];
  return (
    <div className="relative mt-6 overflow-hidden rounded-2xl bg-[#1d2321] px-8 py-7 text-white">
      <div className="pointer-events-none absolute -right-24 -top-24 size-72 rounded-full bg-[#2f5e57]/60 blur-3xl" aria-hidden />
      <div className="pointer-events-none absolute -bottom-28 right-40 size-64 rounded-full bg-[#7a5c2e]/40 blur-3xl" aria-hidden />
      <button type="button" aria-label="Dismiss" onClick={onDismiss} className="absolute right-4 top-4 flex size-7 items-center justify-center rounded-full bg-white/15 text-white/85 hover:bg-white/25">
        <X className="size-4" />
      </button>
      <h2 className="relative font-display text-[30px] leading-9">
        The things <em>you</em> keep typing.
      </h2>
      <p className="relative mt-2 text-[14px] text-white/80">Save text you use often, then say its trigger to insert it instantly.</p>
      <div className="relative mt-5 space-y-2">
        {examples.map(([t, e]) => (
          <div key={t} className="flex items-center gap-2 text-[13px]">
            <span className="rounded-lg bg-white/15 px-3 py-1.5 italic">“{t}”</span>
            <ArrowRight className="size-3.5 text-white/70" />
            <span className="truncate rounded-lg bg-white/90 px-3 py-1.5 text-ink">{e.split('\n')[0]}</span>
          </div>
        ))}
      </div>
      <Button className="relative mt-6 bg-white text-ink hover:bg-white/90" onClick={onAdd}>
        Add new snippet
      </Button>
    </div>
  );
}

function SnippetRow({ snippet, onEdit }: { snippet: Snippet; onEdit: (s: Snippet) => void }) {
  return (
    <div className="group flex min-h-[53px] items-center gap-3 px-4 py-3 transition-colors hover:bg-surface/70">
      <button type="button" onClick={() => onEdit(snippet)} className="flex min-w-0 flex-1 items-center gap-2 text-left text-[15px] outline-none focus-visible:underline">
        <span className="shrink-0 text-ink">{snippet.trigger}</span>
        <ArrowRight className="size-3.5 shrink-0 text-ink-faint" />
        <span className="truncate text-ink-muted">{snippet.expansion.replace(/\n/g, ' ⏎ ')}</span>
      </button>
      <div className="flex items-center gap-0.5 opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
        <RowIconButton label={`Edit ${snippet.trigger}`} onClick={() => onEdit(snippet)}>
          <Pencil />
        </RowIconButton>
        <RowIconButton
          label={`Delete ${snippet.trigger}`}
          className="hover:text-danger"
          onClick={() => api.snippets.remove(snippet.id).catch((e) => toast.error('Could not delete', { description: errorMessage(e) }))}
        >
          <Trash2 />
        </RowIconButton>
      </div>
    </div>
  );
}

export function SnippetsPage() {
  const [snippets, setSnippets] = useState<Snippet[] | null>(null);
  const [search, setSearch] = useState('');
  const [editing, setEditing] = useState<Snippet | null>(null);
  const [dialog, setDialog] = useState(false);
  const [dismissed, setDismissed] = useState(readDismissed);

  const load = useCallback(() => {
    api.snippets
      .list()
      .then(setSnippets)
      .catch((e) => toast.error('Could not load snippets', { description: errorMessage(e) }));
  }, []);
  useEffect(() => {
    load();
    const un = onLexiconChange(load);
    return () => {
      un.then((f) => f());
    };
  }, [load]);

  const shown = useMemo(() => {
    const q = search.trim().toLowerCase();
    return (snippets ?? []).filter((s) => !q || s.trigger.toLowerCase().includes(q) || s.expansion.toLowerCase().includes(q));
  }, [snippets, search]);

  const openAdd = () => {
    setEditing(null);
    setDialog(true);
  };
  const dismiss = () => {
    setDismissed(true);
    try {
      window.localStorage.setItem(INTRO_KEY, '1');
    } catch {
      // storage unavailable: the intro returns next time
    }
  };

  return (
    <div className="mx-auto max-w-[800px]">
      <PageHeader
        title="Snippets"
        actions={
          <Button variant="primary" onClick={openAdd}>
            Add new
          </Button>
        }
      />
      <UnderlineTabs
        label="Snippets"
        tabs={[]}
        value=""
        onChange={() => {}}
        leading={snippets ? `${snippets.length} ${snippets.length === 1 ? 'snippet' : 'snippets'}` : ''}
        trailing={<SearchToggle value={search} onChange={setSearch} placeholder="Search snippets" />}
      />
      {!dismissed && <Intro onAdd={openAdd} onDismiss={dismiss} />}
      {snippets === null ? (
        <div className="flex justify-center py-16">
          <Spinner />
        </div>
      ) : shown.length === 0 ? (
        search ? <EmptyCard title="Nothing matches" hint="Try a different search." /> : dismissed ? <EmptyCard title="No snippets yet" hint="Add text you type often and insert it by voice." action={<Button variant="primary" onClick={openAdd}>Add a snippet</Button>} /> : null
      ) : (
        <ListCard>
          {shown.map((s) => (
            <SnippetRow
              key={s.id}
              snippet={s}
              onEdit={(snip) => {
                setEditing(snip);
                setDialog(true);
              }}
            />
          ))}
        </ListCard>
      )}
      <SnippetDialog snippet={editing} open={dialog} onOpenChange={setDialog} />
    </div>
  );
}

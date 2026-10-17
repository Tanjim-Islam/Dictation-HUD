import { useCallback, useEffect, useMemo, useState } from 'react';
import { toast } from 'sonner';
import { ArrowRight, Check, Pencil, Sparkles, Trash2, X } from 'lucide-react';
import { api, errorMessage, onLexiconChange, type Term } from '@/lib/api';
import { Button } from '@/components/ui/button';
import { Input, Textarea } from '@/components/ui/input';
import { Spinner } from '@/components/ui/misc';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import { EmptyCard, ListCard, PageHeader, RowIconButton, SearchToggle, UnderlineTabs, type TabItem } from '@/components/page';
import { cn } from '@/lib/utils';

type Tab = 'all' | 'vocabulary' | 'corrections' | 'suggested';

function TermDialog({ term, open, onOpenChange }: { term: Term | null; open: boolean; onOpenChange: (v: boolean) => void }) {
  const [phrase, setPhrase] = useState('');
  const [heard, setHeard] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (open) {
      setPhrase(term?.phrase ?? '');
      setHeard(term?.heard ?? '');
      setError(null);
    }
  }, [open, term]);

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    setSaving(true);
    try {
      const input = { phrase, heard: heard.trim() || null };
      if (term) await api.dictionary.update(term.id, input);
      else await api.dictionary.add(input);
      onOpenChange(false);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[min(480px,calc(100vw-48px))]">
        <DialogTitle>{term ? 'Edit word' : 'Add to dictionary'}</DialogTitle>
        <DialogDescription>Names, jargon, and spellings you want transcribed exactly.</DialogDescription>
        <form onSubmit={save} className="mt-5 space-y-4">
          <label className="block">
            <span className="mb-1.5 block text-[13px] font-medium text-ink">Word or phrase</span>
            <Input value={phrase} onChange={(e) => setPhrase(e.target.value)} placeholder="e.g. Tanjim Riju" autoFocus maxLength={80} aria-invalid={!!error} />
          </label>
          <label className="block">
            <span className="mb-1.5 block text-[13px] font-medium text-ink">
              Replaces <span className="font-normal text-ink-muted">(optional)</span>
            </span>
            <Input value={heard} onChange={(e) => setHeard(e.target.value)} placeholder="What it gets transcribed as, e.g. tan jim" maxLength={80} />
            <span className="mt-1.5 block text-xs text-ink-muted">When set, this text is always replaced with the word above.</span>
          </label>
          {error && <p className="text-[13px] text-danger">{error}</p>}
          <div className="flex justify-end gap-2 pt-1">
            <Button type="button" variant="secondary" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" disabled={saving || !phrase.trim()}>
              {saving && <Spinner className="text-panel" />}
              {term ? 'Save' : 'Add'}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function ImportDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (open) setText('');
  }, [open]);
  const run = async () => {
    setBusy(true);
    try {
      const r = await api.dictionary.importText(text);
      toast.success(`Added ${r.added} ${r.added === 1 ? 'entry' : 'entries'}`, r.skipped ? { description: `${r.skipped} skipped (duplicates or invalid lines)` } : undefined);
      onOpenChange(false);
    } catch (e) {
      toast.error('Import failed', { description: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[min(520px,calc(100vw-48px))]">
        <DialogTitle>Add many</DialogTitle>
        <DialogDescription>One entry per line. Write a correction as “misheard -&gt; word”.</DialogDescription>
        <Textarea value={text} onChange={(e) => setText(e.target.value)} className="mt-5 min-h-[180px] font-mono text-[13px]" placeholder={'WACV\nDaraz\ntan jim -> Tanjim'} autoFocus />
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" onClick={run} disabled={busy || !text.trim()}>
            {busy && <Spinner className="text-panel" />}
            Add entries
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function TermRow({ term, onEdit }: { term: Term; onEdit: (t: Term) => void }) {
  const suggested = term.status === 'suggested';
  const act = (p: Promise<unknown>, failed: string) => p.catch((e) => toast.error(failed, { description: errorMessage(e) }));
  return (
    <div className={cn('group flex min-h-[53px] items-center gap-3 px-4 py-3 transition-colors hover:bg-surface/70', suggested && 'bg-accent-soft/40')}>
      <div className="flex min-w-0 flex-1 items-center gap-2 text-[15px]">
        {term.heard && (
          <>
            <span className="truncate text-ink-muted line-through decoration-ink-faint/60">{term.heard}</span>
            <ArrowRight className="size-3.5 shrink-0 text-ink-faint" />
          </>
        )}
        <span className="truncate text-ink">{term.phrase}</span>
        {term.source === 'learned' && <Sparkles className="size-[15px] shrink-0 text-[#d9a43a]" strokeWidth={1.8} aria-label="Learned from your corrections" />}
        {suggested && <span className="shrink-0 text-xs text-accent">Suggested</span>}
      </div>
      <div className={cn('flex items-center gap-0.5 transition-opacity', suggested ? 'opacity-100' : 'opacity-0 group-hover:opacity-100 group-focus-within:opacity-100')}>
        {suggested ? (
          <>
            <Button size="sm" variant="primary" onClick={() => act(api.dictionary.accept(term.id), 'Could not accept')}>
              <Check /> Add
            </Button>
            <RowIconButton label="Dismiss suggestion" onClick={() => act(api.dictionary.remove(term.id), 'Could not dismiss')}>
              <X />
            </RowIconButton>
          </>
        ) : (
          <>
            <RowIconButton label={`Edit ${term.phrase}`} onClick={() => onEdit(term)}>
              <Pencil />
            </RowIconButton>
            <RowIconButton label={`Delete ${term.phrase}`} className="hover:text-danger" onClick={() => act(api.dictionary.remove(term.id), 'Could not delete')}>
              <Trash2 />
            </RowIconButton>
          </>
        )}
      </div>
    </div>
  );
}

export function DictionaryPage() {
  const [terms, setTerms] = useState<Term[] | null>(null);
  const [tab, setTab] = useState<Tab>('all');
  const [search, setSearch] = useState('');
  const [editing, setEditing] = useState<Term | null>(null);
  const [dialog, setDialog] = useState(false);
  const [importing, setImporting] = useState(false);

  const load = useCallback(() => {
    api.dictionary
      .list()
      .then(setTerms)
      .catch((e) => toast.error('Could not load the dictionary', { description: errorMessage(e) }));
  }, []);
  useEffect(() => {
    load();
    const un = onLexiconChange(load);
    return () => {
      un.then((f) => f());
    };
  }, [load]);

  const suggested = terms?.filter((t) => t.status === 'suggested').length ?? 0;
  const tabs: TabItem<Tab>[] = [
    { id: 'all', label: 'All' },
    { id: 'vocabulary', label: 'Vocabulary' },
    { id: 'corrections', label: 'Corrections' },
    { id: 'suggested', label: 'Suggested', count: suggested },
  ];

  const shown = useMemo(() => {
    const q = search.trim().toLowerCase();
    return (terms ?? [])
      .filter((t) => (tab === 'vocabulary' ? t.status === 'active' && !t.heard : tab === 'corrections' ? t.status === 'active' && !!t.heard : tab === 'suggested' ? t.status === 'suggested' : true))
      .filter((t) => !q || t.phrase.toLowerCase().includes(q) || t.heard?.toLowerCase().includes(q));
  }, [terms, tab, search]);

  const openAdd = () => {
    setEditing(null);
    setDialog(true);
  };

  return (
    <div className="mx-auto max-w-[800px]">
      <PageHeader
        title="Dictionary"
        actions={
          <>
            <Button variant="primary" onClick={() => setImporting(true)}>
              Add many
            </Button>
            <Button variant="primary" onClick={openAdd}>
              Add new
            </Button>
          </>
        }
      />
      <UnderlineTabs label="Dictionary filter" tabs={tabs} value={tab} onChange={setTab} trailing={<SearchToggle value={search} onChange={setSearch} placeholder="Search dictionary" />} />
      {terms === null ? (
        <div className="flex justify-center py-16">
          <Spinner />
        </div>
      ) : shown.length === 0 ? (
        search || tab !== 'all' ? (
          <EmptyCard title="Nothing here" hint={tab === 'suggested' ? 'Suggestions appear when you fix a word Dictation HUD got wrong.' : 'Try a different search or tab.'} />
        ) : (
          <EmptyCard
            title="Teach Dictation HUD your words"
            hint="Add names, product terms, and acronyms. They are sent to the speech provider as hints and kept exactly as you spell them."
            action={
              <Button variant="primary" onClick={openAdd}>
                Add a word
              </Button>
            }
          />
        )
      ) : (
        <ListCard>
          {shown.map((t) => (
            <TermRow
              key={t.id}
              term={t}
              onEdit={(term) => {
                setEditing(term);
                setDialog(true);
              }}
            />
          ))}
        </ListCard>
      )}
      <TermDialog term={editing} open={dialog} onOpenChange={setDialog} />
      <ImportDialog open={importing} onOpenChange={setImporting} />
    </div>
  );
}

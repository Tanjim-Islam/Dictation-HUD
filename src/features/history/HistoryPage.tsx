import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AudioLines, Search, X } from 'lucide-react';
import { api, errorMessage, onHistoryChange, type HistoryFilter, type HistoryItem } from '@/lib/api';
import { Keycaps } from '@/features/settings/ShortcutField';
import { Spinner } from '@/components/ui/misc';
import { cn } from '@/lib/utils';
import { HistoryActionsProvider } from './actions';
import { dayKey, dayLabel } from './format';
import { HistoryDetailDialog } from './HistoryDetailDialog';
import { HistoryRow } from './HistoryRow';
import { PlayerProvider } from './player';

const PAGE = 60;

const FILTERS: { id: HistoryFilter; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'needs_attention', label: 'Needs attention' },
  { id: 'with_audio', label: 'With recording' },
];

function greeting() {
  const h = new Date().getHours();
  return h < 12 ? 'Good morning' : h < 18 ? 'Good afternoon' : 'Good evening';
}

function useDebounced<T>(value: T, ms: number) {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = window.setTimeout(() => setV(value), ms);
    return () => window.clearTimeout(t);
  }, [value, ms]);
  return v;
}

function DictateCard() {
  const [hotkey, setHotkey] = useState('');
  useEffect(() => {
    api.getHotkey().then(setHotkey).catch(() => {});
  }, []);
  return (
    <div className="mt-7 flex items-center gap-5 rounded-2xl bg-surface p-6">
      <div className="flex size-12 shrink-0 items-center justify-center rounded-xl bg-ink text-panel">
        <AudioLines className="size-6" strokeWidth={2} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="text-[17px] font-semibold">Dictate anywhere</div>
        <div className="mt-1 flex flex-wrap items-center gap-2 text-[14px] text-ink-muted">
          Press {hotkey ? <Keycaps combo={hotkey} /> : 'your shortcut'} in any app, speak, then press it again.
        </div>
      </div>
    </div>
  );
}

function Toolbar({
  filter,
  onFilter,
  search,
  onSearch,
}: {
  filter: HistoryFilter;
  onFilter: (f: HistoryFilter) => void;
  search: string;
  onSearch: (s: string) => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  return (
    <div className="mt-9 flex items-center gap-3">
      <div className="flex items-center gap-1" role="radiogroup" aria-label="Filter history">
        {FILTERS.map((f) => (
          <button
            key={f.id}
            type="button"
            role="radio"
            aria-checked={filter === f.id}
            onClick={() => onFilter(f.id)}
            className={cn(
              'h-8 rounded-full px-3.5 text-[13px] font-medium transition-colors',
              filter === f.id ? 'bg-ink text-panel' : 'text-ink-muted hover:bg-surface-strong hover:text-ink',
            )}
          >
            {f.label}
          </button>
        ))}
      </div>
      <div className="flex-1" />
      <label className="relative flex h-8 w-[240px] items-center rounded-full bg-surface px-3 transition-colors focus-within:bg-surface-strong">
        <Search className="size-[15px] shrink-0 text-ink-muted" strokeWidth={1.8} />
        <input
          ref={input}
          value={search}
          onChange={(e) => onSearch(e.target.value)}
          onKeyDown={(e) => e.key === 'Escape' && onSearch('')}
          placeholder="Search transcripts"
          aria-label="Search transcripts"
          className="ml-2 min-w-0 flex-1 bg-transparent text-[13px] text-ink outline-none placeholder:text-ink-muted"
        />
        {search && (
          <button
            type="button"
            aria-label="Clear search"
            onClick={() => {
              onSearch('');
              input.current?.focus();
            }}
            className="-mr-1 flex size-6 items-center justify-center rounded-full text-ink-muted hover:text-ink"
          >
            <X className="size-3.5" />
          </button>
        )}
      </label>
    </div>
  );
}

function EmptyState({ filtered }: { filtered: boolean }) {
  return (
    <div className="mt-6 rounded-2xl border border-dashed border-border px-6 py-14 text-center">
      <div className="text-[15px] font-medium text-ink">{filtered ? 'Nothing matches' : 'No dictations yet'}</div>
      <div className="mt-1 text-[14px] text-ink-muted">{filtered ? 'Try a different search or filter.' : 'Your transcripts will appear here.'}</div>
    </div>
  );
}

export function HistoryPage() {
  const [items, setItems] = useState<HistoryItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [more, setMore] = useState(false);
  const [filter, setFilter] = useState<HistoryFilter>('all');
  const [search, setSearch] = useState('');
  const [open, setOpen] = useState<string | null>(null);
  const query = useDebounced(search.trim(), 180);
  const count = useRef(PAGE);
  const sentinel = useRef<HTMLDivElement>(null);
  const loadingMore = useRef(false);

  const reload = useCallback(async () => {
    try {
      const limit = Math.max(PAGE, count.current);
      const rows = await api.history.list({ search: query || undefined, filter, limit });
      setItems(rows);
      setMore(rows.length >= limit);
    } catch (e) {
      console.warn('history load failed', errorMessage(e));
    } finally {
      setLoading(false);
    }
  }, [query, filter]);

  useEffect(() => {
    count.current = PAGE;
    setLoading(true);
    void reload();
  }, [reload]);

  useEffect(() => {
    const un = onHistoryChange(() => void reload());
    return () => {
      un.then((f) => f());
    };
  }, [reload]);

  const loadMore = useCallback(async () => {
    if (loadingMore.current || !more || items.length === 0) return;
    loadingMore.current = true;
    try {
      const rows = await api.history.list({ search: query || undefined, filter, before: items[items.length - 1].created_at, limit: PAGE });
      setItems((prev) => [...prev, ...rows]);
      count.current = items.length + rows.length;
      setMore(rows.length >= PAGE);
    } finally {
      loadingMore.current = false;
    }
  }, [more, items, query, filter]);

  useEffect(() => {
    const el = sentinel.current;
    if (!el) return;
    const io = new IntersectionObserver((entries) => entries[0]?.isIntersecting && void loadMore(), { rootMargin: '400px' });
    io.observe(el);
    return () => io.disconnect();
  }, [loadMore]);

  const groups = useMemo(() => {
    const out: { key: string; label: string; items: HistoryItem[] }[] = [];
    for (const it of items) {
      const key = dayKey(it.created_at);
      const last = out[out.length - 1];
      if (last?.key === key) last.items.push(it);
      else out.push({ key, label: dayLabel(it.created_at), items: [it] });
    }
    return out;
  }, [items]);

  const onDeleted = useCallback((id: string) => {
    setItems((prev) => prev.filter((i) => i.id !== id));
    setOpen((o) => (o === id ? null : o));
  }, []);
  const closeDetail = useCallback(() => setOpen(null), []);

  return (
    <PlayerProvider>
      <HistoryActionsProvider onDeleted={onDeleted}>
        <div className="mx-auto max-w-[768px]">
          <h1 className="text-2xl font-semibold tracking-[-0.01em]">{greeting()}</h1>
          <DictateCard />
          <Toolbar filter={filter} onFilter={setFilter} search={search} onSearch={setSearch} />
          {loading ? (
            <div className="flex justify-center py-16">
              <Spinner />
            </div>
          ) : groups.length === 0 ? (
            <EmptyState filtered={filter !== 'all' || query !== ''} />
          ) : (
            groups.map((g) => (
              <section key={g.key} className="mt-7" aria-label={g.label}>
                <h2 className="mb-3 text-xs font-medium uppercase tracking-[0.08em] text-ink-muted">{g.label}</h2>
                <div className="divide-y divide-border overflow-hidden rounded-2xl border border-border bg-panel">
                  {g.items.map((it) => (
                    <HistoryRow key={it.id} item={it} onOpen={setOpen} />
                  ))}
                </div>
              </section>
            ))
          )}
          <div ref={sentinel} className="h-px" />
          {more && !loading && (
            <div className="flex justify-center py-6">
              <Spinner />
            </div>
          )}
        </div>
        <HistoryDetailDialog id={open} onClose={closeDetail} />
      </HistoryActionsProvider>
    </PlayerProvider>
  );
}

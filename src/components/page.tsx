import * as React from 'react';
import { motion } from 'motion/react';
import { Search, X } from 'lucide-react';
import { transition } from '@/lib/motion';
import { cn } from '@/lib/utils';

/** Page title with actions on the right (Wispr-style list pages). */
export function PageHeader({ title, actions }: { title: string; actions?: React.ReactNode }) {
  return (
    <div className="flex items-center gap-4">
      <h1 className="flex-1 text-[26px] font-semibold tracking-[-0.01em]">{title}</h1>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  );
}

export type TabItem<T extends string> = { id: T; label: string; count?: number };

/** Underlined tabs with a trailing slot (search) on one hairline. */
export function UnderlineTabs<T extends string>({
  tabs,
  value,
  onChange,
  trailing,
  label,
  leading,
}: {
  tabs: TabItem<T>[];
  value: T;
  onChange: (v: T) => void;
  trailing?: React.ReactNode;
  label: string;
  /** Shown instead of tabs when there are none (for example a count). */
  leading?: React.ReactNode;
}) {
  return (
    <div className="mt-8 flex items-center border-b border-border">
      {tabs.length === 0 && <div className="flex-1 pb-2.5 text-[14px] text-ink-muted">{leading}</div>}
      <div role="tablist" aria-label={label} className={cn('flex items-center gap-6', tabs.length > 0 ? 'flex-1' : 'hidden')}>
        {tabs.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={value === t.id}
            onClick={() => onChange(t.id)}
            className={cn('relative pb-2.5 text-[15px] font-medium transition-colors', value === t.id ? 'text-ink' : 'text-ink-muted hover:text-ink')}
          >
            {t.label}
            {t.count ? <span className="ml-1.5 rounded-full bg-surface-strong px-1.5 py-px text-xs tabular-nums text-ink-muted">{t.count}</span> : null}
            {value === t.id && <motion.span layoutId={`tab-${label}`} className="absolute inset-x-0 -bottom-px h-[2px] rounded-full bg-ink" transition={transition.layout} />}
          </button>
        ))}
      </div>
      <div className="flex items-center gap-1 pb-1.5">{trailing}</div>
    </div>
  );
}

/** Search that expands from an icon button; Escape clears and closes. */
export function SearchToggle({ value, onChange, placeholder }: { value: string; onChange: (v: string) => void; placeholder: string }) {
  const [open, setOpen] = React.useState(value !== '');
  const input = React.useRef<HTMLInputElement>(null);
  React.useEffect(() => {
    if (open) input.current?.focus();
  }, [open]);
  if (!open) {
    return (
      <button
        type="button"
        aria-label={placeholder}
        title="Search"
        onClick={() => setOpen(true)}
        className="flex size-8 items-center justify-center rounded-lg text-ink-muted transition-colors hover:bg-surface-strong hover:text-ink"
      >
        <Search className="size-[17px]" strokeWidth={1.8} />
      </button>
    );
  }
  return (
    <label className="flex h-8 w-[220px] items-center rounded-full bg-surface px-3 focus-within:bg-surface-strong">
      <Search className="size-[15px] shrink-0 text-ink-muted" strokeWidth={1.8} />
      <input
        ref={input}
        value={value}
        aria-label={placeholder}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            onChange('');
            setOpen(false);
          }
        }}
        onBlur={() => value === '' && setOpen(false)}
        className="ml-2 min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-ink-muted"
      />
      {value && (
        <button type="button" aria-label="Clear search" onClick={() => onChange('')} className="-mr-1 flex size-6 items-center justify-center text-ink-muted hover:text-ink">
          <X className="size-3.5" />
        </button>
      )}
    </label>
  );
}

/** Rounded bordered list with hairline dividers. */
export function ListCard({ children, className }: { children: React.ReactNode; className?: string }) {
  return <div className={cn('mt-6 divide-y divide-border overflow-hidden rounded-2xl border border-border bg-panel', className)}>{children}</div>;
}

export function EmptyCard({ title, hint, action }: { title: string; hint: string; action?: React.ReactNode }) {
  return (
    <div className="mt-6 flex flex-col items-center rounded-2xl border border-dashed border-border px-6 py-14 text-center">
      <div className="text-[15px] font-medium text-ink">{title}</div>
      <div className="mt-1 max-w-[420px] text-[14px] text-ink-muted">{hint}</div>
      {action && <div className="mt-5">{action}</div>}
    </div>
  );
}

/** Small icon button for row hover actions. */
export const RowIconButton = React.forwardRef<HTMLButtonElement, React.ButtonHTMLAttributes<HTMLButtonElement> & { label: string }>(
  ({ label, className, children, ...props }, ref) => (
    <button
      ref={ref}
      type="button"
      aria-label={label}
      title={label}
      className={cn('flex size-7 items-center justify-center rounded-lg text-ink-muted transition-colors hover:bg-surface-strong hover:text-ink [&_svg]:size-[15px]', className)}
      {...props}
    >
      {children}
    </button>
  ),
);
RowIconButton.displayName = 'RowIconButton';

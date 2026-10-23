import { X } from 'lucide-react';

/** Thin bar, percent, and cancel. Used for local text models and speech models. */
export function DownloadProgress({ pct, onCancel }: { pct: number; onCancel: () => void }) {
  const value = Math.round(pct * 100);
  return (
    <span className="flex items-center gap-2">
      <span className="h-1 w-28 overflow-hidden rounded bg-track" role="progressbar" aria-valuenow={value} aria-valuemin={0} aria-valuemax={100}>
        <span className="block h-full bg-accent transition-[width]" style={{ width: `${value}%` }} />
      </span>
      <span className="w-9 text-right text-xs tabular-nums text-ink-muted">{value}%</span>
      <button type="button" aria-label="Cancel download" onClick={onCancel}>
        <X className="size-4 text-ink-muted" />
      </button>
    </span>
  );
}

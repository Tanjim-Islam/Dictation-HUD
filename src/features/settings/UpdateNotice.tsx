import { useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { ArrowDownToLine, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/misc';
import { transition } from '@/lib/motion';
import { installUpdate, useUpdateState } from './updater';

/** Sidebar card shown above Settings when a newer version is available. */
export function UpdateNotice() {
  const s = useUpdateState();
  // "Later" hides the card until the next launch; installing always shows progress.
  const [dismissed, setDismissed] = useState(false);
  const visible = (s.status === 'available' && !dismissed) || s.status === 'downloading' || s.status === 'ready';

  return (
    <AnimatePresence initial={false}>
      {visible && (
        <motion.div
          key="update"
          initial={{ opacity: 0, y: 6, height: 0 }}
          animate={{ opacity: 1, y: 0, height: 'auto' }}
          exit={{ opacity: 0, y: 6, height: 0 }}
          transition={transition.base}
          className="overflow-hidden"
        >
          <div role="status" aria-live="polite" className="relative mb-3 rounded-xl border border-border bg-card p-3 shadow-[0_1px_2px_rgb(26_26_26/0.04)]">
            {s.status === 'available' && (
              <button
                type="button"
                aria-label="Remind me later"
                title="Later"
                onClick={() => setDismissed(true)}
                className="absolute right-2 top-2 flex size-6 items-center justify-center rounded-md text-ink-faint transition-colors hover:bg-surface hover:text-ink"
              >
                <X className="size-3.5" strokeWidth={2} />
              </button>
            )}
            <div className="flex items-center gap-2 pr-6">
              <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-accent-soft text-accent">
                <ArrowDownToLine className="size-3.5" strokeWidth={2} />
              </span>
              <span className="text-[13px] font-semibold text-ink">Update available</span>
            </div>
            {s.status === 'available' && (
              <>
                <p className="mt-1.5 text-xs text-ink-muted">Version {s.version} is ready to install.</p>
                <Button variant="primary" size="sm" className="mt-2.5 h-7 w-full text-xs" onClick={() => void installUpdate()}>
                  Install and restart
                </Button>
              </>
            )}
            {s.status === 'downloading' && (
              <>
                <p className="mt-1.5 text-xs tabular-nums text-ink-muted">Downloading {s.progress}%</p>
                <div className="mt-2 h-1 overflow-hidden rounded-full bg-surface-strong">
                  <motion.div className="h-full rounded-full bg-accent" initial={false} animate={{ width: `${s.progress}%` }} transition={transition.base} />
                </div>
              </>
            )}
            {s.status === 'ready' && (
              <p className="mt-1.5 flex items-center gap-1.5 text-xs text-ink-muted">
                <Spinner className="size-3" /> Restarting to finish the update...
              </p>
            )}
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

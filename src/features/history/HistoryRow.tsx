import { memo } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { Copy, Download, FileText, LifeBuoy, MoreHorizontal, RotateCcw, Sparkles, Trash2, VolumeX } from 'lucide-react';
import type { HistoryItem } from '@/lib/api';
import { Badge, Spinner } from '@/components/ui/misc';
import { Tooltip } from '@/components/ui/tooltip';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { transition } from '@/lib/motion';
import { cn } from '@/lib/utils';
import { useHistoryActions } from './actions-context';
import { displayText, emptyLine, hasAudio, needsAttention, rowTag, timeLabel } from './format';
import { PlayButton, PlayerBar } from './player';
import { usePlayer } from './player-context';

const iconButton = 'flex size-7 items-center justify-center rounded-lg text-ink-muted transition-colors hover:bg-surface-strong hover:text-ink data-[state=open]:bg-surface-strong data-[state=open]:text-ink';

export function ItemMenu({ item, onDetails, align = 'end' }: { item: HistoryItem; onDetails?: () => void; align?: 'start' | 'end' }) {
  const a = useHistoryActions();
  const text = displayText(item);
  const audio = hasAudio(item);
  const busy = a.busy.has(item.id);
  const running = item.status === 'in_progress';
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className={iconButton} aria-label="More actions" title="More actions">
        <MoreHorizontal className="size-[16px]" strokeWidth={1.8} />
      </DropdownMenuTrigger>
      <DropdownMenuContent align={align} className="min-w-[210px]">
        {onDetails && (
          <DropdownMenuItem onSelect={onDetails}>
            <FileText /> Details
          </DropdownMenuItem>
        )}
        {needsAttention(item) && (
          <DropdownMenuItem disabled={busy || running} onSelect={() => a.recover(item)}>
            <LifeBuoy /> Recover
          </DropdownMenuItem>
        )}
        {audio && (
          <DropdownMenuItem disabled={busy || running} onSelect={() => a.retry(item)}>
            <RotateCcw /> Retry transcript
          </DropdownMenuItem>
        )}
        {item.raw_text?.trim() && (
          <DropdownMenuItem disabled={busy || running} onSelect={() => a.retryRefinement(item)}>
            <Sparkles /> Retry refinement
          </DropdownMenuItem>
        )}
        {text && (
          <DropdownMenuItem onSelect={() => a.copy(item)}>
            <Copy /> Copy transcript
          </DropdownMenuItem>
        )}
        {audio && (
          <DropdownMenuItem onSelect={() => a.exportWav(item)}>
            <Download /> Export WAV
          </DropdownMenuItem>
        )}
        <DropdownMenuSeparator />
        {audio && (
          <DropdownMenuItem disabled={busy || running} onSelect={() => a.askDeleteAudio(item)}>
            <VolumeX /> Delete recording
          </DropdownMenuItem>
        )}
        <DropdownMenuItem destructive disabled={busy || running} onSelect={() => a.askDelete(item)}>
          <Trash2 /> Delete transcript
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export const HistoryRow = memo(function HistoryRow({ item, onOpen }: { item: HistoryItem; onOpen: (id: string) => void }) {
  const a = useHistoryActions();
  const player = usePlayer();
  const text = displayText(item);
  const tag = rowTag(item);
  const busy = a.busy.has(item.id);
  const playing = player.id === item.id;

  return (
    <div
      className={cn('group relative grid grid-cols-[88px_1fr_auto] items-start gap-x-4 px-4 py-[18px] transition-colors hover:bg-surface/70 focus-within:bg-surface/70', playing && 'bg-surface/70')}
    >
      <div className="pt-px text-[13px] leading-[22px] text-ink-muted">{timeLabel(item.created_at)}</div>
      <div className="min-w-0">
        <button
          type="button"
          onClick={() => onOpen(item.id)}
          className={cn('block w-full text-left text-[15px] leading-[22px] outline-none focus-visible:underline', text ? 'text-ink' : 'italic text-ink-muted')}
        >
          <span className={cn('line-clamp-4 whitespace-pre-wrap break-words', busy && 'animate-pulse')}>{text || emptyLine(item)}</span>
        </button>
        {tag && (
          <div className="mt-1.5">
            <Badge tone={tag.tone}>{tag.label}</Badge>
          </div>
        )}
        <AnimatePresence initial={false}>
          {playing && (
            <motion.div key="player" initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} transition={transition.base} className="overflow-hidden">
              <div className="pt-2.5">
                <PlayerBar id={item.id} durationMs={item.audio_ms ?? item.duration_ms} />
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
      <div className={cn('flex h-[22px] w-[88px] items-center justify-end gap-0.5 opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100 has-[[data-state=open]]:opacity-100', (busy || playing) && 'opacity-100')}>
        {busy ? (
          <span className="flex size-7 items-center justify-center" aria-label="Working">
            <Spinner className="size-[15px]" />
          </span>
        ) : (
          <>
            {hasAudio(item) && !playing && <PlayButton id={item.id} durationMs={item.audio_ms ?? item.duration_ms} />}
            {text && (
              <Tooltip content="Copy">
                <button type="button" className={iconButton} aria-label="Copy transcript" onClick={() => a.copy(item)}>
                  <Copy className="size-[15px]" strokeWidth={1.8} />
                </button>
              </Tooltip>
            )}
          </>
        )}
        <ItemMenu item={item} onDetails={() => onOpen(item.id)} />
      </div>
    </div>
  );
});

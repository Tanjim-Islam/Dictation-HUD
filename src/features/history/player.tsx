import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Slider } from 'radix-ui';
import { Pause, Play } from 'lucide-react';
import { toast } from 'sonner';
import { api, errorMessage } from '@/lib/api';
import { Spinner } from '@/components/ui/misc';
import { cn } from '@/lib/utils';
import { clock } from './format';
import { PlayerContext, usePlayer, type PlayerState } from './player-context';

const IDLE: PlayerState = { id: null, playing: false, loading: false, position: 0, duration: 0 };

/**
 * One recording plays at a time. Audio bytes come from Rust and play through Web Audio:
 * WebView2 refuses blob: and data: sources for media elements, while decodeAudioData works.
 */
export function PlayerProvider({ children }: { children: React.ReactNode }) {
  const [state, setState] = useState<PlayerState>(IDLE);
  const ctx = useRef<AudioContext | null>(null);
  const buffer = useRef<AudioBuffer | null>(null);
  const source = useRef<AudioBufferSourceNode | null>(null);
  // Playback clock: position = ctx.currentTime - startedAt while playing, else `offset`.
  const startedAt = useRef(0);
  const offset = useRef(0);
  const frame = useRef(0);
  const requested = useRef<string | null>(null);

  const halt = useCallback(() => {
    cancelAnimationFrame(frame.current);
    const node = source.current;
    source.current = null;
    if (node) {
      node.onended = null;
      try {
        node.stop();
      } catch {
        // already stopped
      }
    }
  }, []);

  const tick = useCallback(() => {
    const c = ctx.current;
    if (!c || !source.current) return;
    setState((s) => ({ ...s, position: (c.currentTime - startedAt.current) * 1000 }));
    frame.current = requestAnimationFrame(tick);
  }, []);

  const startAt = useCallback(
    (seconds: number) => {
      const c = ctx.current;
      const buf = buffer.current;
      if (!c || !buf) return;
      halt();
      const node = c.createBufferSource();
      node.buffer = buf;
      node.connect(c.destination);
      node.onended = () => {
        if (source.current !== node) return;
        source.current = null;
        offset.current = 0;
        cancelAnimationFrame(frame.current);
        setState((s) => ({ ...s, playing: false, position: 0 }));
      };
      const at = Math.min(Math.max(seconds, 0), buf.duration);
      node.start(0, at);
      source.current = node;
      startedAt.current = c.currentTime - at;
      setState((s) => ({ ...s, playing: true, loading: false, position: at * 1000 }));
      frame.current = requestAnimationFrame(tick);
    },
    [halt, tick],
  );

  const pause = useCallback(() => {
    const c = ctx.current;
    if (!c || !source.current) return;
    offset.current = c.currentTime - startedAt.current;
    halt();
    setState((s) => ({ ...s, playing: false, position: offset.current * 1000 }));
  }, [halt]);

  const stop = useCallback(() => {
    requested.current = null;
    halt();
    buffer.current = null;
    offset.current = 0;
    setState(IDLE);
  }, [halt]);

  useEffect(
    () => () => {
      halt();
      void ctx.current?.close();
    },
    [halt],
  );

  const toggle = useCallback(
    async (id: string, durationMs: number | null) => {
      if (state.id === id && buffer.current) {
        if (source.current) pause();
        else startAt(offset.current);
        return;
      }
      halt();
      buffer.current = null;
      offset.current = 0;
      requested.current = id;
      setState({ id, playing: false, loading: true, position: 0, duration: durationMs ?? 0 });
      try {
        ctx.current ??= new AudioContext();
        if (ctx.current.state === 'suspended') await ctx.current.resume();
        const bytes = await api.history.audio(id);
        const decoded = await ctx.current.decodeAudioData(bytes);
        if (requested.current !== id) return;
        buffer.current = decoded;
        setState((s) => ({ ...s, duration: decoded.duration * 1000 }));
        startAt(0);
      } catch (e) {
        if (requested.current === id) {
          stop();
          toast.error('Could not play the recording', { description: errorMessage(e) });
        }
      }
    },
    [halt, pause, startAt, state.id, stop],
  );

  const seek = useCallback(
    (ms: number) => {
      if (!buffer.current) return;
      if (source.current) startAt(ms / 1000);
      else {
        offset.current = ms / 1000;
        setState((s) => ({ ...s, position: ms }));
      }
    },
    [startAt],
  );

  const value = useMemo(() => ({ ...state, toggle, seek, stop }), [state, toggle, seek, stop]);
  return <PlayerContext.Provider value={value}>{children}</PlayerContext.Provider>;
}

export function PlayButton({ id, durationMs, className }: { id: string; durationMs: number | null; className?: string }) {
  const player = usePlayer();
  const active = player.id === id;
  const label = active && player.playing ? 'Pause recording' : 'Play recording';
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={() => player.toggle(id, durationMs)}
      className={cn('flex size-7 items-center justify-center rounded-lg text-ink-muted transition-colors hover:bg-surface-strong hover:text-ink', className)}
    >
      {active && player.loading ? <Spinner className="size-[15px]" /> : active && player.playing ? <Pause className="size-[15px]" strokeWidth={1.8} /> : <Play className="size-[15px]" strokeWidth={1.8} />}
    </button>
  );
}

/** Progress, scrubber, and time for the recording that is loaded. */
export function PlayerBar({ id, durationMs }: { id: string; durationMs: number | null }) {
  const player = usePlayer();
  if (player.id !== id) return null;
  const duration = player.duration || durationMs || 0;
  return (
    <div className="flex items-center gap-3">
      <PlayButton id={id} durationMs={durationMs} className="-ml-1.5" />
      <span className="w-9 shrink-0 text-right text-xs tabular-nums text-ink-muted">{clock(player.position)}</span>
      <Slider.Root
        className="relative flex h-5 flex-1 touch-none select-none items-center"
        value={[Math.min(player.position, duration)]}
        max={Math.max(duration, 1)}
        step={50}
        onValueChange={([v]) => player.seek(v)}
      >
        <Slider.Track className="relative h-[3px] grow overflow-hidden rounded-full bg-track/60">
          <Slider.Range className="absolute h-full rounded-full bg-ink" />
        </Slider.Track>
        <Slider.Thumb aria-label="Seek" className="block size-3 rounded-full bg-ink shadow outline-none transition-transform focus-visible:ring-2 focus-visible:ring-ink/30 hover:scale-110" />
      </Slider.Root>
      <span className="w-9 shrink-0 text-xs tabular-nums text-ink-muted">{clock(duration)}</span>
    </div>
  );
}

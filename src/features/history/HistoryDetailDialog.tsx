import { useEffect, useState } from 'react';
import { Copy } from 'lucide-react';
import { api, onHistoryChange, type HistoryDetail } from '@/lib/api';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Badge, Spinner } from '@/components/ui/misc';
import { useHistoryActions } from './actions-context';
import { bytesLabel, clock, deliveryLabel, displayText, emptyLine, failureText, fullDate, hasAudio, rowTag, timeLabel } from './format';
import { PlayButton, PlayerBar } from './player';
import { usePlayer } from './player-context';
import { ItemMenu } from './HistoryRow';

const PROVIDERS: Record<string, string> = { deepgram: 'Deepgram', elevenlabs: 'ElevenLabs', openrouter: 'OpenRouter', megallm: 'MegaLLM' };
const providerName = (id: string) => PROVIDERS[id] ?? id;

function Meta({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <div className="text-xs text-ink-muted">{label}</div>
      <div className="mt-0.5 truncate text-[14px] text-ink">{children}</div>
    </div>
  );
}

function TextBlock({ title, text }: { title: string; text: string }) {
  return (
    <section className="mt-5">
      <h3 className="mb-1.5 text-xs font-medium uppercase tracking-[0.08em] text-ink-muted">{title}</h3>
      <p className="whitespace-pre-wrap break-words rounded-xl bg-surface px-4 py-3 text-[15px] leading-[22px] text-ink">{text}</p>
    </section>
  );
}

function refineLabel(status: string | null) {
  if (status === 'ok') return 'Refined';
  if (status === 'failed') return 'Failed, used formatted text';
  return 'Off';
}

export function HistoryDetailDialog({ id, onClose }: { id: string | null; onClose: () => void }) {
  const [detail, setDetail] = useState<HistoryDetail | null>(null);
  const [missing, setMissing] = useState(false);
  const actions = useHistoryActions();
  const player = usePlayer();

  useEffect(() => {
    if (!id) return;
    let alive = true;
    const load = () =>
      api.history
        .detail(id)
        .then((d) => alive && (setDetail(d), setMissing(false)))
        .catch(() => alive && setMissing(true));
    setDetail(null);
    load();
    const un = onHistoryChange(load);
    return () => {
      alive = false;
      un.then((f) => f());
    };
  }, [id]);

  useEffect(() => {
    if (missing && id) onClose();
  }, [missing, id, onClose]);

  const d = detail;
  const text = d ? displayText(d) : '';
  const tag = d ? rowTag(d) : null;
  const raw = d?.raw_text?.trim() ?? '';
  const busy = d ? actions.busy.has(d.id) : false;
  // The text comes from the newest successful attempt (a retry may use another provider).
  const source = d ? ([...d.attempts].reverse().find((t) => t.status === 'ok') ?? { provider: d.provider, model: d.model }) : null;

  return (
    <Dialog open={id !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[min(720px,calc(100vh-80px))] w-[min(680px,calc(100vw-80px))] flex-col p-0">
        {!d ? (
          <div className="flex h-40 items-center justify-center">
            <DialogTitle className="sr-only">Transcript</DialogTitle>
            <Spinner />
          </div>
        ) : (
          <>
            <div className="border-b border-border px-7 pb-5 pt-6">
              <DialogTitle className="flex items-center gap-2.5 pr-10 text-[17px]">
                {fullDate(d.created_at)}
                {tag && <Badge tone={tag.tone}>{tag.label}</Badge>}
                {busy && <Spinner className="size-[15px]" />}
              </DialogTitle>
              <DialogDescription className="sr-only">Transcript details</DialogDescription>
              <div className="mt-4 grid grid-cols-4 gap-4">
                <Meta label="App">{d.target_app ?? 'Unknown'}</Meta>
                <Meta label="Length">{d.duration_ms ? clock(d.duration_ms) : '–'}</Meta>
                <Meta label="Words">{d.word_count ?? 0}</Meta>
                <Meta label="Speech">
                  {source && source.provider !== 'unknown' ? `${providerName(source.provider)} ${source.model}` : 'Unknown'}
                </Meta>
              </div>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto px-7 pb-6">
              {text ? <TextBlock title="Transcript" text={text} /> : <p className="mt-5 italic text-ink-muted">{emptyLine(d)}</p>}
              {raw && raw !== text && <TextBlock title="Original transcript" text={raw} />}

              <section className="mt-5">
                <h3 className="mb-1.5 text-xs font-medium uppercase tracking-[0.08em] text-ink-muted">Recording</h3>
                {hasAudio(d) ? (
                  <div className="rounded-xl bg-surface px-4 py-2.5">
                    {player.id === d.id ? (
                      <PlayerBar id={d.id} durationMs={d.audio_ms ?? d.duration_ms} />
                    ) : (
                      <div className="flex items-center gap-3">
                        <PlayButton id={d.id} durationMs={d.audio_ms ?? d.duration_ms} className="-ml-1.5" />
                        <span className="text-[14px] text-ink">{clock(d.audio_ms ?? d.duration_ms ?? 0)}</span>
                        <span className="text-xs text-ink-muted">{d.audio_bytes ? bytesLabel(d.audio_bytes) : ''} Opus</span>
                      </div>
                    )}
                  </div>
                ) : (
                  <p className="text-[14px] text-ink-muted">{d.audio_state === 'deleted' ? 'Recording deleted.' : 'No recording is stored for this transcript.'}</p>
                )}
              </section>

              <section className="mt-5 grid grid-cols-2 gap-4">
                <Meta label="Delivery">{deliveryLabel(d) ?? '–'}</Meta>
                <Meta label="AI refinement">{refineLabel(d.refine_status)}</Meta>
              </section>

              {d.attempts.length > 0 && (
                <section className="mt-5">
                  <h3 className="mb-1.5 text-xs font-medium uppercase tracking-[0.08em] text-ink-muted">Attempts</h3>
                  <ol className="divide-y divide-border rounded-xl border border-border">
                    {d.attempts.map((t, i) => {
                      const parent = d.attempts.findIndex((p) => p.id === t.parent_id);
                      return (
                        <li key={t.id} className="flex items-center gap-3 px-4 py-2.5 text-[14px]">
                          <span className="w-6 text-ink-muted">#{i + 1}</span>
                          <span className="min-w-0 flex-1 truncate">
                            {t.kind === 'retry' ? `Retry${parent >= 0 ? ` of #${parent + 1}` : ''}` : 'Live'} · {providerName(t.provider)} {t.model}
                          </span>
                          <span className="text-xs text-ink-muted">{timeLabel(t.started_at)}</span>
                          <Badge tone={t.status === 'ok' ? 'success' : t.status === 'running' ? 'neutral' : 'warning'}>
                            {t.status === 'ok' ? 'OK' : t.status === 'running' ? 'Running' : t.error ? failureText(t.error) : t.status}
                          </Badge>
                        </li>
                      );
                    })}
                  </ol>
                </section>
              )}
            </div>
            <div className="flex items-center justify-end gap-2 border-t border-border px-7 py-4">
              {text && (
                <Button variant="secondary" onClick={() => actions.copy(d)}>
                  <Copy /> Copy
                </Button>
              )}
              <ItemMenu item={d} align="end" />
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

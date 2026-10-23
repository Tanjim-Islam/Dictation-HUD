import { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Spinner } from '@/components/ui/misc';
import { api, errorMessage, onLlmPull, type LocalLlmState, type LocalLlmStatus } from '@/lib/api';
import { bytesLabel } from '@/features/history/format';
import { Row } from './parts';
import { DownloadProgress } from './DownloadProgress';
import { sortLocalModels, stateRow } from './local-llm';

type Props = { provider: string; name: string; value: string; onChange: (v: string) => void };

/** Model row for Ollama and LM Studio: lists the user's text models, starts the server when it is
 * off, and offers two small models when none is installed. */
export function LocalModelPicker({ provider, name, value, onChange }: Props) {
  const [status, setStatus] = useState<LocalLlmStatus | null>(null);
  const [state, setState] = useState<LocalLlmState>('running');
  const [pull, setPull] = useState<{ model: string; pct: number } | null>(null);
  const changeRef = useRef(onChange);
  changeRef.current = onChange;

  const start = useCallback(async () => {
    setState('starting');
    const s = await api.localLlm.start(provider).catch(() => null);
    if (s) {
      setStatus(s);
      setState(s.state);
    } else setState('failed');
  }, [provider]);

  useEffect(() => {
    let live = true;
    setStatus(null);
    api.localLlm
      .status(provider)
      .then((s) => {
        if (!live) return;
        setStatus(s);
        setState(s.state);
        if (s.state === 'stopped') void start();
      })
      .catch(() => live && setState('failed'));
    return () => {
      live = false;
    };
  }, [provider, start]);

  useEffect(() => {
    const off = onLlmPull((e) => {
      if (e.provider !== provider) return;
      if (e.state === 'downloading') {
        setPull({ model: e.model, pct: e.pct });
        return;
      }
      setPull(null);
      if (e.state === 'failed' && e.error !== 'Cancelled') toast.error(`Could not download ${e.model}`, { description: e.error ?? undefined });
      if (e.state === 'done') {
        changeRef.current(e.model);
        void api.localLlm.status(provider).then(setStatus);
      }
    });
    return () => void off.then((f) => f());
  }, [provider]);

  if (!status) return <Row title="Model" control={<Spinner />} />;
  const models = sortLocalModels(status.models);
  const row = stateRow(state, name);
  const download = (id: string) => api.localLlm.pull(provider, id).catch((e) => toast.error('Download failed', { description: errorMessage(e) }));

  return (
    <>
      <Row
        title="Model"
        description="If the model fails or times out, the formatted transcript is pasted instead."
        control={
          <Select value={models.some((m) => m.id === value) ? value : undefined} onValueChange={onChange} disabled={models.length === 0}>
            <SelectTrigger className="w-[300px]" aria-label="AI model">
              <SelectValue placeholder={models.length ? 'Choose a model' : 'No models'} />
            </SelectTrigger>
            <SelectContent>
              {models.map((m) => (
                <SelectItem key={m.id} value={m.id} disabled={!m.usable} hint={bytesLabel(m.size_bytes)}>
                  {m.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        }
      />
      {row && (
        <Row
          title={<span className="text-ink-muted">{row.text}</span>}
          control={
            row.retry ? (
              <Button size="sm" variant="secondary" onClick={() => void start()}>
                Retry
              </Button>
            ) : state === 'starting' ? (
              <Spinner />
            ) : null
          }
        />
      )}
      {models.length === 0 && state === 'running' && status.picks.length > 0 && (
        <>
            {status.picks.map((p) => (
              <Row
                key={p.id}
                title={
                  <span className="flex items-baseline gap-2">
                    {p.name}
                    <span className="text-xs text-ink-muted">{bytesLabel(p.size_bytes)}</span>
                  </span>
                }
                control={
                  pull?.model === p.id ? (
                    <DownloadProgress pct={pull.pct} onCancel={() => void api.localLlm.cancelPull(provider)} />
                  ) : (
                    <Button size="sm" variant="secondary" disabled={!!pull} onClick={() => void download(p.id)}>
                      Download
                    </Button>
                  )
                }
              />
            ))}
        </>
      )}
    </>
  );
}

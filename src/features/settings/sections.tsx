import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Check, ChevronsUpDown } from 'lucide-react';
import { getVersion } from '@tauri-apps/api/app';
import { Switch } from '@/components/ui/switch';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectSeparator, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Popover, PopoverContent, PopoverTrigger, Command, CommandEmpty, CommandInput, CommandItem, CommandList } from '@/components/ui/popover';
import { Spinner } from '@/components/ui/misc';
import { api, errorMessage, onHistoryChange, type AudioDevice, type ProvidersOverview, type StorageView } from '@/lib/api';
import { bytesLabel } from '@/features/history/format';
import { languageLabel, sortLanguages } from '@/lib/languages';
import { useSettings } from './settings-context';
import { Group, GroupLabel, Hint, Row, SectionTitle } from './parts';
import { ShortcutField } from './ShortcutField';
import { KeySourceBadge, SecretField } from './SecretField';
import { ElevenLabsPool } from './ElevenLabsPool';
import { UpdatesRow } from './UpdatesRow';

const DEFAULT_DEVICE = '__default__';

function useProviders() {
  const [overview, setOverview] = useState<ProvidersOverview | null>(null);
  const reload = useCallback(() => {
    api.providersOverview().then(setOverview).catch((e) => toast.error('Could not load providers', { description: errorMessage(e) }));
  }, []);
  useEffect(reload, [reload]);
  return { overview, reload };
}

export function GeneralSection() {
  const { settings, update } = useSettings();
  if (!settings) return <Spinner />;
  return (
    <>
      <SectionTitle>General</SectionTitle>
      <Group>
        <Row title="Dictation shortcut" description="Press once to start, press again to finish." control={<ShortcutField />} />
        <Row
          title="Paste automatically"
          description="Insert the transcript into the app you were using. When off, it is only copied to the clipboard."
          control={<Switch aria-label="Paste automatically" checked={settings.auto_paste} onCheckedChange={(v) => update({ auto_paste: v })} />}
        />
        <Row
          title="Flow Bar position"
          description="Where the Flow Bar appears while you dictate, on the screen of the app you are using."
          control={
            <Select value={settings.flowbar_edge} onValueChange={(v) => update({ flowbar_edge: v as typeof settings.flowbar_edge })}>
              <SelectTrigger className="w-[150px]" aria-label="Flow Bar position">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="bottom">Bottom</SelectItem>
                <SelectItem value="top">Top</SelectItem>
                <SelectItem value="left">Left</SelectItem>
                <SelectItem value="right">Right</SelectItem>
              </SelectContent>
            </Select>
          }
        />
      </Group>
      <GroupLabel>App</GroupLabel>
      <Group>
        <Row
          title="Launch at login"
          description="Starts quietly in the system tray."
          control={
            <Switch
              aria-label="Launch at login"
              checked={settings.autostart}
              onCheckedChange={async (v) => {
                try {
                  await api.setAutostart(v);
                  await update({});
                } catch (e) {
                  toast.error('Could not change launch at login', { description: errorMessage(e) });
                }
              }}
            />
          }
        />
      </Group>
    </>
  );
}

export function AudioSection() {
  const { settings, update } = useSettings();
  const [devices, setDevices] = useState<AudioDevice[] | null>(null);

  useEffect(() => {
    api.audioDevices().then(setDevices).catch(() => setDevices([]));
  }, []);

  if (!settings) return <Spinner />;
  const selected = settings.input_device ?? DEFAULT_DEVICE;
  const missing = settings.input_device && devices && !devices.some((d) => d.id === settings.input_device);
  const defaultName = devices?.find((d) => d.is_default)?.name;

  return (
    <>
      <SectionTitle>Audio</SectionTitle>
      <Group>
        <Row title="Microphone" description="Used for the next dictation." control={null}>
          <Select value={selected} onValueChange={(v) => update({ input_device: v === DEFAULT_DEVICE ? '' : v })}>
            <SelectTrigger aria-label="Microphone">
              <SelectValue placeholder="Choose a microphone" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={DEFAULT_DEVICE} hint={defaultName}>
                System default
              </SelectItem>
              {devices && devices.length > 0 && <SelectSeparator />}
              {devices?.map((d) => (
                <SelectItem key={d.id} value={d.id}>
                  {d.name}
                </SelectItem>
              ))}
              {missing && (
                <SelectItem value={settings.input_device!} disabled>
                  Disconnected microphone
                </SelectItem>
              )}
            </SelectContent>
          </Select>
          {missing && (
            <div className="mt-2">
              <Hint tone="warning">The selected microphone is not connected. The system default is used until it returns.</Hint>
            </div>
          )}
        </Row>
      </Group>
      <GroupLabel>Processing</GroupLabel>
      <Group>
        <Row
          title="Echo cancellation"
          description="Removes sound from your speakers that the microphone picks up. Uses your default output device as the reference."
          control={<Switch aria-label="Echo cancellation" checked={settings.echo_cancellation} onCheckedChange={(v) => update({ echo_cancellation: v })} />}
        />
        <Row
          title="Noise suppression"
          description="Reduces steady background noise, like fans or hum, before audio reaches the speech provider."
          control={<Switch aria-label="Noise suppression" checked={settings.noise_suppression} onCheckedChange={(v) => update({ noise_suppression: v })} />}
        />
      </Group>
      <div className="mt-3">
        <Hint>Processing runs inside Dictation HUD. Some microphones also apply their own processing, which these switches do not control.</Hint>
      </div>
    </>
  );
}

export function SpeechSection() {
  const { settings, update } = useSettings();
  const { overview, reload } = useProviders();
  if (!settings || !overview) return <Spinner />;

  const provider = overview.stt.find((p) => p.id === settings.stt_provider) ?? overview.stt[0];
  const modelId = provider.id === 'elevenlabs' ? settings.elevenlabs_model : settings.deepgram_model;
  const model = provider.models.find((m) => m.id === modelId) ?? provider.models.find((m) => m.id === provider.default_model)!;
  const languages = sortLanguages(model.languages);
  const langSupported = model.languages.includes(settings.language) || model.languages.includes(settings.language.split('-')[0]);
  const modelKey = provider.id === 'elevenlabs' ? 'elevenlabs_model' : 'deepgram_model';
  const otherModelWithLang = provider.models.find((m) => m.id !== model.id && m.languages.includes(settings.language));

  return (
    <>
      <SectionTitle>Speech to text</SectionTitle>
      <Group>
        <Row title="Provider" description="Where your audio is transcribed." control={
          <Select value={provider.id} onValueChange={(v) => update({ stt_provider: v })}>
            <SelectTrigger className="w-[200px]" aria-label="Speech provider">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {overview.stt.map((p) => (
                <SelectItem key={p.id} value={p.id} hint={p.configured ? undefined : 'No key'}>
                  {p.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        } />
        <Row title="Model" control={
          <Select value={model.id} onValueChange={(v) => update({ [modelKey]: v })}>
            <SelectTrigger className="w-[200px]" aria-label="Speech model">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {provider.models.map((m) => (
                <SelectItem key={m.id} value={m.id}>
                  {m.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        } />
        <Row title="Language" description={`${model.languages.length - (model.languages.includes('multi') ? 1 : 0)} languages on ${model.name}.`} control={
          <Select value={langSupported ? settings.language : undefined} onValueChange={(v) => update({ language: v })}>
            <SelectTrigger className="w-[240px]" aria-label="Dictation language">
              <SelectValue placeholder="Not supported by this model" />
            </SelectTrigger>
            <SelectContent>
              {languages.map((code) => (
                <SelectItem key={code} value={code}>
                  {languageLabel(code)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        }>
          {!langSupported && (
            <div className="flex items-center gap-3">
              <Hint tone="warning">{languageLabel(settings.language)} is not available on {model.name}.</Hint>
              {otherModelWithLang && (
                <Button size="sm" variant="secondary" onClick={() => update({ [modelKey]: otherModelWithLang.id })}>
                  Switch to {otherModelWithLang.name}
                </Button>
              )}
            </div>
          )}
        </Row>
      </Group>

      <GroupLabel>{provider.name} API key</GroupLabel>
      <Group>
        {provider.id === 'elevenlabs' ? (
          <Row title={<span className="flex items-center gap-2">ElevenLabs <KeySourceBadge source={provider.key_source} /></span>}>
            <ElevenLabsPool />
          </Row>
        ) : (
          <Row title={<span className="flex items-center gap-2">{provider.name} <KeySourceBadge source={provider.key_source} /></span>} description="Stored in Windows Credential Manager. It never leaves the backend.">
            <SecretField provider={provider.id} name={provider.name} source={provider.key_source} onChanged={reload} />
          </Row>
        )}
      </Group>
    </>
  );
}

function ModelPicker({ provider, value, onChange }: { provider: string; value: string; onChange: (v: string) => void }) {
  const [open, setOpen] = useState(false);
  const [models, setModels] = useState<string[] | null>(null);
  const [query, setQuery] = useState('');
  useEffect(() => {
    if (!open || models) return;
    api.listRefineModels(provider).then(setModels).catch(() => setModels([]));
  }, [open, models, provider]);
  useEffect(() => setModels(null), [provider]);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label="AI model"
          className="flex h-9 w-[300px] items-center justify-between gap-2 rounded-lg border border-border bg-white px-3 text-left text-[14px] hover:border-[#e2ded3]"
        >
          <span className="truncate">{value}</span>
          <ChevronsUpDown className="size-4 shrink-0 text-ink-muted" strokeWidth={1.75} />
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[360px] p-0">
        <Command>
          <CommandInput placeholder="Search or type a model id" value={query} onValueChange={setQuery} />
          <CommandList>
            {models === null ? (
              <div className="flex justify-center py-6">
                <Spinner />
              </div>
            ) : (
              <>
                <CommandEmpty>No matching model.</CommandEmpty>
                {query.trim() && !models.includes(query.trim()) && (
                  <CommandItem value={`custom:${query}`} onSelect={() => { onChange(query.trim()); setOpen(false); }}>
                    Use “{query.trim()}”
                  </CommandItem>
                )}
                {models.slice(0, 400).map((m) => (
                  <CommandItem key={m} value={m} onSelect={() => { onChange(m); setOpen(false); }}>
                    <span className="truncate">{m}</span>
                    {m === value && <Check className="ml-auto size-4" strokeWidth={2} />}
                  </CommandItem>
                ))}
              </>
            )}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

export function RefinementSection() {
  const { settings, update } = useSettings();
  const { overview, reload } = useProviders();
  if (!settings || !overview) return <Spinner />;
  const provider = overview.refine.find((p) => p.id === settings.ai_provider) ?? overview.refine[0];
  const modelKey = provider.id === 'megallm' ? 'megallm_model' : 'openrouter_model';
  const model = provider.id === 'megallm' ? settings.megallm_model : settings.openrouter_model;
  return (
    <>
      <SectionTitle>AI refinement</SectionTitle>
      <Group>
        <Row
          title="AI refinement"
          description="Clean up punctuation, grammar, and filler words with an AI model. When off, no transcript is sent to an AI provider; built-in formatting still runs."
          control={<Switch aria-label="AI refinement" checked={settings.ai_refine} onCheckedChange={(v) => update({ ai_refine: v })} />}
        />
      </Group>
      <div className={settings.ai_refine ? '' : 'pointer-events-none opacity-50'} aria-disabled={!settings.ai_refine}>
        <GroupLabel>Model</GroupLabel>
        <Group>
          <Row title="Provider" control={
            <Select value={provider.id} onValueChange={(v) => update({ ai_provider: v })}>
              <SelectTrigger className="w-[200px]" aria-label="AI provider">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {overview.refine.map((p) => (
                  <SelectItem key={p.id} value={p.id} hint={p.configured ? undefined : 'No key'}>
                    {p.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          } />
          <Row title="Model" description="If the model fails or times out, the formatted transcript is pasted instead." control={
            <ModelPicker provider={provider.id} value={model} onChange={(v) => update({ [modelKey]: v })} />
          } />
          <Row title={<span className="flex items-center gap-2">{provider.name} API key <KeySourceBadge source={provider.key_source} /></span>}>
            <SecretField provider={provider.id} name={provider.name} source={provider.key_source} onChanged={reload} />
          </Row>
        </Group>
      </div>
    </>
  );
}

const RETENTION = [
  { days: 1, label: '1 day' },
  { days: 7, label: '7 days' },
  { days: 14, label: '14 days' },
  { days: 30, label: '30 days' },
  { days: 90, label: '90 days' },
  { days: 365, label: '1 year' },
  { days: 0, label: 'Forever' },
];

export function HistorySection() {
  const { settings, update } = useSettings();
  const [storage, setStorage] = useState<StorageView | null>(null);
  useEffect(() => {
    const load = () => api.history.storage().then(setStorage).catch(() => {});
    load();
    const un = onHistoryChange(load);
    return () => {
      un.then((f) => f());
    };
  }, []);
  if (!settings) return <Spinner />;
  const days = settings.audio_retention_days;
  const options = RETENTION.some((r) => r.days === days) ? RETENTION : [...RETENTION, { days, label: `${days} days` }];
  return (
    <>
      <SectionTitle>History</SectionTitle>
      <Group>
        <Row
          title="Keep recordings"
          description="Recordings let you replay a dictation or retry its transcript. Older recordings are deleted automatically; transcripts are always kept."
          control={
            <Select value={String(days)} onValueChange={(v) => update({ audio_retention_days: Number(v) })}>
              <SelectTrigger className="w-[150px]" aria-label="Keep recordings">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {options.map((r) => (
                  <SelectItem key={r.days} value={String(r.days)}>
                    {r.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          }
        />
      </Group>
      <GroupLabel>Storage</GroupLabel>
      <Group>
        <Row title="Transcripts" control={<span className="text-[14px] tabular-nums text-ink-muted">{storage ? storage.sessions.toLocaleString() : '–'}</span>} />
        <Row
          title="Recordings"
          description="Stored on this computer only, compressed with Opus."
          control={
            <span className="text-[14px] tabular-nums text-ink-muted">
              {storage ? `${storage.audio_files.toLocaleString()} files · ${bytesLabel(storage.audio_bytes)}` : '–'}
            </span>
          }
        />
      </Group>
    </>
  );
}

export function AboutSection() {
  const [version, setVersion] = useState('');
  useEffect(() => {
    getVersion().then(setVersion).catch(() => setVersion('dev'));
  }, []);
  return (
    <>
      <SectionTitle>About</SectionTitle>
      <Group>
        <Row title="Dictation HUD" description={`Version ${version}`} />
        <UpdatesRow />
      </Group>
    </>
  );
}

import { useState } from 'react';
import { toast } from 'sonner';
import { Check, ChevronDown, FolderOpen, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/misc';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { api, errorMessage, type LocalSttModel } from '@/lib/api';
import { bytesLabel } from '@/features/history/format';
import { Group, GroupLabel, Row } from './parts';
import { DownloadProgress } from './DownloadProgress';
import { isReady, modelHint, sortSttModels, splitSttModels } from './local-stt';

const startDownload = (id: string) => api.localStt.download(id).catch((e) => toast.error('Download failed', { description: errorMessage(e) }));

/** Model row for the Local provider. Picking a model that is not installed starts its download. */
export function LocalModelSelect({ models, value, onChange }: { models: LocalSttModel[]; value: string; onChange: (id: string) => void }) {
  return (
    <Select
      value={value}
      onValueChange={(id) => {
        if (models.find((m) => m.id === id)?.state === 'not_installed') void startDownload(id);
        onChange(id);
      }}
    >
      <SelectTrigger className="w-[200px]" aria-label="Speech model">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {sortSttModels(models).map((m) => (
          <SelectItem key={m.id} value={m.id} hint={modelHint(m)}>
            {m.name}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

export function LocalModelsGroup({ models, reload }: { models: LocalSttModel[]; reload: () => void }) {
  const [showMore, setShowMore] = useState(false);
  const { shown, more } = splitSttModels(models);
  const link = async () => {
    try {
      const r = await api.localStt.link();
      if (r) {
        toast.success(`Using ${r.name} from this PC`);
        reload();
      }
    } catch (e) {
      toast.error("Couldn't use that folder", { description: errorMessage(e) });
    }
  };
  const remove = (m: LocalSttModel) =>
    api.localStt
      .remove(m.id)
      .then(reload)
      .catch((e) => toast.error(`Couldn't remove ${m.name}`, { description: errorMessage(e) }));

  return (
    <>
      <GroupLabel>Models</GroupLabel>
      <Group>
        {[...shown, ...(showMore ? more : [])].map((m) => (
          <Row
            key={m.id}
            title={
              <span className="flex items-baseline gap-2">
                {m.name}
                <span className="text-xs text-ink-muted">{bytesLabel(m.size_bytes)}</span>
              </span>
            }
            control={
              m.state === 'downloading' ? (
                <DownloadProgress pct={m.pct ?? 0} onCancel={() => void api.localStt.cancel(m.id)} />
              ) : isReady(m.state) ? (
                <span className="flex items-center gap-1">
                  <Badge tone="success">
                    <Check className="size-3" strokeWidth={2.5} />
                    Installed
                  </Badge>
                  <Button size="icon-sm" variant="subtle" aria-label={`Remove ${m.name}`} onClick={() => void remove(m)}>
                    <Trash2 />
                  </Button>
                </span>
              ) : m.state === 'missing' ? (
                <Button size="sm" variant="secondary" onClick={() => void link()}>
                  Locate again
                </Button>
              ) : (
                <Button size="sm" variant="secondary" onClick={() => void startDownload(m.id)}>
                  Download
                </Button>
              )
            }
          />
        ))}
        {more.length > 0 && (
          <Row
            title={
              <button type="button" className="flex items-center gap-2 text-[14px] text-ink-muted hover:text-ink" aria-expanded={showMore} onClick={() => setShowMore((v) => !v)}>
                <ChevronDown className={`size-4 transition-transform ${showMore ? 'rotate-180' : ''}`} strokeWidth={1.75} />
                {showMore ? 'Fewer models' : `More models (${more.length})`}
              </button>
            }
          />
        )}
        <Row
          title={
            <button type="button" className="flex items-center gap-2 text-[14px] text-accent hover:underline" onClick={() => void link()}>
              <FolderOpen className="size-4" strokeWidth={1.75} />
              Use a model on this PC
            </button>
          }
        />
      </Group>
    </>
  );
}

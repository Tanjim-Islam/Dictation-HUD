import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Check, ChevronDown, Download, FolderOpen, RotateCcw, Trash2, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Badge } from '@/components/ui/misc';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { api, errorMessage, type LocalSttModel, type ModelsFolder } from '@/lib/api';
import { bytesLabel } from '@/features/history/format';
import { Group, GroupLabel, Row } from './parts';
import { DownloadProgress } from './DownloadProgress';
import { isReady, modelHint, shortPath, sortSttModels, splitSttModels } from './local-stt';
import { downloadAll, stopAll, useDownloadAllRunning } from './download-all';

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

/** Where downloads go. New downloads use the picked folder; downloaded models stay usable. */
export function ModelsFolderRow({ onChanged }: { onChanged: () => void }) {
  const [folder, setFolder] = useState<ModelsFolder | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api.localStt.folder().then(setFolder).catch(() => {});
  }, []);
  const run = async (change: () => Promise<ModelsFolder | null>, done: string) => {
    setBusy(true);
    try {
      const f = await change();
      if (!f) return;
      setFolder(f);
      onChanged();
      toast.success(done, { description: 'New downloads go here. Models you already downloaded keep working.' });
    } catch (e) {
      toast.error("Couldn't change the models folder", { description: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Row
      title="Models folder"
      description={
        <span className="block truncate" title={folder?.path}>
          {!folder ? '\u00A0' : folder.custom ? shortPath(folder.path) : 'Default'}
        </span>
      }
      control={
        <>
          {folder?.custom && (
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => void run(api.localStt.resetFolder, 'Models folder reset to default')}>
              <RotateCcw strokeWidth={1.75} />
              Reset to default
            </Button>
          )}
          <Button size="sm" variant="secondary" disabled={busy || !folder} onClick={() => void run(api.localStt.chooseFolder, 'Models folder changed')}>
            <FolderOpen strokeWidth={1.75} />
            Change
          </Button>
        </>
      }
    />
  );
}

export function LocalModelsGroup({ models, reload }: { models: LocalSttModel[]; reload: () => void }) {
  const [showMore, setShowMore] = useState(false);
  const [confirmAll, setConfirmAll] = useState(false);
  const downloadingAll = useDownloadAllRunning();
  const { shown, more } = splitSttModels(models);
  const missing = sortSttModels(models).filter((m) => m.state === 'not_installed');
  const missingBytes = missing.reduce((sum, m) => sum + m.size_bytes, 0);
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
      <div className="mb-2.5 mt-7 flex items-center justify-between gap-3">
        <GroupLabel className="m-0">Models</GroupLabel>
        {downloadingAll ? (
          <Button size="sm" variant="ghost" onClick={stopAll}>
            <X strokeWidth={1.75} />
            Stop downloads
          </Button>
        ) : (
          missing.length > 0 && (
            <Button size="sm" variant="ghost" onClick={() => setConfirmAll(true)}>
              <Download strokeWidth={1.75} />
              Download all
            </Button>
          )
        )}
      </div>
      <AlertDialog open={confirmAll} onOpenChange={setConfirmAll}>
        <AlertDialogContent>
          <AlertDialogTitle>
            Download {missing.length} {missing.length === 1 ? 'model' : 'models'}?
          </AlertDialogTitle>
          <AlertDialogDescription>
            This downloads {bytesLabel(missingBytes)}, one model at a time. You can keep using the app, and stop at any time.
          </AlertDialogDescription>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={() => void downloadAll(missing.map((m) => m.id))}>Download all</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
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

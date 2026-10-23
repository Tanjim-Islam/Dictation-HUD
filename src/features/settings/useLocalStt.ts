import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { api, onSttDownload, type LocalSttModel } from '@/lib/api';

/** Local speech models with live download progress. */
export function useLocalStt() {
  const [models, setModels] = useState<LocalSttModel[] | null>(null);
  const reload = useCallback(() => {
    void api.localStt
      .models()
      .then(setModels)
      .catch(() => setModels([]));
  }, []);
  useEffect(reload, [reload]);
  useEffect(() => {
    const off = onSttDownload((e) => {
      if (e.state === 'downloading') {
        setModels((ms) => ms?.map((m) => (m.id === e.id ? { ...m, state: 'downloading', pct: e.pct } : m)) ?? ms);
        return;
      }
      if (e.state === 'failed') toast.error('Download failed', { description: e.error ?? undefined });
      reload();
    });
    return () => void off.then((f) => f());
  }, [reload]);
  return { models, reload };
}

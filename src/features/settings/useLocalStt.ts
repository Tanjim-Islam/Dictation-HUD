import { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { api, onSttDownload, type LocalSttModel } from '@/lib/api';

/** Focus and visibility often fire together; one check covers both. */
const RECHECK_GAP_MS = 1000;

/** Local speech models with live download progress. */
export function useLocalStt() {
  const [models, setModels] = useState<LocalSttModel[] | null>(null);
  const lastCheck = useRef(0);
  const reload = useCallback(() => {
    lastCheck.current = Date.now();
    void api.localStt
      .models()
      // An unchanged list keeps the old array, so nothing re-renders.
      .then((next) => setModels((ms) => (JSON.stringify(ms) === JSON.stringify(next) ? ms : next)))
      .catch(() => setModels([]));
  }, []);
  useEffect(reload, [reload]);
  useEffect(() => {
    // Model folders can be deleted or moved in Explorer. Coming back to the app re-checks the
    // files (sizes only, no reads), so a deleted model shows Download again without polling.
    const recheck = () => {
      if (Date.now() - lastCheck.current >= RECHECK_GAP_MS) reload();
    };
    const onVisible = () => document.visibilityState === 'visible' && recheck();
    window.addEventListener('focus', recheck);
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      window.removeEventListener('focus', recheck);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, [reload]);
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

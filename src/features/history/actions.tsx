import { useCallback, useMemo, useState } from 'react';
import { toast } from 'sonner';
import { api, errorMessage, type HistoryItem } from '@/lib/api';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { usePlayer } from './player-context';
import { ActionsContext, type Actions } from './actions-context';

type Confirm = { kind: 'audio' | 'session'; item: HistoryItem } | null;


export function HistoryActionsProvider({ children, onDeleted }: { children: React.ReactNode; onDeleted?: (id: string) => void }) {
  const [busy, setBusy] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState<Confirm>(null);
  const player = usePlayer();

  const job = useCallback(async (id: string, run: () => Promise<unknown>, done: string, failed: string) => {
    setBusy((b) => new Set(b).add(id));
    try {
      await run();
      toast.success(done);
    } catch (e) {
      toast.error(failed, { description: errorMessage(e) });
    } finally {
      setBusy((b) => {
        const n = new Set(b);
        n.delete(id);
        return n;
      });
    }
  }, []);

  const value = useMemo<Actions>(
    () => ({
      busy,
      copy: (item) => {
        api.history
          .copy(item.id)
          .then(() => toast.success('Copied to clipboard'))
          .catch((e) => toast.error('Could not copy', { description: errorMessage(e) }));
      },
      retry: (item) => job(item.id, () => api.history.retry(item.id), 'Transcript updated', 'Retry failed'),
      recover: (item) => job(item.id, () => api.history.recover(item.id), 'Transcript recovered', 'Could not recover'),
      retryRefinement: (item) => job(item.id, () => api.history.retryRefinement(item.id), 'Refinement updated', 'Refinement failed'),
      exportWav: (item) => {
        api.history
          .exportWav(item.id)
          .then((path) =>
            toast.success('Saved WAV to Downloads', {
              action: { label: 'Show in folder', onClick: () => void api.history.showExport(path).catch(() => {}) },
            }),
          )
          .catch((e) => toast.error('Export failed', { description: errorMessage(e) }));
      },
      askDeleteAudio: (item) => setConfirm({ kind: 'audio', item }),
      askDelete: (item) => setConfirm({ kind: 'session', item }),
    }),
    [busy, job],
  );

  const runDelete = async () => {
    if (!confirm) return;
    const { kind, item } = confirm;
    setConfirm(null);
    if (player.id === item.id) player.stop();
    try {
      await api.history.remove(item.id, kind === 'audio');
      toast.success(kind === 'audio' ? 'Recording deleted' : 'Transcript deleted');
      if (kind === 'session') onDeleted?.(item.id);
    } catch (e) {
      toast.error('Could not delete', { description: errorMessage(e) });
    }
  };

  return (
    <ActionsContext.Provider value={value}>
      {children}
      <AlertDialog open={confirm !== null} onOpenChange={(o) => !o && setConfirm(null)}>
        <AlertDialogContent>
          <AlertDialogTitle>{confirm?.kind === 'audio' ? 'Delete this recording?' : 'Delete this transcript?'}</AlertDialogTitle>
          <AlertDialogDescription>
            {confirm?.kind === 'audio'
              ? 'The transcript stays in History, but you will not be able to play or retry it.'
              : 'The transcript and its recording are removed from this computer. This cannot be undone.'}
          </AlertDialogDescription>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction destructive onClick={runDelete}>
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </ActionsContext.Provider>
  );
}

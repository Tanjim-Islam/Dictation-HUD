import { createContext, useContext } from 'react';
import type { HistoryItem } from '@/lib/api';

export type Actions = {
  busy: ReadonlySet<string>;
  copy: (item: HistoryItem) => void;
  retry: (item: HistoryItem) => void;
  recover: (item: HistoryItem) => void;
  retryRefinement: (item: HistoryItem) => void;
  exportWav: (item: HistoryItem) => void;
  askDeleteAudio: (item: HistoryItem) => void;
  askDelete: (item: HistoryItem) => void;
};

export const ActionsContext = createContext<Actions | null>(null);

export function useHistoryActions() {
  const a = useContext(ActionsContext);
  if (!a) throw new Error('useHistoryActions outside HistoryActionsProvider');
  return a;
}

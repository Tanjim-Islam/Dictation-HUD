import { createContext, useContext } from 'react';

export type PlayerState = { id: string | null; playing: boolean; loading: boolean; position: number; duration: number };

export type Player = PlayerState & {
  toggle: (id: string, durationMs: number | null) => void;
  seek: (ms: number) => void;
  stop: () => void;
};

export const PlayerContext = createContext<Player | null>(null);

export function usePlayer() {
  const p = useContext(PlayerContext);
  if (!p) throw new Error('usePlayer outside PlayerProvider');
  return p;
}

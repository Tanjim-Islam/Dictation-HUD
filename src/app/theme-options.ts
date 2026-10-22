import { Monitor, Moon, Sun, type LucideIcon } from 'lucide-react';
import type { Theme } from '@/lib/api';

/** Theme choices shared by the title bar menu and Settings. */
export const THEME_OPTIONS: { id: Theme; label: string; icon: LucideIcon }[] = [
  { id: 'light', label: 'Light', icon: Sun },
  { id: 'dark', label: 'Dark', icon: Moon },
  { id: 'system', label: 'System', icon: Monitor },
];

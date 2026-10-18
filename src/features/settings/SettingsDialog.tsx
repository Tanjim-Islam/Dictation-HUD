import { useEffect, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { Dialog as DialogPrimitive } from 'radix-ui';
import type { LucideIcon } from 'lucide-react';
import { AudioLines, History, Info, Mic, ShieldCheck, SlidersHorizontal, Sparkles, X } from 'lucide-react';
import { getVersion } from '@tauri-apps/api/app';
import { transition } from '@/lib/motion';
import { cn } from '@/lib/utils';
import { AboutSection, AudioSection, GeneralSection, HistorySection, PrivacySection, RefinementSection, SpeechSection } from './sections';

export type SettingsSectionId = 'general' | 'audio' | 'speech' | 'refinement' | 'privacy' | 'history' | 'about';

const SECTIONS: { id: SettingsSectionId; label: string; icon: LucideIcon; render: () => React.ReactNode }[] = [
  { id: 'general', label: 'General', icon: SlidersHorizontal, render: () => <GeneralSection /> },
  { id: 'audio', label: 'Audio', icon: Mic, render: () => <AudioSection /> },
  { id: 'speech', label: 'Speech to text', icon: AudioLines, render: () => <SpeechSection /> },
  { id: 'refinement', label: 'AI refinement', icon: Sparkles, render: () => <RefinementSection /> },
  { id: 'privacy', label: 'Privacy', icon: ShieldCheck, render: () => <PrivacySection /> },
  { id: 'history', label: 'History', icon: History, render: () => <HistorySection /> },
  { id: 'about', label: 'About', icon: Info, render: () => <AboutSection /> },
];

export function SettingsDialog({
  open,
  onOpenChange,
  section,
  onSectionChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  section: SettingsSectionId;
  onSectionChange: (s: SettingsSectionId) => void;
}) {
  const [version, setVersion] = useState('');
  useEffect(() => {
    getVersion().then(setVersion).catch(() => setVersion('dev'));
  }, []);
  const active = SECTIONS.find((s) => s.id === section) ?? SECTIONS[0];

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-40 bg-black/25 data-[state=open]:animate-fade-in data-[state=closed]:animate-fade-out" />
        <DialogPrimitive.Content
          aria-describedby={undefined}
          className="fixed left-1/2 top-1/2 z-40 flex h-[min(640px,calc(100vh-96px))] w-[min(960px,calc(100vw-96px))] -translate-x-1/2 -translate-y-1/2 overflow-hidden rounded-2xl bg-white shadow-dialog outline-none data-[state=open]:animate-dialog-in data-[state=closed]:animate-dialog-out"
        >
          <DialogPrimitive.Title className="sr-only">Settings</DialogPrimitive.Title>
          <nav className="flex w-[210px] shrink-0 flex-col bg-surface px-2 pb-3 pt-6" aria-label="Settings sections">
            <div className="px-3 pb-2 text-xs font-medium uppercase tracking-[0.08em] text-ink-muted">Settings</div>
            <div className="flex flex-col gap-0.5">
              {SECTIONS.map((s) => {
                const Icon = s.icon;
                const selected = s.id === active.id;
                return (
                  <button
                    key={s.id}
                    type="button"
                    onClick={() => onSectionChange(s.id)}
                    aria-current={selected ? 'page' : undefined}
                    className={cn('relative flex h-9 items-center gap-3 rounded-lg px-3 text-left text-[15px] text-ink transition-colors', !selected && 'hover:bg-surface-strong/60')}
                  >
                    {selected && <motion.span layoutId="settings-active" className="absolute inset-0 rounded-lg bg-surface-strong" transition={transition.layout} />}
                    <Icon className="relative size-[18px]" strokeWidth={1.6} />
                    <span className="relative">{s.label}</span>
                  </button>
                );
              })}
            </div>
            <div className="flex-1" />
            <div className="px-3 text-xs text-ink-muted">Dictation HUD v{version}</div>
          </nav>
          <div className="relative min-w-0 flex-1">
            <DialogPrimitive.Close
              className="absolute right-4 top-4 z-10 flex size-8 items-center justify-center rounded-lg text-ink-muted transition-colors hover:bg-surface hover:text-ink"
              aria-label="Close settings"
            >
              <X className="size-[18px]" strokeWidth={1.75} />
            </DialogPrimitive.Close>
            <div className="h-full overflow-y-auto overscroll-contain px-12 pb-12 pt-14">
              <AnimatePresence mode="wait" initial={false}>
                <motion.div
                  key={active.id}
                  initial={{ opacity: 0, y: 6 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -4 }}
                  transition={transition.fast}
                  className="mx-auto max-w-[660px]"
                >
                  {active.render()}
                </motion.div>
              </AnimatePresence>
            </div>
          </div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

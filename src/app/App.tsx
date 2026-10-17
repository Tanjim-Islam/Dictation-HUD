import { useEffect, useState } from 'react';
import { AnimatePresence, motion, MotionConfig } from 'motion/react';
import { listen } from '@tauri-apps/api/event';
import { BookA, History, Scissors } from 'lucide-react';
import { TooltipProvider } from '@/components/ui/tooltip';
import { Toaster } from '@/components/ui/toaster';
import { SettingsProvider } from '@/features/settings/SettingsContext';
import { SettingsDialog, type SettingsSectionId } from '@/features/settings/SettingsDialog';
import { HistoryPage } from '@/features/history/HistoryPage';
import { DictionaryPage } from '@/features/dictionary/DictionaryPage';
import { SnippetsPage } from '@/features/snippets/SnippetsPage';
import { transition } from '@/lib/motion';
import { Sidebar, type NavItem } from './Sidebar';
import { TitleBar } from './TitleBar';

const NAV: NavItem[] = [
  { id: 'history', label: 'History', icon: History },
  { id: 'dictionary', label: 'Dictionary', icon: BookA },
  { id: 'snippets', label: 'Snippets', icon: Scissors },
];

const PAGES: Record<string, () => React.ReactNode> = {
  history: () => <HistoryPage />,
  dictionary: () => <DictionaryPage />,
  snippets: () => <SnippetsPage />,
};

export function App() {
  const [page, setPage] = useState('history');
  const [sidebar, setSidebar] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [section, setSection] = useState<SettingsSectionId>('general');

  // The tray can open a specific settings section.
  useEffect(() => {
    if (!('__TAURI_INTERNALS__' in window)) return;
    const un = listen<SettingsSectionId>('app://open-settings', (e) => {
      setSection(e.payload ?? 'general');
      setSettingsOpen(true);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  return (
    <MotionConfig reducedMotion="user">
      <TooltipProvider delayDuration={400}>
        <SettingsProvider>
          <div className="flex h-full flex-col bg-frame">
            <TitleBar onToggleSidebar={() => setSidebar((v) => !v)} />
            <div className="flex min-h-0 flex-1">
              <AnimatePresence initial={false}>
                {sidebar && (
                  <motion.div
                    key="sidebar"
                    initial={{ width: 0, opacity: 0 }}
                    animate={{ width: 218, opacity: 1 }}
                    exit={{ width: 0, opacity: 0 }}
                    transition={transition.base}
                    className="flex shrink-0 overflow-hidden"
                  >
                    <Sidebar items={NAV} active={page} onSelect={setPage} onOpenSettings={() => setSettingsOpen(true)} />
                  </motion.div>
                )}
              </AnimatePresence>
              <main className="mb-3 mr-3 min-w-0 flex-1 overflow-hidden rounded-[20px] border border-border/60 bg-panel" style={{ marginLeft: sidebar ? 0 : 12 }}>
                <div className="h-full overflow-y-auto px-10 pb-10 pt-12">
                  <AnimatePresence mode="wait" initial={false}>
                    <motion.div key={page} initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -4 }} transition={transition.fast}>
                      {PAGES[page]?.()}
                    </motion.div>
                  </AnimatePresence>
                </div>
              </main>
            </div>
          </div>
          <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} section={section} onSectionChange={setSection} />
          <Toaster />
        </SettingsProvider>
      </TooltipProvider>
    </MotionConfig>
  );
}

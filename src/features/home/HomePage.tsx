import { useEffect, useState } from 'react';
import { AudioLines } from 'lucide-react';
import { api } from '@/lib/api';
import { Keycaps } from '@/features/settings/ShortcutField';

function greeting() {
  const h = new Date().getHours();
  return h < 12 ? 'Good morning' : h < 18 ? 'Good afternoon' : 'Good evening';
}

export function HomePage() {
  const [hotkey, setHotkey] = useState('');
  useEffect(() => {
    api.getHotkey().then(setHotkey).catch(() => {});
  }, []);

  return (
    <div className="mx-auto max-w-[768px]">
      <h1 className="text-2xl font-semibold tracking-[-0.01em]">{greeting()}</h1>
      <div className="mt-8 flex items-center gap-5 rounded-2xl bg-surface p-6">
        <div className="flex size-12 shrink-0 items-center justify-center rounded-xl bg-ink text-panel">
          <AudioLines className="size-6" strokeWidth={2} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[17px] font-semibold">Dictate anywhere</div>
          <div className="mt-1 flex flex-wrap items-center gap-2 text-[14px] text-ink-muted">
            Press {hotkey ? <Keycaps combo={hotkey} /> : 'your shortcut'} in any app, speak, then press it again.
          </div>
        </div>
      </div>
    </div>
  );
}

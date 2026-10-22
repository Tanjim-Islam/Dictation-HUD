import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Copy, Minus, PanelLeft, Square, X } from 'lucide-react';
import { Tooltip } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { ThemeMenu } from './ThemeMenu';

const inTauri = '__TAURI_INTERNALS__' in window;

function CaptionButton({ label, onClick, children, danger }: { label: string; onClick: () => void; children: React.ReactNode; danger?: boolean }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        'flex h-full w-[46px] items-center justify-center text-ink/80 transition-colors duration-100 [&_svg]:stroke-[1.5]',
        danger ? 'hover:bg-[#c42b1c] hover:text-white' : 'hover:bg-surface-strong',
      )}
    >
      {children}
    </button>
  );
}

/**
 * Custom title bar. The empty area is a Tauri drag region (double click maximizes/restores
 * natively). Buttons are outside the drag region so clicks never start a window drag.
 * Close hides to the tray; only tray Exit quits.
 */
export function TitleBar({ onToggleSidebar }: { onToggleSidebar: () => void }) {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!inTauri) return;
    const win = getCurrentWindow();
    win.isMaximized().then(setMaximized).catch(() => {});
    const un = win.onResized(() => {
      win.isMaximized().then(setMaximized).catch(() => {});
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const win = inTauri ? getCurrentWindow() : null;

  return (
    <header className="flex h-[52px] shrink-0 select-none items-stretch" data-tauri-drag-region>
      <div className="flex items-center gap-1 pl-3">
        <Tooltip content="Toggle sidebar">
          <button
            type="button"
            aria-label="Toggle sidebar"
            onClick={onToggleSidebar}
            className="flex size-8 items-center justify-center rounded-lg text-ink/80 transition-colors hover:bg-surface-strong"
          >
            <PanelLeft className="size-[18px]" strokeWidth={1.6} />
          </button>
        </Tooltip>
      </div>
      <div className="flex-1" data-tauri-drag-region />
      <div className="flex items-center pr-2">
        <ThemeMenu />
      </div>
      <div className="flex items-stretch">
        <CaptionButton label="Minimize" onClick={() => win?.minimize()}>
          <Minus className="size-4" />
        </CaptionButton>
        <CaptionButton label={maximized ? 'Restore' : 'Maximize'} onClick={() => win?.toggleMaximize()}>
          {maximized ? <Copy className="size-[13px] -scale-x-100" /> : <Square className="size-[13px]" />}
        </CaptionButton>
        <CaptionButton label="Close" danger onClick={() => win?.close()}>
          <X className="size-[17px]" />
        </CaptionButton>
      </div>
    </header>
  );
}

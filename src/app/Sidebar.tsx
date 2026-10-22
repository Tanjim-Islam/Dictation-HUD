import { motion } from 'motion/react';
import type { LucideIcon } from 'lucide-react';
import { Settings as SettingsIcon } from 'lucide-react';
import logoDark from '@/assets/logo-dark.png';
import logoLight from '@/assets/logo-light.png';
import { transition } from '@/lib/motion';
import { cn } from '@/lib/utils';
import { UpdateNotice } from '@/features/settings/UpdateNotice';

export type NavItem = { id: string; label: string; icon: LucideIcon };

function NavButton({ item, active, onClick }: { item: NavItem; active: boolean; onClick: () => void }) {
  const Icon = item.icon;
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? 'page' : undefined}
      className={cn(
        'relative flex h-9 w-full items-center gap-3 rounded-lg px-3 text-left text-[15px] font-[550] transition-colors duration-150',
        active ? 'text-ink' : 'text-ink/85 hover:bg-surface-strong/70',
      )}
    >
      {active && <motion.span layoutId="nav-active" className="absolute inset-0 rounded-lg bg-surface-strong" transition={transition.layout} />}
      <Icon className="relative size-[18px] shrink-0" strokeWidth={2} />
      <span className="relative truncate">{item.label}</span>
    </button>
  );
}

export function Sidebar({
  items,
  active,
  onSelect,
  onOpenSettings,
}: {
  items: NavItem[];
  active: string;
  onSelect: (id: string) => void;
  onOpenSettings: () => void;
}) {
  return (
    <aside className="flex w-[218px] shrink-0 flex-col px-3 pb-3">
      <div className="flex h-11 items-center gap-2 px-2 pb-1">
        <img src={logoDark} alt="" draggable={false} className="h-[18px] w-auto dark:hidden" />
        <img src={logoLight} alt="" draggable={false} className="hidden h-[18px] w-auto dark:block" />
        <span className="text-[19px] font-semibold tracking-[-0.01em]">Dictation HUD</span>
      </div>
      <nav className="mt-4 flex flex-col gap-1" aria-label="Main">
        {items.map((item) => (
          <NavButton key={item.id} item={item} active={active === item.id} onClick={() => onSelect(item.id)} />
        ))}
      </nav>
      <div className="flex-1" />
      <UpdateNotice />
      <div className="flex flex-col gap-1 border-t border-border pt-3">
        <NavButton item={{ id: 'settings', label: 'Settings', icon: SettingsIcon }} active={false} onClick={onOpenSettings} />
      </div>
    </aside>
  );
}

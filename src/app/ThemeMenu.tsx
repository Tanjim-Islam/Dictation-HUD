import { Moon, Sun } from 'lucide-react';
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { useSettings } from '@/features/settings/settings-context';
import type { Theme } from '@/lib/api';
import { useResolvedTheme } from '@/lib/theme';
import { THEME_OPTIONS } from './theme-options';

/** Title bar theme button: shows the theme it would switch to (moon in light, sun in dark)
 * and opens Light, Dark, and System. */
export function ThemeMenu() {
  const { settings, update } = useSettings();
  const resolved = useResolvedTheme(settings?.theme);
  const Icon = resolved === 'dark' ? Sun : Moon;
  return (
    <DropdownMenu modal={false}>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label="Theme"
          disabled={!settings}
          className="flex size-9 items-center justify-center rounded-md text-ink/80 transition-colors hover:bg-surface-strong data-[state=open]:bg-surface-strong"
        >
          <Icon className="size-5" strokeWidth={2} />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-[160px]">
        <DropdownMenuRadioGroup value={settings?.theme ?? 'system'} onValueChange={(v) => void update({ theme: v as Theme })}>
          {THEME_OPTIONS.map(({ id, label, icon: ItemIcon }) => (
            <DropdownMenuRadioItem key={id} value={id}>
              <ItemIcon className="!size-3.5" />
              {label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

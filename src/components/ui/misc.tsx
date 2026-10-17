import * as React from 'react';
import { Separator as SeparatorPrimitive, ScrollArea as ScrollPrimitive, Label as LabelPrimitive } from 'radix-ui';
import { Loader2 } from 'lucide-react';
import { cn } from '@/lib/utils';

export function Separator({ className, orientation = 'horizontal', ...props }: React.ComponentPropsWithoutRef<typeof SeparatorPrimitive.Root>) {
  return (
    <SeparatorPrimitive.Root
      orientation={orientation}
      className={cn('shrink-0 bg-border', orientation === 'horizontal' ? 'h-px w-full' : 'h-full w-px', className)}
      {...props}
    />
  );
}

export function ScrollArea({
  className,
  children,
  viewportClassName,
  ...props
}: React.ComponentPropsWithoutRef<typeof ScrollPrimitive.Root> & { viewportClassName?: string }) {
  return (
    <ScrollPrimitive.Root className={cn('relative overflow-hidden', className)} {...props}>
      <ScrollPrimitive.Viewport className={cn('size-full rounded-[inherit]', viewportClassName)}>{children}</ScrollPrimitive.Viewport>
      <ScrollPrimitive.Scrollbar orientation="vertical" className="flex w-2.5 touch-none select-none p-0.5">
        <ScrollPrimitive.Thumb className="relative flex-1 rounded-full bg-track/60 hover:bg-track" />
      </ScrollPrimitive.Scrollbar>
    </ScrollPrimitive.Root>
  );
}

export const Label = React.forwardRef<React.ElementRef<typeof LabelPrimitive.Root>, React.ComponentPropsWithoutRef<typeof LabelPrimitive.Root>>(
  ({ className, ...props }, ref) => <LabelPrimitive.Root ref={ref} className={cn('text-[14px] font-medium text-ink', className)} {...props} />,
);
Label.displayName = 'Label';

export function Spinner({ className }: { className?: string }) {
  return <Loader2 className={cn('size-4 animate-spin text-ink-muted', className)} strokeWidth={2} aria-hidden />;
}

export type Tone = 'neutral' | 'success' | 'warning' | 'danger' | 'accent';
const toneClass: Record<Tone, string> = {
  neutral: 'bg-surface-strong text-ink-muted',
  success: 'bg-success-soft text-success',
  warning: 'bg-warning-soft text-warning',
  danger: 'bg-danger-soft text-danger',
  accent: 'bg-accent-soft text-accent',
};

export function Badge({ tone = 'neutral', className, children }: { tone?: Tone; className?: string; children: React.ReactNode }) {
  return (
    <span className={cn('inline-flex h-[22px] items-center gap-1 whitespace-nowrap rounded-full px-2 text-xs font-medium', toneClass[tone], className)}>
      {children}
    </span>
  );
}

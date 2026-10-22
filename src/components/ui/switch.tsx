import * as React from 'react';
import { Switch as SwitchPrimitive } from 'radix-ui';
import { cn } from '@/lib/utils';

export const Switch = React.forwardRef<
  React.ElementRef<typeof SwitchPrimitive.Root>,
  React.ComponentPropsWithoutRef<typeof SwitchPrimitive.Root>
>(({ className, ...props }, ref) => (
  <SwitchPrimitive.Root
    ref={ref}
    className={cn(
      'peer inline-flex h-[24px] w-[42px] shrink-0 cursor-pointer items-center rounded-full p-[3px] transition-colors duration-200 disabled:cursor-not-allowed disabled:opacity-45 data-[state=checked]:bg-ink data-[state=unchecked]:bg-track',
      className,
    )}
    {...props}
  >
    <SwitchPrimitive.Thumb className="pointer-events-none block size-[18px] rounded-full bg-white shadow-[0_1px_2px_rgb(0_0_0/0.18)] dark:data-[state=checked]:bg-panel transition-transform duration-200 [transition-timing-function:cubic-bezier(0.22,1,0.36,1)] data-[state=checked]:translate-x-[18px] data-[state=unchecked]:translate-x-0" />
  </SwitchPrimitive.Root>
));
Switch.displayName = 'Switch';

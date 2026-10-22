import * as React from 'react';
import { cn } from '@/lib/utils';

export const Input = React.forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(({ className, ...props }, ref) => (
  <input
    ref={ref}
    className={cn(
      'flex h-9 w-full min-w-0 rounded-lg border border-border bg-card px-3 text-[14px] text-ink transition-colors placeholder:text-ink-faint hover:border-border-strong focus-visible:border-ink/40 focus-visible:ring-0 disabled:cursor-not-allowed disabled:opacity-50 aria-[invalid=true]:border-danger/60',
      className,
    )}
    {...props}
  />
));
Input.displayName = 'Input';

export const Textarea = React.forwardRef<HTMLTextAreaElement, React.TextareaHTMLAttributes<HTMLTextAreaElement>>(
  ({ className, ...props }, ref) => (
    <textarea
      ref={ref}
      className={cn(
        'flex min-h-[88px] w-full rounded-lg border border-border bg-card px-3 py-2 text-[14px] text-ink transition-colors placeholder:text-ink-faint hover:border-border-strong focus-visible:border-ink/40 focus-visible:ring-0 disabled:opacity-50',
        className,
      )}
      {...props}
    />
  ),
);
Textarea.displayName = 'Textarea';

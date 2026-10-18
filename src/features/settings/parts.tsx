import * as React from 'react';
import { cn } from '@/lib/utils';

export function SectionTitle({ children }: { children: React.ReactNode }) {
  return <h2 className="mb-6 font-display text-[30px] leading-9 tracking-[-0.01em] text-ink">{children}</h2>;
}

export function GroupLabel({ children, className }: { children: React.ReactNode; className?: string }) {
  return <h3 className={cn('mb-2.5 mt-7 text-[15px] font-semibold text-ink', className)}>{children}</h3>;
}

/** Beige rounded group of rows separated by hairlines. */
export function Group({ children, className }: { children: React.ReactNode; className?: string }) {
  return <div className={cn('rounded-xl bg-surface px-5 [&>*+*]:border-t [&>*+*]:border-border', className)}>{children}</div>;
}

export function Row({
  title,
  description,
  control,
  children,
  className,
  htmlFor,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  control?: React.ReactNode;
  children?: React.ReactNode;
  className?: string;
  htmlFor?: string;
}) {
  return (
    <div className={cn('py-4', className)}>
      <div className="flex min-h-9 items-center gap-6">
        <div className="min-w-0 flex-1">
          <label htmlFor={htmlFor} className="block text-[15px] font-medium leading-5 text-ink">
            {title}
          </label>
          {description && <div className="mt-1 text-[14px] leading-5 text-ink-muted">{description}</div>}
        </div>
        {control && <div className="flex shrink-0 items-center gap-2">{control}</div>}
      </div>
      {children && <div className="mt-3">{children}</div>}
    </div>
  );
}

export function Hint({ tone = 'muted', children }: { tone?: 'muted' | 'warning' | 'danger'; children: React.ReactNode }) {
  return (
    <p className={cn('text-[13px] leading-5', tone === 'muted' && 'text-ink-muted', tone === 'warning' && 'text-warning', tone === 'danger' && 'text-danger')}>
      {children}
    </p>
  );
}

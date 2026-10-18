import { cva } from 'class-variance-authority';

export const buttonVariants = cva(
  'inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-lg font-medium transition-[background-color,color,box-shadow,transform] duration-150 active:scale-[0.98] disabled:pointer-events-none disabled:opacity-45 [&_svg]:size-4 [&_svg]:shrink-0',
  {
    variants: {
      variant: {
        primary: 'bg-ink text-panel hover:bg-ink/85',
        secondary: 'bg-surface-strong text-ink hover:bg-[#e6e2d7]',
        outline: 'border border-border bg-panel text-ink hover:bg-surface',
        ghost: 'text-ink hover:bg-surface-strong',
        subtle: 'text-ink-muted hover:bg-surface-strong hover:text-ink',
        danger: 'bg-danger text-white hover:bg-danger/90',
        'danger-ghost': 'text-danger hover:bg-danger-soft',
      },
      size: {
        sm: 'h-8 px-3 text-sm',
        md: 'h-9 px-4 text-[14px]',
        lg: 'h-10 px-5 text-base',
        icon: 'size-8 p-0',
        'icon-sm': 'size-7 p-0 [&_svg]:size-[15px]',
      },
    },
    defaultVariants: { variant: 'secondary', size: 'md' },
  },
);

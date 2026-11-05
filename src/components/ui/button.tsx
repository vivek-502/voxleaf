'use client';

import * as React from 'react';
import { Slot } from './slot';
import { cva, type VariantProps } from 'class-variance-authority';
import { Loader2 } from 'lucide-react';

import { cn } from '@/utils/cn';

/**
 * Button primitive.
 *
 * Set in the interface's small-capital voice: mono, uppercase, tightly tracked,
 * with square corners. Sizes stay generous enough for touch: `sm` is 36px tall
 * and every icon button is at least 36px square.
 */
const buttonVariants = cva(
  [
    'relative inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-[3px]',
    'font-mono font-medium uppercase tracking-[0.08em]',
    'transition-[transform,background-color,color,border-color] duration-150',
    'ease-[var(--ease-out-quint)] select-none',
    'disabled:pointer-events-none disabled:opacity-45',
    'focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent',
    'active:translate-y-px',
  ],
  {
    variants: {
      variant: {
        primary: 'bg-accent text-[#fffaf1] hover:bg-accent-deep',
        secondary:
          'bg-transparent text-ink border border-hairline-strong hover:bg-overlay hover:border-ink/40',
        outline:
          'border border-hairline text-ink hover:border-ink hover:bg-transparent',
        ghost: 'text-ink-muted hover:text-ink hover:bg-overlay',
        danger:
          'border border-danger/45 text-danger hover:bg-danger/10 hover:border-danger',
        subtle: 'bg-accent-wash text-accent border border-accent-line hover:bg-accent/15',
      },
      size: {
        sm: 'h-9 px-3.5 text-[11px]',
        md: 'h-10 px-4 text-[11px]',
        lg: 'h-12 px-6 text-[12px]',
        icon: 'size-10 p-0',
        'icon-sm': 'size-9 p-0',
        'icon-lg': 'size-12 p-0',
      },
      block: {
        true: 'w-full',
      },
    },
    defaultVariants: { variant: 'primary', size: 'md' },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
  loading?: boolean;
  /** Rendered before the label. Hidden while loading. */
  icon?: React.ReactNode;
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { className, variant, size, block, asChild = false, loading = false, icon, children, disabled, ...props },
  ref,
) {
  const classes = cn(buttonVariants({ variant, size, block }), className);
  const content = (
    <>
      {loading ? <Loader2 className="size-4 shrink-0 animate-spin" aria-hidden /> : icon}
      {children}
    </>
  );

  if (asChild && !loading) {
    return (
      <Slot ref={ref} className={classes} {...props}>
        {children}
      </Slot>
    );
  }

  return (
    <button
      ref={ref}
      className={classes}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      {...props}
    >
      {content}
    </button>
  );
});

export { buttonVariants };

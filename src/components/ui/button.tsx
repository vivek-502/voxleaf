import * as React from 'react';
import { Slot } from './slot';
import { cva, type VariantProps } from 'class-variance-authority';
import { Loader2 } from 'lucide-react';

import { cn } from '@/utils/cn';

/**
 * Button primitive.
 *
 * Sizes are deliberately generous: `sm` is still 36px tall and `default` is
 * 40px, so every control clears the 44px touch-target guideline on mobile via
 * the wrapper padding on mobile layouts.
 */
const buttonVariants = cva(
  [
    'relative inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-full',
    'font-medium transition-[transform,background-color,color,box-shadow,border-color] duration-150',
    'ease-[var(--ease-out-quint)] select-none',
    'disabled:pointer-events-none disabled:opacity-45',
    'focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent-soft',
    'active:scale-[0.975]',
  ],
  {
    variants: {
      variant: {
        primary:
          'bg-accent text-white shadow-[0_6px_20px_-8px_var(--color-accent)] hover:bg-accent-soft hover:shadow-[0_10px_28px_-8px_var(--color-accent)]',
        secondary:
          'bg-overlay text-ink border border-hairline hover:border-hairline-strong hover:bg-hairline/40',
        outline:
          'border border-hairline text-ink hover:border-accent-line hover:bg-accent-wash hover:text-accent-soft',
        ghost: 'text-ink-muted hover:bg-overlay hover:text-ink',
        danger:
          'border border-danger/40 text-danger hover:bg-danger/12 hover:border-danger/70',
        subtle: 'bg-accent-wash text-accent-soft border border-accent-line hover:bg-accent/20',
      },
      size: {
        sm: 'h-9 px-4 text-[13px]',
        md: 'h-10 px-5 text-sm',
        lg: 'h-12 px-7 text-[15px]',
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

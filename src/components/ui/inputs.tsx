import * as React from 'react';

import { cn } from '@/utils/cn';

export const Input = React.forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(
  function Input({ className, type = 'text', ...props }, ref) {
    return (
      <input
        ref={ref}
        type={type}
        className={cn(
          'flex h-11 w-full rounded-xl border border-hairline bg-surface px-4 text-sm text-ink',
          'placeholder:text-ink-subtle transition-colors',
          'hover:border-hairline-strong',
          'focus:border-accent-line focus:outline-none',
          'focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent-soft',
          'disabled:cursor-not-allowed disabled:opacity-50',
          'aria-[invalid=true]:border-danger aria-[invalid=true]:outline-none',
          className,
        )}
        {...props}
      />
    );
  },
);

export const Textarea = React.forwardRef<
  HTMLTextAreaElement,
  React.TextareaHTMLAttributes<HTMLTextAreaElement>
>(function Textarea({ className, ...props }, ref) {
  return (
    <textarea
      ref={ref}
      className={cn(
        'flex min-h-24 w-full resize-y rounded-xl border border-hairline bg-surface px-4 py-3',
        'text-sm leading-relaxed text-ink placeholder:text-ink-subtle transition-colors',
        'hover:border-hairline-strong focus:border-accent-line focus:outline-none',
        'focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent-soft',
        'disabled:cursor-not-allowed disabled:opacity-50',
        className,
      )}
      {...props}
    />
  );
});

export function Label({ className, ...props }: React.LabelHTMLAttributes<HTMLLabelElement>) {
  return (
    <label
      className={cn('text-sm font-medium text-ink', className)}
      {...props}
    />
  );
}

export function FieldDescription({ className, ...props }: React.HTMLAttributes<HTMLParagraphElement>) {
  return <p className={cn('text-xs leading-relaxed text-ink-subtle', className)} {...props} />;
}

export function FieldError({ className, ...props }: React.HTMLAttributes<HTMLParagraphElement>) {
  return (
    <p
      role="alert"
      className={cn('flex items-start gap-1.5 text-xs font-medium text-danger', className)}
      {...props}
    />
  );
}

/** Label + control + description/error, with the wiring already done. */
export function Field({
  label,
  htmlFor,
  hint,
  error,
  children,
  className,
}: {
  label: React.ReactNode;
  htmlFor: string;
  hint?: React.ReactNode;
  error?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={cn('space-y-2', className)}>
      <div className="flex items-baseline justify-between gap-3">
        <Label htmlFor={htmlFor}>{label}</Label>
      </div>
      {children}
      {error ? <FieldError>{error}</FieldError> : hint ? <FieldDescription>{hint}</FieldDescription> : null}
    </div>
  );
}

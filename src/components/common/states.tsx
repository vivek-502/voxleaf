import * as React from 'react';
import { Link } from 'react-router-dom';
import { AlertTriangle, Inbox, RefreshCw, SearchX, WifiOff } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/primitives';
import { cn } from '@/utils/cn';

type IconComponent = React.ComponentType<{ className?: string; 'aria-hidden'?: boolean | 'true' | 'false' }>;

export interface EmptyStateProps {
  icon?: IconComponent;
  title: string;
  description?: React.ReactNode;
  action?: React.ReactNode;
  secondaryAction?: React.ReactNode;
  className?: string;
}

/**
 * "Nothing here yet" — always paired with the action that fills the space, so
 * the page never dead-ends.
 */
export function EmptyState({
  icon: Icon = Inbox,
  title,
  description,
  action,
  secondaryAction,
  className,
}: EmptyStateProps) {
  return (
    <div
      className={cn(
        'flex flex-col items-center justify-center rounded-panel border border-dashed border-hairline px-6 py-16 text-center',
        className,
      )}
    >
      <span className="grid size-14 place-items-center rounded-2xl bg-overlay text-ink-subtle">
        <Icon className="size-6" aria-hidden />
      </span>
      <h2 className="mt-5 text-lg font-semibold text-ink">{title}</h2>
      {description && (
        <p className="mt-2 max-w-sm text-sm leading-relaxed text-ink-muted">{description}</p>
      )}
      {(action || secondaryAction) && (
        <div className="mt-6 flex flex-wrap items-center justify-center gap-2">
          {action}
          {secondaryAction}
        </div>
      )}
    </div>
  );
}

export interface ErrorStateProps {
  title?: string;
  message?: string;
  onRetry?: () => void;
  /** Distinguishes "the provider is down" from "this request failed". */
  offline?: boolean;
  className?: string;
}

/**
 * Failure state. Always states what went wrong *and* offers a retry, so a
 * transient error never looks like a permanent one.
 */
export function ErrorState({
  title = 'Something went wrong',
  message,
  onRetry,
  offline = false,
  className,
}: ErrorStateProps) {
  const Icon = offline ? WifiOff : AlertTriangle;

  return (
    <div
      role="alert"
      className={cn(
        'flex flex-col items-center justify-center rounded-panel border border-danger/25 bg-danger/5 px-6 py-14 text-center',
        className,
      )}
    >
      <span className="grid size-14 place-items-center rounded-2xl bg-danger/12 text-danger">
        <Icon className="size-6" aria-hidden />
      </span>
      <h2 className="mt-5 text-lg font-semibold text-ink">{title}</h2>
      {message && <p className="mt-2 max-w-sm text-sm leading-relaxed text-ink-muted">{message}</p>}
      {onRetry && (
        <Button
          variant="outline"
          className="mt-6"
          onClick={onRetry}
          icon={<RefreshCw className="size-4" aria-hidden />}
        >
          Try again
        </Button>
      )}
    </div>
  );
}

/** Search-specific empty state, with a way back to browsing. */
export function NoResults({
  query,
  onClear,
}: {
  query: string;
  onClear?: () => void;
}) {
  return (
    <EmptyState
      icon={SearchX}
      title={`No results for “${query}”`}
      description="Try a different spelling, search by author, or browse a category instead."
      action={
        onClear ? (
          <Button variant="outline" onClick={onClear} icon={<RefreshCw className="size-4" aria-hidden />}>
            Clear search
          </Button>
        ) : (
          <Button asChild variant="outline">
            <Link to="/store">Browse the store</Link>
          </Button>
        )
      }
    />
  );
}

/* -------------------------------------------------------------------------- */
/* Loading skeletons                                                           */
/* -------------------------------------------------------------------------- */

function CoverSkeleton({ className }: { className?: string }) {
  return <Skeleton className={cn('aspect-[2/3] w-full rounded-lg', className)} />;
}

/** Grid of book cards. Mirrors `BookGrid`'s layout so there is no shift. */
export function BookGridSkeleton({ count = 12 }: { count?: number }) {
  return (
    <div
      role="status"
      aria-label="Loading books"
      className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6"
    >
      {Array.from({ length: count }, (_, index) => (
        <div key={index} className="flex flex-col gap-2.5">
          <CoverSkeleton />
          <Skeleton className="h-3.5 w-4/5" />
          <Skeleton className="h-3 w-3/5" />
        </div>
      ))}
      <span className="sr-only-focusable">Loading…</span>
    </div>
  );
}

/** Vertical list of chapter rows. */
export function ChapterListSkeleton({ count = 6 }: { count?: number }) {
  return (
    <div role="status" aria-label="Loading chapters" className="flex flex-col gap-2">
      {Array.from({ length: count }, (_, index) => (
        <div
          key={index}
          className="card-surface flex items-center gap-3 p-3"
        >
          <Skeleton className="size-9 shrink-0 rounded-full" />
          <div className="flex-1">
            <Skeleton className="h-3.5 w-1/3" />
            <Skeleton className="mt-2 h-3 w-2/3" />
          </div>
          <Skeleton className="size-9 shrink-0 rounded-full" />
        </div>
      ))}
      <span className="sr-only-focusable">Loading…</span>
    </div>
  );
}

/** Horizontal cards used by Continue / Recently listened shelves. */
export function BookRowSkeleton({ count = 6 }: { count?: number }) {
  return (
    <div
      role="status"
      aria-label="Loading books"
      className="flex gap-4 overflow-hidden pb-2"
    >
      {Array.from({ length: count }, (_, index) => (
        <div key={index} className="flex w-40 shrink-0 flex-col gap-2.5">
          <CoverSkeleton />
          <Skeleton className="h-3.5 w-4/5" />
          <Skeleton className="h-3 w-3/5" />
        </div>
      ))}
      <span className="sr-only-focusable">Loading…</span>
    </div>
  );
}

/** Full-page loader, used as the Suspense fallback for route segments. */
export function PageSkeleton() {
  return (
    <div role="status" aria-label="Loading" className="container-page py-8">
      <Skeleton className="h-8 w-48" />
      <Skeleton className="mt-3 h-4 w-80" />
      <div className="mt-8">
        <BookGridSkeleton count={10} />
      </div>
      <span className="sr-only-focusable">Loading…</span>
    </div>
  );
}

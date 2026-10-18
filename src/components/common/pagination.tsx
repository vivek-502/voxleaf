import * as React from 'react';

import { Button } from '@/components/ui/button';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { cn } from '@/utils/cn';

export interface PaginationProps {
  page: number;
  hasMore: boolean;
  onPageChange: (page: number) => void;
  /** Total items, when known — used to render the "Showing X–Y of Z" line. */
  total?: number;
  pageSize?: number;
  className?: string;
}

/**
 * Page controls.
 *
 * Uses explicit previous/next buttons rather than a page-number grid: the
 * results are a continuously scrolling list, so jumping to an arbitrary page
 * is less useful than stepping, and this stays usable on narrow screens.
 */
export function Pagination({
  page,
  hasMore,
  onPageChange,
  total,
  pageSize,
  className,
}: PaginationProps) {
  const from = total !== undefined && pageSize ? (page - 1) * pageSize + 1 : null;
  const to = total !== undefined && pageSize ? Math.min(page * pageSize, total) : null;

  return (
    <nav
      aria-label="Pagination"
      className={cn('mt-8 flex flex-wrap items-center justify-between gap-3', className)}
    >
      <p className="text-xs text-ink-subtle" aria-live="polite">
        {from !== null && to !== null
          ? `Showing ${from}–${to} of ${total}`
          : page > 1 || hasMore
            ? `Page ${page}`
            : ''}
      </p>

      <div className="flex items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          disabled={page <= 1}
          onClick={() => onPageChange(page - 1)}
          icon={<ChevronLeft className="size-4" aria-hidden />}
        >
          Previous
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={!hasMore}
          onClick={() => onPageChange(page + 1)}
        >
          Next
          <ChevronRight className="size-4" aria-hidden />
        </Button>
      </div>
    </nav>
  );
}

import * as React from 'react';
import * as TabsPrimitive from '@radix-ui/react-tabs';

import { cn } from '@/utils/cn';

export const Tabs = TabsPrimitive.Root;

export const TabsList = React.forwardRef<
  React.ComponentRef<typeof TabsPrimitive.List>,
  React.ComponentPropsWithoutRef<typeof TabsPrimitive.List>
>(function TabsList({ className, ...props }, ref) {
  return (
    <TabsPrimitive.List
      ref={ref}
      className={cn(
        'inline-flex items-center gap-1 rounded-full border border-hairline bg-surface p-1',
        className,
      )}
      {...props}
    />
  );
});

export const TabsTrigger = React.forwardRef<
  React.ComponentRef<typeof TabsPrimitive.Trigger>,
  React.ComponentPropsWithoutRef<typeof TabsPrimitive.Trigger>
>(function TabsTrigger({ className, ...props }, ref) {
  return (
    <TabsPrimitive.Trigger
      ref={ref}
      className={cn(
        'inline-flex items-center justify-center gap-1.5 whitespace-nowrap rounded-full px-4 py-1.5',
        'text-sm font-medium text-ink-muted transition-all duration-150',
        'hover:text-ink focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent-soft',
        // Active tab uses a filled pill, so selection is not colour-only.
        'data-[state=active]:bg-accent data-[state=active]:text-white data-[state=active]:shadow-[0_4px_14px_-6px_var(--color-accent)]',
        '[&_svg]:size-4',
        className,
      )}
      {...props}
    />
  );
});

export const TabsContent = TabsPrimitive.Content;

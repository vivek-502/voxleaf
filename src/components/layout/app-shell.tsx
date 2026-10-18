import * as React from 'react';

import { Sidebar } from './sidebar';
import { MobileNav } from './mobile-nav';
import { TopBar } from './top-bar';
import { MiniPlayer } from '@/components/player/mini-player';
import { FullPlayer } from '@/components/player/full-player';
import { usePlayer } from '@/components/providers/player-provider';

/**
 * The persistent app frame.
 *
 * Rendered by the root layout and therefore never unmounted, which is what
 * lets audio keep playing while the router swaps the page underneath.
 *
 * Vertical space is budgeted as:
 *   scrolling content  →  mini-player  →  mobile bottom nav
 * The bottom two live in a single fixed flex column, so the nav can never end
 * up underneath the player, and the content column reserves exactly as much
 * padding as that stack currently occupies.
 */
export function AppShell({ children }: { children: React.ReactNode }) {
  const { hasTrack, isExpanded } = usePlayer();
  const miniPlayerVisible = hasTrack && !isExpanded;

  return (
    <div
      className="relative flex min-h-dvh"
      // Consumed by the content column's padding and the mobile nav inset.
      style={{ ['--mini-player-height' as string]: miniPlayerVisible ? '72px' : '0px' }}
    >
      <Sidebar />

      <div className="flex min-w-0 flex-1 flex-col">
        {/* TopBar uses useSearchParams, which requires a Suspense boundary
            during prerendering (including the 404 page). */}
        <React.Suspense fallback={<div className="h-16 border-b border-hairline" />}>
          <TopBar />
        </React.Suspense>

        <main id="main" tabIndex={-1} className="flex-1 pb-[calc(58px+var(--mini-player-height)+var(--safe-bottom))] focus:outline-none lg:pb-[var(--mini-player-height)]">
          {children}
        </main>
      </div>

      <div className="pointer-events-none fixed inset-x-0 bottom-0 z-50 flex flex-col">
        <div className="pointer-events-auto">
          <MiniPlayer />
        </div>
        <div className="pointer-events-auto lg:hidden">
          <MobileNav />
        </div>
      </div>

      <FullPlayer />
    </div>
  );
}

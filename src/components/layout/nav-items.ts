import {
  Bookmark,
  Clock,
  Compass,
  Heart,
  Home,
  Library,
  Settings,
  type LucideIcon,
} from 'lucide-react';

/**
 * Navigation model shared by the masthead and the mobile bottom bar, so the
 * two can never drift apart.
 */
export interface NavItem {
  href: string;
  label: string;
  icon: LucideIcon;
  /** Longer description, used by the mobile bar's accessible label. */
  description: string;
}

export const PRIMARY_NAV: NavItem[] = [
  {
    href: '/',
    label: 'Home',
    icon: Home,
    description: 'The front page of your listening room',
  },
  {
    href: '/store',
    label: 'Discover',
    icon: Compass,
    description: 'Search millions of books and add them to your library',
  },
  {
    href: '/library',
    label: 'Library',
    icon: Library,
    description: 'Everything on your shelves',
  },
  {
    href: '/continue',
    label: 'Continue',
    icon: Clock,
    description: 'Pick up where you left off',
  },
];

export const SECONDARY_NAV: NavItem[] = [
  {
    href: '/bookmarks',
    label: 'Saved',
    icon: Bookmark,
    description: 'Bookmarks and favourite books',
  },
  {
    href: '/favorites',
    label: 'Favorites',
    icon: Heart,
    description: 'Books you have starred',
  },
  {
    href: '/settings',
    label: 'Settings',
    icon: Settings,
    description: 'Playback, appearance and data',
  },
];

/** Exactly the five destinations that fit in the mobile bottom bar. */
export const MOBILE_NAV: NavItem[] = [
  PRIMARY_NAV[0],
  PRIMARY_NAV[1],
  PRIMARY_NAV[2],
  {
    href: '/bookmarks',
    label: 'Saved',
    icon: Bookmark,
    description: SECONDARY_NAV[0].description,
  },
  PRIMARY_NAV[3],
];

export function isNavActive(pathname: string, href: string): boolean {
  if (href === '/') return pathname === '/';
  if (href === '/bookmarks') return pathname === '/bookmarks' || pathname === '/favorites';
  return pathname === href || pathname.startsWith(`${href}/`);
}

import {
  Bookmark,
  Clock,
  Compass,
  Heart,
  Library,
  Moon,
  Settings,
  Sun,
  type LucideIcon,
} from 'lucide-react';

/**
 * Navigation model shared by the desktop sidebar, the mobile bottom bar and
 * the command palette, so the three can never drift apart.
 */
export interface NavItem {
  href: string;
  label: string;
  icon: LucideIcon;
  /** Longer description, used by the command palette and as a tooltip. */
  description: string;
  /** Shown in the mobile bottom bar. Hidden from the sidebar on large screens. */
  primary?: boolean;
}

export const PRIMARY_NAV: NavItem[] = [
  {
    href: '/',
    label: 'Home',
    icon: Compass,
    description: 'Your dashboard and recent listening',
    primary: true,
  },
  {
    href: '/store',
    label: 'Store',
    icon: Library,
    description: 'Search millions of books and add them to your library',
    primary: true,
  },
  {
    href: '/library',
    label: 'Library',
    icon: Library,
    description: 'Everything you have added',
    primary: true,
  },
  {
    href: '/continue',
    label: 'Continue',
    icon: Clock,
    description: 'Pick up where you left off',
    primary: true,
  },
];

export const SECONDARY_NAV: NavItem[] = [
  {
    href: '/favorites',
    label: 'Favorites',
    icon: Heart,
    description: 'Books you have starred',
    primary: false,
  },
  {
    href: '/bookmarks',
    label: 'Bookmarks',
    icon: Bookmark,
    description: 'Moments you saved while listening',
    primary: false,
  },
  {
    href: '/settings',
    label: 'Settings',
    icon: Settings,
    description: 'Playback, appearance and data',
    primary: false,
  },
];

/** Exactly the five destinations that fit in a mobile bottom bar. */
export const MOBILE_NAV: NavItem[] = [
  PRIMARY_NAV[0],
  PRIMARY_NAV[1],
  PRIMARY_NAV[2],
  {
    href: '/bookmarks',
    label: 'Saved',
    icon: Bookmark,
    description: SECONDARY_NAV[1].description,
    primary: true,
  },
  PRIMARY_NAV[3],
];

export const THEME_ICONS: Record<string, LucideIcon> = {
  light: Sun,
  dark: Moon,
  system: Settings,
};

export function isNavActive(pathname: string, href: string): boolean {
  if (href === '/') return pathname === '/';
  return pathname === href || pathname.startsWith(`${href}/`);
}

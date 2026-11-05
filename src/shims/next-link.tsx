import * as React from 'react';
import { Link as RouterLink, type LinkProps } from 'react-router-dom';

type NextLinkProps = Omit<LinkProps, 'to'> & {
  href: string;
  /** Next-only hint; navigation is synchronous here. */
  prefetch?: boolean;
  /** Next-only hint; React Router scrolls to the top on navigation. */
  scroll?: boolean;
};

/**
 * `next/link` on React Router.
 *
 * The ported pages use Next's `<Link href>` API. This keeps those files almost
 * identical to the web app while routing through the desktop's BrowserRouter.
 */
const Link = React.forwardRef<HTMLAnchorElement, NextLinkProps>(function Link(
  { href, prefetch: _prefetch, scroll: _scroll, ...props },
  ref,
) {
  return <RouterLink ref={ref} to={href} {...props} />;
});

export default Link;

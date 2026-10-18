import * as React from 'react';

/**
 * A minimal `Slot` that merges a component's props onto its single child.
 * Lets `Button asChild` render a `<Link>` or `<a>` while keeping the button
 * styling, focus ring and click handlers.
 */
export const Slot = React.forwardRef<
  HTMLElement,
  React.HTMLAttributes<HTMLElement> & { children?: React.ReactNode }
>(function Slot({ children, ...slotProps }, ref) {
  if (!React.isValidElement(children)) {
    return null;
  }

  const child = children as React.ReactElement<Record<string, unknown>>;
  const childProps = child.props as Record<string, unknown>;
  // Merging happens on untyped bags: the child can be any element, so its prop
  // names are not statically known to TypeScript.
  const own = slotProps as Record<string, unknown>;

  const merged: Record<string, unknown> = { ...own };
  for (const [key, value] of Object.entries(childProps)) {
    // Slot handlers run first, then the child's own handler, unless the child
    // explicitly stops propagation.
    if (key.startsWith('on') && typeof value === 'function' && typeof own[key] === 'function') {
      const slotHandler = own[key] as (...args: unknown[]) => void;
      const childHandler = value as (...args: unknown[]) => void;
      merged[key] = (...args: unknown[]) => {
        slotHandler(...args);
        if (!(args[0] as { defaultPrevented?: boolean })?.defaultPrevented) {
          childHandler(...args);
        }
      };
    } else if (key === 'className') {
      merged.className = [own.className, value].filter(Boolean).join(' ');
    } else if (key === 'style') {
      merged.style = { ...(own.style as object), ...(value as object) };
    } else {
      merged[key] = value;
    }
  }

  if (ref) {
    merged.ref = ref;
  }

  return React.cloneElement(child, merged);
});

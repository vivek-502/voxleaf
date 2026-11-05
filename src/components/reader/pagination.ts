/**
 * Geometry for reading a section as facing pages, the way Foliate lays out a
 * book.
 *
 * Every page of the section lives in one multi-column article: the text runs to
 * the bottom of the first column, continues in the next column to its right,
 * and overflows sideways. On a screen wide enough for it two columns are
 * visible at once — an open book — and turning a page slides the article by the
 * width of that spread. On a phone the same box is one column wide, so a turn
 * advances one page.
 *
 * The limits are Foliate's own: a page never grows past 720 CSS pixels or 1440
 * tall, the gutter is 6% of the spread, and the outer margins are capped at 48
 * pixels. Keeping that measure is what makes a web chapter read like printed
 * pages instead of a full-width web column.
 *
 * Nothing here reads or writes React state: it measures the DOM and answers the
 * questions the reader asks after every reflow — how many times must the pages
 * be turned, which spread holds this element, and how many printed pages the
 * section has.
 */

export interface PageLayout {
  /** Width of the visible area: one page, or the two-page spread. */
  width: number;
  height: number;
  /** Blank space between the text of one page and the text of the next. */
  gap: number;
  /** Width of one text column — one printed page of text. */
  columnWidth: number;
  /** Columns visible at once: 1 on a phone, 2 on a screen with room for a spread. */
  columns: number;
}

/** Widest a single printed page of text may get, in CSS pixels. */
export const MAX_PAGE_INLINE_SIZE = 720;
/** Tallest a page may get before it stops growing with the window. */
export const MAX_PAGE_BLOCK_SIZE = 1440;
/** Gutter as a fraction of the spread, before the outer half-gutters are added. */
const GUTTER_RATIO = 0.06;

/**
 * How the visible area splits into pages.
 *
 * A spread appears only once the area is wider than one maximum-width page.
 * Below that, two columns would each be narrower than a book's text block, and
 * a single page is the honest layout.
 */
export function pageLayout(width: number, height: number): PageLayout {
  const columns = width > MAX_PAGE_INLINE_SIZE ? 2 : 1;
  // Foliate widens the inner gutter so that, after the outer half-gutters are
  // added, the spacing between columns and at the edges looks even: g' = g / (1 - g).
  const gap = Math.round((GUTTER_RATIO / (1 - GUTTER_RATIO)) * width);
  return {
    width,
    height,
    gap,
    columnWidth: Math.max(80, width / columns - gap),
    columns,
  };
}

/**
 * The horizontal extent of the article's text, from the article's left edge.
 *
 * Measured with a `Range` rather than `scrollWidth`: both include the columns
 * that overflow the box, but the range reports the extent of the *text*, which
 * is the number of pages a reader can actually turn to. A section whose last
 * column holds only a trailing margin should not end on a blank page.
 *
 * The range's rect is in viewport coordinates, and so is the article's, so the
 * article's current page offset cancels out.
 */
function contentExtent(article: HTMLElement): number {
  const range = article.ownerDocument.createRange();
  range.selectNodeContents(article);
  const content = range.getBoundingClientRect();
  const origin = article.getBoundingClientRect();
  return content.width > 0 ? content.right - origin.left : article.scrollWidth;
}

/**
 * How many spreads, and how many printed pages, the laid-out section holds.
 *
 * A spread is what a turn advances; a printed page is one column, so a spread
 * covers two of them. The page label counts columns — "pages 5–6 of 12" — which
 * is what a reader would call the two sides of the book in front of them.
 */
export function measurePageFill(
  article: HTMLElement,
  layout: PageLayout,
): { pages: number; printed: number } {
  if (!(layout.width > 0)) return { pages: 1, printed: 1 };
  const extent = contentExtent(article);
  if (!(extent > 0)) return { pages: 1, printed: 1 };
  // One pixel of slack: a column that is exactly full reports its right edge
  // fractionally past the boundary, which must not become an empty page.
  const columnStep = Math.max(1, layout.columnWidth + layout.gap);
  return {
    pages: Math.max(1, Math.ceil((extent - 1) / layout.width)),
    printed: Math.max(1, Math.ceil((extent - 1) / columnStep)),
  };
}

/** Which page a horizontal position belongs to, from the article's left edge. */
export function pageOfLeft(article: HTMLElement, step: number, left: number): number {
  if (!(step > 0)) return 0;
  const origin = article.getBoundingClientRect().left;
  return Math.max(0, Math.floor((left - origin) / step));
}

/** The first fragment of an element or range that has a real size. */
export function firstClientRect(target: Element | Range): DOMRect | null {
  const rects = target.getClientRects();
  for (const rect of Array.from(rects)) {
    if (rect.width > 0 && rect.height > 0) return rect;
  }
  return rects[0] ?? null;
}

/** Which page an element starts on, across a column break if it has one. */
export function pageOfElement(article: HTMLElement, step: number, element: Element): number {
  const rect = firstClientRect(element);
  if (!rect) return 0;
  return pageOfLeft(article, step, rect.left);
}

/** Which page a highlighted or found range starts on. */
export function pageOfRange(article: HTMLElement, step: number, range: Range): number {
  const rect = firstClientRect(range);
  if (!rect) return 0;
  return pageOfLeft(article, step, rect.left);
}

/**
 * The block at the top of a page.
 *
 * Blocks are in document order and so are the columns they start in, so the
 * answer is the last block that starts at or before this page — the same rule
 * the scrolled reader used for "at the top of the viewport", with the page
 * number standing in for the scroll position. A block that began on an earlier
 * page and continues onto this one resolves to itself, which is what the
 * reader is looking at.
 */
export function blockAtPage(
  article: HTMLElement,
  step: number,
  blocks: readonly { start: number; el: HTMLElement }[],
  page: number,
): { start: number; el: HTMLElement } | null {
  if (blocks.length === 0) return null;

  let low = 0;
  let high = blocks.length - 1;
  let found = 0;
  while (low <= high) {
    const mid = (low + high) >> 1;
    if (pageOfElement(article, step, blocks[mid]!.el) <= page) {
      found = mid;
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }
  return blocks[found]!;
}

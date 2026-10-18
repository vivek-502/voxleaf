/**
 * Reconciling the parser's character offsets with the browser's layout.
 *
 * A contents row addresses a place in a chapter by how many visible text
 * characters come before it — counted server-side, during sanitising, because
 * that is the only point where the source markup and the browser's view of it
 * are both known. The browser has no such counter, so the two are reconciled
 * here: walk the injected text nodes in order and note where each block begins.
 *
 * Offsets therefore mean the same thing on both sides up to the whitespace the
 * sanitiser tidies up afterwards, which is why every lookup snaps to a block
 * rather than to an exact scroll pixel. A few characters of drift lands on the
 * same paragraph a reader would have picked anyway.
 */

/** Elements a row can sensibly come to rest on. */
const BLOCK_SELECTOR =
  'p,h1,h2,h3,h4,h5,h6,li,dt,dd,blockquote,pre,figcaption,caption,td,th,figure,div,section,article,aside,table,tr';

/** The same list as a set, so the common case — text straight inside a `p` — is
 *  one lookup rather than a selector match per text node. */
const BLOCK_TAGS = new Set(BLOCK_SELECTOR.split(','));

export interface TextBlock {
  /** Visible-text offset of the block's first character. */
  start: number;
  /** Visible-text offset just past the block's last character. */
  end: number;
  el: HTMLElement;
}

/**
 * Index the blocks of an injected chapter, in text order.
 *
 * `start` rises monotonically, which is what lets both lookups below be a plain
 * binary search. Nested blocks are entered once each: a `p` inside a `div` gets
 * its own entry rather than being folded into the `div`, because the `p` is the
 * element worth scrolling to.
 */
export function buildTextIndex(root: HTMLElement): TextBlock[] {
  const doc = root.ownerDocument;
  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const blocks: TextBlock[] = [];
  /** Element -> the block its text belongs to, so repeats resolve to one entry. */
  const seen = new Map<Element, TextBlock>();

  let offset = 0;
  // The walker is asked for text nodes only, so every node it hands back is one.
  let node = walker.nextNode() as globalThis.Text | null;
  while (node) {
    const parent = node.parentElement;
    if (parent) {
      const el = BLOCK_TAGS.has(parent.tagName.toLowerCase())
        ? parent
        : (parent.closest<HTMLElement>(BLOCK_SELECTOR) ?? root);
      let block = seen.get(el);
      if (!block) {
        block = { start: offset, end: offset, el };
        seen.set(el, block);
        blocks.push(block);
      }
      seen.set(parent, block);
      offset += node.data.length;
      block.end = offset;
    }
    node = walker.nextNode() as globalThis.Text | null;
  }

  return blocks;
}

/**
 * The block a visible-text offset falls inside.
 *
 * The last block starting at or before the offset, so an offset landing in the
 * gap between two blocks resolves to the one the reader just finished rather
 * than the next one. An offset before the text starts resolves to the first
 * block, and an empty chapter to nothing.
 *
 * `refresh` re-reads the index from the DOM. It is called when the block found is
 * no longer connected — an index is a cache of live element references, and
 * element references die quietly. Rewriting a section's markup replaces every
 * element inside it while the index itself still looks perfectly ordinary, and a
 * detached element measures as a zero rect, which reads as "already at the top of
 * the viewport". A stale index therefore does not fail loudly: it turns every
 * contents jump into a silent no-op, marking the row active while the text stays
 * exactly where it was.
 */
export function blockAtOffset(
  blocks: readonly TextBlock[],
  offset: number,
  refresh?: () => readonly TextBlock[],
): TextBlock | null {
  const found = search(blocks, offset);
  if (found && found.el.isConnected) return found;
  if (!refresh) return found;

  const rebuilt = refresh();
  if (rebuilt === blocks) return found;
  return search(rebuilt, offset);
}

/** The last block starting at or before `offset`, or null for an empty index. */
function search(blocks: readonly TextBlock[], offset: number): TextBlock | null {
  if (blocks.length === 0) return null;

  let low = 0;
  let high = blocks.length - 1;
  let found = 0;
  while (low <= high) {
    const mid = (low + high) >> 1;
    if (blocks[mid]!.start <= offset) {
      found = mid;
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }
  return blocks[found]!;
}

/**
 * Put a block at the top of a scrolling container.
 *
 * `scrollIntoView` would work, but it also scrolls every scrollable ancestor it
 * can find — which, in a reader with a docked contents panel, is a good way to
 * move something the reader did not ask to move. Adjusting `scrollTop` by the
 * measured delta touches exactly one container.
 *
 * Returns where the container ended up, so the caller can tell whether a later
 * layout shift moved the text out from under the jump.
 */
export function scrollBlockIntoView(
  container: HTMLElement,
  block: TextBlock,
  padding = 0,
): number {
  const delta = block.el.getBoundingClientRect().top - container.getBoundingClientRect().top - padding;
  if (Math.abs(delta) >= 1) container.scrollTop += delta;
  return container.scrollTop;
}

/** Scroll a chapter so the block holding `offset` is at the top of `container`. */
export function scrollToOffset(
  container: HTMLElement,
  blocks: readonly TextBlock[],
  offset: number,
  padding = 0,
  refresh?: () => readonly TextBlock[],
): number {
  const block = blockAtOffset(blocks, offset, refresh);
  if (!block) return 0;
  return scrollBlockIntoView(container, block, padding);
}

/**
 * The visible-text offset at the top of what a reader is looking at.
 *
 * Blocks are in document order and so are their tops, so the answer is a binary
 * search rather than a scan of every block on every scroll frame.
 *
 * `probe` is a few pixels below the top edge: a row has to be a little way into
 * view before it counts as the one being read, or the highlight flickers between
 * a heading and the paragraph under it while the heading is still half on screen.
 */
export function offsetAtViewportTop(
  container: HTMLElement,
  blocks: readonly TextBlock[],
  probe = 12,
  refresh?: () => readonly TextBlock[],
): number {
  const found = atViewportTop(container, blocks, probe);
  // A detached element reports a zero rect, which would place every such block
  // above the probe line and answer with the wrong row entirely. Re-read the
  // index rather than report a position nobody is at.
  if (found && !found.el.isConnected && refresh) {
    const rebuilt = refresh();
    if (rebuilt !== blocks) return atViewportTop(container, rebuilt, probe)?.start ?? 0;
  }
  return found?.start ?? 0;
}

/** The last block at or above the probe line, or null for an empty index. */
function atViewportTop(
  container: HTMLElement,
  blocks: readonly TextBlock[],
  probe: number,
): TextBlock | null {
  if (blocks.length === 0) return null;
  const top = container.getBoundingClientRect().top + probe;

  let low = 0;
  let high = blocks.length - 1;
  let found = 0;
  while (low <= high) {
    const mid = (low + high) >> 1;
    if (blocks[mid]!.el.getBoundingClientRect().top <= top) {
      found = mid;
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }
  return blocks[found]!;
}
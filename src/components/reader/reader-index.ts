'use client';

import type { EpubChapterBlock } from '@/types';

/**
 * The bridge between an annotation's anchor and the words on screen.
 *
 * Annotations are stored against the *source* text: a block index and a pair of
 * character offsets, the same blocks the translator uses. The page, however, is
 * sanitised markup rendered by the browser, and may be the original or a
 * translation. This module reads the rendered block back into normalised text
 * with a map for every character to its DOM position, so an anchor can become a
 * `Range` (for the CSS Custom Highlight API) and a selection can become an
 * anchor.
 *
 * Normalisation mirrors the server's block extractor: invisible marks dropped,
 * whitespace runs collapsed, `<br>` kept as a line break. That is what makes a
 * stored offset mean the same thing after a reload, a reflow, or a change of
 * theme — and, with proportional mapping, after the section is translated into
 * another language.
 */

/** Block elements that own text, matching the server's extractor. */
const BLOCK_TAGS = [
  'h1',
  'h2',
  'h3',
  'h4',
  'h5',
  'h6',
  'p',
  'li',
  'blockquote',
  'figcaption',
  'pre',
  'figcaption',
  'th',
  'td',
] as const;

/** Containers that close a block on the server; their loose text becomes a paragraph. */
const CONTAINER_TAGS = ['div', 'section', 'article', 'ul', 'ol'] as const;

const SELECTOR = [...BLOCK_TAGS, ...CONTAINER_TAGS].join(',');
const CONTAINER_SET = new Set<string>(CONTAINER_TAGS);
/** Elements that end the text a block owns: a nested block is read separately. */
const BOUNDARY_SELECTOR = `${SELECTOR},img,figure,table,thead,tbody,tfoot,tr`;

const INVISIBLE = /[\u00AD\u200B-\u200F\u202A-\u202E\u2060-\u2064\u2066-\u2069\uFEFF]/;

interface DomPoint {
  node: Node;
  offset: number;
  /** A `<br>` is a point between its neighbours, not a text offset. */
  element?: 'before' | 'after';
}

interface CharPoint {
  start: DomPoint;
  end: DomPoint;
}

export interface ReaderBlock {
  /** Index into the chapter's (or translation's) block array, or -1 if unmatched. */
  serverIndex: number;
  tag: string;
  text: string;
  el: HTMLElement;
  points: CharPoint[];
}

interface RawChar {
  char: string;
  start: DomPoint;
  end: DomPoint;
}

/** Assign DOM order numbers, which give a comparable position for any boundary. */
function collectRaw(
  root: HTMLElement,
  order: Map<Node, number>,
  nextOrder: { value: number },
): RawChar[] {
  const raw: RawChar[] = [];

  const visit = (node: Node) => {
    if (node.nodeType === Node.TEXT_NODE) {
      const text = node as Text;
      const index = nextOrder.value;
      nextOrder.value += 1;
      order.set(text, index);
      for (let at = 0; at < text.data.length; at += 1) {
        raw.push({
          char: text.data[at]!,
          start: { node: text, offset: at },
          end: { node: text, offset: at + 1 },
        });
      }
      return;
    }

    if (node.nodeType !== Node.ELEMENT_NODE) return;
    const element = node as HTMLElement;
    const tag = element.tagName.toLowerCase();

    if (tag === 'br') {
      const index = nextOrder.value;
      nextOrder.value += 1;
      order.set(element, index);
      raw.push({
        char: '\n',
        start: { node: element, offset: 0, element: 'before' },
        end: { node: element, offset: 0, element: 'after' },
      });
      return;
    }

    // A nested block (or a figure/table/image) is another block's text; the
    // server closes the current block at the same boundary.
    if (element !== root && element.matches(BOUNDARY_SELECTOR)) return;

    for (const child of element.childNodes) visit(child);
  };

  for (const child of root.childNodes) visit(child);
  return raw;
}

/**
 * Collapse whitespace the way the server's `tidy` does, keeping each emitted
 * character's original boundary. Leading and trailing whitespace disappears; a
 * run containing line breaks becomes one or two `\n`, anything else one space.
 */
function normalize(raw: RawChar[]): { text: string; points: CharPoint[] } {
  let text = '';
  const points: CharPoint[] = [];
  let pending: RawChar[] = [];

  const flush = (followedByText: boolean) => {
    if (pending.length === 0) return;
    if (!followedByText || text.length === 0) {
      pending = [];
      return;
    }

    const newlines = pending.filter((item) => item.char === '\n');
    if (newlines.length > 0) {
      const count = Math.min(2, newlines.length);
      const first = newlines[0]!;
      const last = newlines[newlines.length - 1]!;
      for (let index = 0; index < count; index += 1) {
        text += '\n';
        points.push({ start: first.start, end: last.end });
      }
    } else {
      text += ' ';
      points.push({ start: pending[0]!.start, end: pending[pending.length - 1]!.end });
    }
    pending = [];
  };

  for (const item of raw) {
    if (INVISIBLE.test(item.char)) continue;
    if (item.char === '\r') continue;
    if (item.char === '\n' || /\s/.test(item.char)) {
      pending.push(item);
      continue;
    }
    flush(true);
    text += item.char;
    points.push({ start: item.start, end: item.end });
  }
  flush(false);

  return { text, points };
}

function buildBlock(el: HTMLElement, tag: string, serverIndex: number): ReaderBlock | null {
  const order = new Map<Node, number>();
  const raw = collectRaw(el, order, { value: 0 });
  const { text, points } = normalize(raw);
  if (!text) return null;
  return { serverIndex, tag, text, el, points };
}

/**
 * Blocks of the rendered original, matched to the server's block array.
 *
 * The server drops empty blocks (an image is a block with no text); the DOM has
 * no entry for them either. Matching walks both lists in order and tolerates the
 * gaps, so the index on a block is the index an annotation would be stored with.
 */
export function buildReaderBlocks(root: HTMLElement, serverBlocks: EpubChapterBlock[]): ReaderBlock[] {
  const blocks: ReaderBlock[] = [];
  let cursor = 0;

  const candidates = root.querySelectorAll<HTMLElement>(SELECTOR);
  for (const el of candidates) {
    const rawTag = el.tagName.toLowerCase();
    const tag = CONTAINER_SET.has(rawTag) ? 'p' : rawTag;
    const built = buildBlock(el, tag, -1);
    if (!built) continue;

    let matched = -1;
    // Search a little ahead for the same text. Blocks can be present on the
    // server and absent here (or the reverse) when a parser and a browser
    // disagree about an exotic structure; a nearby match is still the right one.
    for (let at = cursor; at < Math.min(serverBlocks.length, cursor + 12); at += 1) {
      const candidate = serverBlocks[at]!;
      if (candidate.tag === built.tag && candidate.text === built.text) {
        matched = at;
        break;
      }
    }
    if (matched === -1) {
      for (let at = cursor; at < Math.min(serverBlocks.length, cursor + 12); at += 1) {
        if (serverBlocks[at]!.tag === built.tag) {
          matched = at;
          break;
        }
      }
    }
    built.serverIndex = matched;
    if (matched !== -1) cursor = matched + 1;
    blocks.push(built);
  }

  return blocks;
}

/**
 * Blocks of a rendered translation. `TranslatedText` labels each block with the
 * source block index, so no text matching is needed and an annotation lands on
 * the same paragraph in every language.
 */
export function buildTranslatedBlocks(root: HTMLElement): ReaderBlock[] {
  const blocks: ReaderBlock[] = [];
  const elements = root.querySelectorAll<HTMLElement>('[data-block-index]');
  for (const el of elements) {
    const index = Number.parseInt(el.dataset.blockIndex ?? '', 10);
    if (!Number.isInteger(index) || index < 0) continue;
    const built = buildBlock(el, el.tagName.toLowerCase(), index);
    if (built) blocks.push(built);
  }
  return blocks;
}

/** The block a node lives in, if any. */
export function blockForNode(blocks: ReaderBlock[], node: Node | null): ReaderBlock | null {
  if (!node) return null;
  let el: Element | null = node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement;
  while (el) {
    const found = blocks.find((block) => block.el === el);
    if (found) return found;
    el = el.parentElement;
  }
  return null;
}

/** Ordering key for a DOM boundary inside a block; a number, not a DOM op. */
function boundaryKey(block: ReaderBlock, order: Map<Node, number>, node: Node, offset: number): number {
  if (node.nodeType === Node.TEXT_NODE) {
    const rank = order.get(node);
    return rank === undefined ? Number.POSITIVE_INFINITY : rank * 1_000_000 + offset;
  }

  if (node.nodeType === Node.ELEMENT_NODE) {
    const element = node as HTMLElement;
    if (element.tagName.toLowerCase() === 'br') {
      const rank = order.get(element);
      return rank === undefined ? Number.POSITIVE_INFINITY : rank * 1_000_000 + 1;
    }
    const children = element.childNodes;
    const child = children[offset];
    if (child) {
      if (child.nodeType === Node.TEXT_NODE) {
        const rank = order.get(child);
        if (rank !== undefined) return rank * 1_000_000 - 0.5;
      }
      const first = firstText(child, order);
      if (first !== null) return first * 1_000_000 - 0.5;
      const next = children[offset + 1];
      if (next) {
        const rank = firstText(next, order);
        if (rank !== null) return rank * 1_000_000 - 0.5;
      }
    }
    return Number.POSITIVE_INFINITY;
  }

  void block;
  return Number.POSITIVE_INFINITY;
}

function firstText(node: Node, order: Map<Node, number>): number | null {
  if (node.nodeType === Node.TEXT_NODE) return order.get(node) ?? null;
  for (const child of node.childNodes) {
    const rank = firstText(child, order);
    if (rank !== null) return rank;
  }
  return null;
}

/**
 * Normalised character offset of a DOM boundary inside a block.
 *
 * Used when the reader selects text: the browser gives a node and an offset, an
 * annotation needs a count into the block's normalised text.
 */
export function offsetAtPoint(block: ReaderBlock, node: Node, offset: number): number {
  const order = orderFor(block);
  const key = boundaryKey(block, order, node, offset);

  let count = 0;
  for (const point of block.points) {
    const endKey = pointKey(block, order, point.end);
    if (endKey <= key) count += 1;
    else break;
  }
  return count;
}

const orderCache = new WeakMap<ReaderBlock, Map<Node, number>>();
function orderFor(block: ReaderBlock): Map<Node, number> {
  const cached = orderCache.get(block);
  if (cached) return cached;
  const order = new Map<Node, number>();
  collectRaw(block.el, order, { value: 0 });
  orderCache.set(block, order);
  return order;
}

function pointKey(block: ReaderBlock, order: Map<Node, number>, point: DomPoint): number {
  if (point.element === 'before') {
    const rank = order.get(point.node);
    return rank === undefined ? Number.POSITIVE_INFINITY : rank * 1_000_000;
  }
  if (point.element === 'after') {
    const rank = order.get(point.node);
    return rank === undefined ? Number.POSITIVE_INFINITY : rank * 1_000_000 + 1;
  }
  return boundaryKey(block, order, point.node, point.offset);
}

/** A range covering characters `[start, end)` of a block's normalised text. */
export function rangeForBlockOffsets(block: ReaderBlock, start: number, end: number): Range | null {
  const from = Math.max(0, Math.min(start, block.text.length));
  const to = Math.max(0, Math.min(end, block.text.length));
  if (to <= from || block.points.length === 0) return null;

  const first = block.points[from];
  const last = block.points[to - 1];
  if (!first || !last) return null;

  const range = block.el.ownerDocument.createRange();
  try {
    setBoundary(range, true, first.start);
    setBoundary(range, false, last.end);
  } catch {
    return null;
  }
  return range.collapsed ? null : range;
}

function setBoundary(range: Range, isStart: boolean, point: DomPoint): void {
  if (point.element === 'before') {
    if (isStart) range.setStartBefore(point.node);
    else range.setEndBefore(point.node);
    return;
  }
  if (point.element === 'after') {
    if (isStart) range.setStartAfter(point.node);
    else range.setEndAfter(point.node);
    return;
  }
  if (isStart) range.setStart(point.node, point.offset);
  else range.setEnd(point.node, point.offset);
}

/**
 * Project a source-text interval onto a translation of the same block.
 *
 * A translated block is the same paragraph in another language, but its words
 * have different lengths, so there is no character-for-character mapping. The
 * proportional interval is the honest approximation: a highlight at the start of
 * a paragraph stays at the start, one in the middle stays near the middle.
 */
export function projectOffsets(
  sourceLength: number,
  translatedLength: number,
  start: number,
  end: number,
): { start: number; end: number } {
  const src = Math.max(1, sourceLength);
  const target = Math.max(0, translatedLength);
  let nextStart = Math.round((Math.max(0, start) / src) * target);
  let nextEnd = Math.round((Math.max(0, end) / src) * target);
  nextStart = Math.min(nextStart, target);
  nextEnd = Math.min(nextEnd, target);
  if (nextEnd <= nextStart) nextEnd = Math.min(target, nextStart + 1);
  return { start: nextStart, end: nextEnd };
}

/**
 * Widen a projected interval to the words it lands inside.
 *
 * Proportional projection puts a mark at roughly the right place, but a
 * character offset can fall mid-word ("रेशान" instead of "परेशान"), which reads
 * as a sloppy mark rather than a translated one. Scripts without spaces stay
 * untouched: there is no word boundary to snap to, and moving would be worse
 * than the approximation.
 */
export function snapToWords(text: string, start: number, end: number): { start: number; end: number } {
  const isWord = (char: string | undefined) => char !== undefined && /[\p{L}\p{N}]/u.test(char);
  let from = Math.max(0, Math.min(start, text.length));
  let to = Math.max(from, Math.min(end, text.length));

  if (isWord(text[from]) && isWord(text[from - 1])) {
    while (from > 0 && isWord(text[from - 1])) from -= 1;
  }
  if (isWord(text[to - 1]) && isWord(text[to])) {
    while (to < text.length && isWord(text[to])) to += 1;
  }
  return { start: from, end: to };
}

/* -------------------------------------------------------------------------- */
/* Selection → anchor                                                         */
/* -------------------------------------------------------------------------- */

export interface SelectionAnchor {
  /** The visible words, whitespace tidied. */
  text: string;
  /** Source-language words the visible selection corresponds to. */
  quote: string;
  start: { block: number; offset: number };
  end: { block: number; offset: number };
  /** DOM rect of the selection, for placing the toolbar. */
  rect: DOMRect;
}

/**
 * Read the current selection into anchors.
 *
 * `blocks` are the blocks on screen (source or translation). In a translation
 * the returned offsets address the translated block; `toSourceAnchor` converts
 * them back to the source coordinates an annotation is stored in.
 */
export function readSelection(blocks: ReaderBlock[], selection: Selection): SelectionAnchor | null {
  if (selection.rangeCount === 0 || selection.isCollapsed) return null;
  const range = selection.getRangeAt(0);
  const startBlock = blockForNode(blocks, range.startContainer);
  const endBlock = blockForNode(blocks, range.endContainer);
  if (!startBlock || !endBlock) return null;

  const rawText = selection.toString().replace(/\s+/g, ' ').trim();
  if (!rawText) return null;

  const startOffset = offsetAtPoint(startBlock, range.startContainer, range.startOffset);
  const endOffset = offsetAtPoint(endBlock, range.endContainer, range.endOffset);
  if (startBlock.serverIndex < 0 || endBlock.serverIndex < 0) return null;

  if (startBlock === endBlock && endOffset <= startOffset) return null;

  return {
    text: rawText,
    quote: rawText,
    start: { block: startBlock.serverIndex, offset: startOffset },
    end: { block: endBlock.serverIndex, offset: endOffset },
    rect: range.getBoundingClientRect(),
  };
}

/**
 * Turn a visible selection made on a translated page into a source anchor.
 *
 * `sourceBlocks` describes the original text, `visibleBlocks` the translation.
 * The block indices are the same; only the offsets are projected, because the
 * translated block has different words of a different length.
 */
export function toSourceAnchor(
  anchor: SelectionAnchor,
  sourceBlocks: EpubChapterBlock[],
  visibleBlocks: ReaderBlock[],
): SelectionAnchor {
  const visibleLength = (index: number) =>
    visibleBlocks.find((block) => block.serverIndex === index)?.text.length ??
    sourceBlocks[index]?.text.length ??
    0;

  const sourceStart = sourceBlocks[anchor.start.block]?.text ?? '';
  const sourceEnd = sourceBlocks[anchor.end.block]?.text ?? '';
  const startLength = visibleLength(anchor.start.block);
  const endLength = visibleLength(anchor.end.block);

  let start = sourceStart.length
    ? Math.round((anchor.start.offset / Math.max(1, startLength)) * sourceStart.length)
    : 0;
  let end =
    anchor.start.block === anchor.end.block
      ? Math.round((anchor.end.offset / Math.max(1, startLength)) * sourceStart.length)
      : sourceEnd.length
        ? Math.round((anchor.end.offset / Math.max(1, endLength)) * sourceEnd.length)
        : sourceEnd.length;

  if (anchor.start.block === anchor.end.block && sourceStart.length) {
    ({ start, end } = snapToWords(sourceStart, start, end));
  }

  const quoteParts: string[] = [];
  for (let index = anchor.start.block; index <= anchor.end.block; index += 1) {
    const text = sourceBlocks[index]?.text ?? '';
    if (!text) continue;
    const from = index === anchor.start.block ? start : 0;
    const to = index === anchor.end.block ? end || text.length : text.length;
    const bounded = Math.min(Math.max(to, from + 1), text.length);
    quoteParts.push(text.slice(Math.min(from, text.length), bounded));
  }

  return {
    ...anchor,
    quote: quoteParts.join('\n').trim() || anchor.text,
    start: { block: anchor.start.block, offset: start },
    end: {
      block: anchor.end.block,
      offset: anchor.start.block === anchor.end.block ? end : end || sourceEnd.length,
    },
  };
}

/* -------------------------------------------------------------------------- */
/* Client-side matching, for painting all hits in the open section            */
/* -------------------------------------------------------------------------- */

export interface TextMatch {
  start: number;
  end: number;
}

function foldText(text: string, lowercase: boolean): string {
  const folded = text.normalize('NFD').replace(/\p{M}+/gu, '');
  return lowercase ? folded.toLocaleLowerCase() : folded;
}

/** Matches of `query` in one block, used to paint a search's hits on the page. */
export function findMatches(
  text: string,
  query: string,
  options: { caseSensitive?: boolean; wholeWord?: boolean; matchDiacritics?: boolean } = {},
): TextMatch[] {
  if (!query) return [];
  const stripMarks = options.matchDiacritics !== true;
  const haystack = foldText(text, options.caseSensitive !== true);
  const needle = foldText(query, options.caseSensitive !== true);
  if (!needle) return [];

  const bounds = (index: number) => index <= 0 || index >= haystack.length || !/[\p{L}\p{N}]/u.test(haystack[index]!);
  const found: TextMatch[] = [];
  let at = 0;
  while (at <= haystack.length - needle.length) {
    const match = haystack.indexOf(needle, at);
    if (match === -1) break;
    const end = match + needle.length;
    if (!options.wholeWord || (bounds(match) && bounds(end))) {
      // With diacritics stripped, indices can drift from the original; map by
      // walking the original with the same fold and counting folded characters.
      if (stripMarks) {
        let foldedAt = 0;
        let start = 0;
        let stop = text.length;
        let started = false;
        for (let index = 0; index < text.length; index += 1) {
          const char = foldText(text[index]!, options.caseSensitive !== true);
          if (!started && foldedAt + char.length > match) {
            start = index;
            started = true;
          }
          foldedAt += char.length;
          if (started && foldedAt >= end) {
            stop = index + 1;
            break;
          }
        }
        found.push({ start, end: stop });
      } else {
        found.push({ start: match, end });
      }
    }
    at = match + Math.max(1, needle.length);
  }
  return found;
}

/** The smallest rect around a range, for placing cards beside a highlight. */
export function rectForRange(range: Range): DOMRect {
  return range.getBoundingClientRect();
}

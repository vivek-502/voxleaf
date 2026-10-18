import * as React from 'react';

import type { TranslationBlock } from '@/types';

/**
 * A translated section, rendered from its blocks.
 *
 * Deliberately not `dangerouslySetInnerHTML`, even though the text on screen is
 * the app's own sanitised markup translated into another language: a translation
 * comes back from a third-party service, and a reader is not going to audit what
 * a service did with a paragraph before reading it. As React children, a
 * translation is text — it cannot become a tag, an attribute or a script, and the
 * only thing lost is the inline emphasis the extractor flattened, which a
 * translation would have thrown away anyway.
 *
 * The blocks keep their own tags, so a chapter's shape survives the round trip:
 * its headings are still headings and its list items are still a list.
 */
export function TranslatedText({ blocks }: { blocks: TranslationBlock[] }) {
  const nodes: React.ReactNode[] = [];

  for (let index = 0; index < blocks.length; index += 1) {
    const block = blocks[index]!;

    // Consecutive list items are one list, which is what they were in the source.
    if (block.tag === 'li') {
      const first = index;
      const items: TranslationBlock[] = [];
      while (index < blocks.length && blocks[index]!.tag === 'li') {
        items.push(blocks[index]!);
        index += 1;
      }
      index -= 1; // the loop's own increment moves past the last item
      nodes.push(
        <ul key={`ul-${first}`}>
          {items.map((item, position) => (
            <li key={`li-${first}-${position}`}>{lines(item.text)}</li>
          ))}
        </ul>,
      );
      continue;
    }

    nodes.push(render(block, index));
  }

  return <>{nodes}</>;
}

/** Render one block under the tag the source used, falling back to a paragraph. */
function render(block: TranslationBlock, key: number): React.ReactNode {
  const { tag, text, html } = block;
  switch (tag) {
    case 'h1':
    case 'h2':
    case 'h3':
    case 'h4':
    case 'h5':
    case 'h6': {
      const Heading = tag as 'h1';
      return <Heading key={key}>{lines(text)}</Heading>;
    }
    case 'blockquote':
      return <blockquote key={key}>{lines(text)}</blockquote>;
    case 'figcaption':
      return <figcaption key={key}>{lines(text)}</figcaption>;
    case 'pre':
      return (
        <pre key={key}>
          <code>{text}</code>
        </pre>
      );
    case 'img': {
      if (html) {
        const { alt, ...attrs } = parseImgAttrs(html);
        // Not next/image: the src comes from the book and may be a data URI or
        // an arbitrary CDN, neither of which the optimizer can be pointed at.
        // Most EPUB figures carry no alt and their caption lives in the
        // following figcaption, so an absent alt means decorative — which stops
        // a screen reader announcing the filename in place of the figure.
        // eslint-disable-next-line @next/next/no-img-element
        return <img key={key} alt={alt ?? ''} {...attrs} />;
      }
      return <p key={key}>{lines(text)}</p>;
    }
    case 'table': {
      if (html) {
        return <table key={key} dangerouslySetInnerHTML={{ __html: html }} />;
      }
      return <p key={key}>{lines(text)}</p>;
    }
    case 'tr': {
      if (html) {
        return <tr key={key} dangerouslySetInnerHTML={{ __html: html }} />;
      }
      return <p key={key}>{lines(text)}</p>;
    }
    case 'td':
      return <td key={key}>{lines(text)}</td>;
    case 'th':
      return <th key={key}>{lines(text)}</th>;
    case 'thead':
    case 'tbody':
    case 'tfoot': {
      return React.createElement(tag, { key });
    }
    default:
      return <p key={key}>{lines(text)}</p>;
  }
}

function parseImgAttrs(html: string): React.ImgHTMLAttributes<HTMLImageElement> {
  const attrs: React.ImgHTMLAttributes<HTMLImageElement> = {};
  const srcMatch = html.match(/src=["']([^"']*)["']/i);
  if (srcMatch?.[1]) attrs.src = srcMatch[1];
  const altMatch = html.match(/alt=["']([^"']*)["']/i);
  if (altMatch?.[1]) attrs.alt = altMatch[1];
  const widthMatch = html.match(/width=["']([^"']*)["']/i);
  if (widthMatch?.[1]) attrs.width = Number(widthMatch[1]) || widthMatch[1];
  const heightMatch = html.match(/height=["']([^"']*)["']/i);
  if (heightMatch?.[1]) attrs.height = Number(heightMatch[1]) || heightMatch[1];
  return attrs;
}

/**
 * Text with its line breaks.
 *
 * The extractor keeps the line breaks a stanza or an address is made of and
 * collapses everything else, so these are real breaks rather than the ragged edge
 * of some publisher's markup.
 */
function lines(text: string): React.ReactNode {
  const parts = text.split('\n');
  if (parts.length === 1) return text;
  return parts.map((part, index) => (
    <React.Fragment key={index}>
      {index > 0 && <br />}
      {part}
    </React.Fragment>
  ));
}

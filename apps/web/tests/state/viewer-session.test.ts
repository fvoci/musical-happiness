/**
 * A deck's hidden slides in the viewer's session
 * (src/lib/file-viewer/session.svelte.ts), with a frame that only answers:
 * the deck opens on the first slide a show gives, pages are turned past the
 * slides left out, showing them keeps the reader's slide, leaving them out
 * lands on the next one, a page told before they were left out is not the
 * reader's, and only a deck has hidden slides.
 */
import { Blob as NodeBlob } from 'node:buffer';

import { afterEach, describe, expect, it, vi } from 'vitest';

import { ViewerSession } from '$lib/file-viewer/session.svelte.ts';

import { koreanPdf } from '../support/viewer-files.ts';
import { numberedPptx } from '../support/viewer-pptx.ts';

const sessions: ViewerSession[] = [];

afterEach(() => {
  for (const session of sessions.splice(0)) session.close();
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

/** A session of `file` with its frame told it is ready; what it sends, and a way to answer. */
async function opened(name: string, file: Buffer) {
  const session = new ViewerSession(
    {
      name,
      size: file.length,
      read: () => Promise.resolve(new NodeBlob([new Uint8Array(file)]) as Blob),
    },
    { theme: () => 'light', lang: () => 'en', onKey: () => undefined, onDownload: () => undefined },
  );
  sessions.push(session);
  await vi.waitFor(() => {
    expect(session.status.name).toBe('opening');
  });
  const frame = document.createElement('iframe');
  document.body.append(frame);
  const window_ = frame.contentWindow;
  if (!window_) throw new Error('no frame');
  const sent: unknown[] = [];
  vi.spyOn(window_, 'postMessage').mockImplementation((message: unknown) => {
    sent.push(message);
  });
  session.attach(frame);
  const tell = (data: unknown): void => {
    window.dispatchEvent(new MessageEvent('message', { data, source: window_ }));
  };
  tell({ kind: 'ready' });
  return { session, sent, tell };
}

describe("a deck's hidden slides in the session", () => {
  it('first and last: opened on the first a show gives, turned past them both ways', async () => {
    const { session, sent, tell } = await opened('deck.pptx', numberedPptx(6, [1, 2, 6]));
    tell({ kind: 'hidden', pages: [1, 2, 6] });
    tell({ kind: 'shown', pages: 6 });
    expect(session.status.name).toBe('shown');
    expect(session.page).toBe(3);
    expect(session.canTurn(-1)).toBe(false);
    session.turn(1);
    session.turn(1);
    expect(session.page).toBe(5);
    expect(sent.at(-1)).toEqual({ kind: 'page', page: 5 });
    expect(session.canTurn(1)).toBe(false);

    // Shown: the reader stays on 5, and may turn to 6.
    session.toggleHidden();
    expect(session.showHidden).toBe(true);
    expect(sent.at(-1)).toEqual({ kind: 'hidden', shown: true });
    expect(session.page).toBe(5);
    session.turn(1);
    expect(session.page).toBe(6);

    // Left out on 6: back on 5, the last a show gives. A page the frame told of before (6), or
    // any slide left out, is not the reader's; the frame's own landing is.
    session.toggleHidden();
    expect(sent.at(-1)).toEqual({ kind: 'hidden', shown: false });
    expect(session.page).toBe(5);
    tell({ kind: 'page', page: 6 });
    tell({ kind: 'page', page: 2 });
    expect(session.page).toBe(5);
    tell({ kind: 'page', page: 4 });
    expect(session.page).toBe(4);
  });

  it('every slide hidden: all are turned through, and there is nothing to leave out', async () => {
    const { session, sent, tell } = await opened('deck.pptx', numberedPptx(2, [1, 2]));
    tell({ kind: 'hidden', pages: [1, 2] });
    tell({ kind: 'shown', pages: 2 });
    expect(session.page).toBe(1);
    expect(session.slides?.allHidden).toBe(true);
    const before = sent.length;
    session.toggleHidden();
    expect(sent).toHaveLength(before);
    session.turn(1);
    expect(session.page).toBe(2);
  });

  it('only a deck has hidden slides; ones past its pages are none', async () => {
    const pdf = await opened('report.pdf', koreanPdf(3));
    pdf.tell({ kind: 'hidden', pages: [2] });
    pdf.tell({ kind: 'shown', pages: 3 });
    expect(pdf.session.slides).toBeUndefined();
    pdf.session.turn(1);
    expect(pdf.session.page).toBe(2);

    const deck = await opened('deck.pptx', numberedPptx(3, []));
    deck.tell({ kind: 'hidden', pages: [9] });
    deck.tell({ kind: 'shown', pages: 3 });
    expect(deck.session.slides).toBeUndefined();
    expect(deck.session.page).toBe(1);
  });
});

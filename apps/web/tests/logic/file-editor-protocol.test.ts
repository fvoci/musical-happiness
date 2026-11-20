/**
 * The messages between the app and the editor's frame are read as exactly
 * what they are: a message with a field more, a field less, or a value of
 * another kind is none.
 */
import { describe, expect, it } from 'vitest';

import {
  LOSS_COUNT_LIMIT,
  LOSS_LIST_LIMIT,
  LOSS_PATH_LIMIT,
  NAME_LIMIT,
  readFromEditor,
  readToEditor,
  REQUEST_LIMIT,
  type Loss,
} from '$lib/file-viewer/editor-protocol.ts';
import { PAGE_LIMIT } from '$lib/file-viewer/protocol.ts';

const LOSS: Loss = {
  code: 'binaryContentEmptied',
  subject: 'binaryData',
  reason: 'resourceReadFailedOrLimitExceeded',
  path: 'BinData/image1.png',
};

describe('a message from the editor’s frame, in the app', () => {
  it('is one of the protocol’s', () => {
    const bytes = new ArrayBuffer(4);
    for (const message of [
      { kind: 'ready' },
      { kind: 'opened', pages: 1 },
      { kind: 'opened', pages: PAGE_LIMIT },
      { kind: 'failed', reason: 'unreadable' },
      { kind: 'failed', reason: 'locked' },
      { kind: 'failed', reason: 'too-large' },
      { kind: 'failed', reason: 'editor' },
      { kind: 'changed', dirty: true },
      { kind: 'changed', dirty: false },
      { kind: 'save-asked' },
      { kind: 'leave' },
      { kind: 'exported', request: 1, bytes, count: 0, losses: [] },
      { kind: 'exported', request: REQUEST_LIMIT, bytes, count: 1, losses: [LOSS] },
      { kind: 'export-failed', request: 3 },
    ]) {
      expect(readFromEditor(message), JSON.stringify(message)).toEqual(message);
    }
  });

  it('carries the bytes only with a report that agrees with itself', () => {
    const bytes = new ArrayBuffer(4);
    const exported = { kind: 'exported', request: 1, bytes, count: 1, losses: [LOSS] };
    const full = Array<Loss>(LOSS_LIST_LIMIT).fill(LOSS);
    // More losses than a message lists: the count says how many, the list is cut at the limit.
    const many = { ...exported, count: LOSS_COUNT_LIMIT, losses: full };
    expect(readFromEditor(many)).toEqual(many);
    for (const wrong of [
      { kind: 'exported', request: 1, bytes },
      { kind: 'exported', request: 1, bytes, count: 0 },
      { kind: 'exported', request: 1, count: 0, losses: [] },
      { ...exported, count: 0 },
      { ...exported, count: 2 },
      { ...exported, count: -1, losses: [] },
      { ...exported, count: 1.5 },
      { ...exported, count: LOSS_COUNT_LIMIT + 1, losses: full },
      { ...exported, count: LOSS_LIST_LIMIT + 1, losses: [...full, LOSS] },
      { ...exported, count: LOSS_LIST_LIMIT + 1, losses: full.slice(1) },
      { ...exported, bytes: new Uint8Array(4) },
      { ...exported, bytes: 'AAAA' },
      { ...exported, request: 0 },
      { ...exported, request: REQUEST_LIMIT + 1 },
      { ...exported, request: '1' },
      { ...exported, losses: [{ ...LOSS, code: 'somethingNew' }] },
      { ...exported, losses: [{ ...LOSS, subject: 'page' }] },
      { ...exported, losses: [{ ...LOSS, reason: 'because' }] },
      { ...exported, losses: [{ ...LOSS, path: 'p'.repeat(LOSS_PATH_LIMIT + 1) }] },
      { ...exported, losses: [{ ...LOSS, path: 7 }] },
      { ...exported, losses: [{ ...LOSS, resourceId: 1 }] },
      { ...exported, losses: [null] },
      { ...exported, losses: 'none' },
      { ...exported, saved: true },
    ]) {
      expect(readFromEditor(wrong), JSON.stringify(wrong).slice(0, 120)).toBeUndefined();
    }
  });

  it('is nothing else', () => {
    for (const wrong of [
      undefined,
      null,
      'ready',
      7,
      [],
      [{ kind: 'ready' }],
      {},
      { kind: 'ready', more: 1 },
      { kind: 'opened' },
      { kind: 'opened', pages: 0 },
      { kind: 'opened', pages: PAGE_LIMIT + 1 },
      { kind: 'opened', pages: '3' },
      { kind: 'failed', reason: 'renderer' },
      { kind: 'failed', reason: 'slow' },
      { kind: 'failed' },
      { kind: 'changed', dirty: 1 },
      { kind: 'changed' },
      { kind: 'save-asked', as: 'copy' },
      { kind: 'leave', to: 'https://example.invalid/' },
      { kind: 'export-failed' },
      { kind: 'export-failed', request: 0 },
      // What the app says to the frame is not what the frame says to the app.
      { kind: 'export', request: 1 },
      { kind: 'stored', request: 1 },
      { kind: 'navigate', url: 'https://example.invalid/' },
      { kind: 'download', name: 'a.hwp' },
      new (class Ready {
        kind = 'ready';
      })(),
    ]) {
      expect(readFromEditor(wrong), JSON.stringify(wrong)).toBeUndefined();
    }
  });
});

describe('a message from the app, in the editor’s frame', () => {
  const open = {
    kind: 'open',
    format: 'hwpx',
    name: '회의록.hwpx',
    bytes: new ArrayBuffer(4),
    engine: new ArrayBuffer(8),
    theme: 'dark',
    lang: 'ko',
  };

  it('opens a Hangul document with rhwp’s engine, asks for it, or says it is stored', () => {
    expect(readToEditor(open)).toEqual(open);
    expect(readToEditor({ ...open, format: 'hwp', theme: 'light', lang: 'en-US' })).toEqual({
      ...open,
      format: 'hwp',
      theme: 'light',
      lang: 'en-US',
    });
    const longest = { ...open, name: 'n'.repeat(NAME_LIMIT) };
    expect(readToEditor(longest)).toEqual(longest);
    expect(readToEditor({ kind: 'export', request: 1 })).toEqual({ kind: 'export', request: 1 });
    expect(readToEditor({ kind: 'stored', request: 9 })).toEqual({ kind: 'stored', request: 9 });
  });

  it('is nothing else', () => {
    const { engine: _engine, ...withoutEngine } = open;
    for (const wrong of [
      undefined,
      null,
      'open',
      withoutEngine,
      { ...open, format: 'pdf' },
      { ...open, format: 'docx' },
      { ...open, name: '' },
      { ...open, name: 'n'.repeat(NAME_LIMIT + 1) },
      { ...open, name: 7 },
      { ...open, bytes: new Uint8Array(4) },
      { ...open, engine: null },
      { ...open, engine: new Uint8Array(8) },
      { ...open, theme: 'sepia' },
      { ...open, lang: 'ko"><script>' },
      { ...open, lang: '' },
      { ...open, labels: {} },
      { kind: 'export' },
      { kind: 'export', request: 0 },
      { kind: 'export', request: 1, format: 'hwp' },
      { kind: 'stored', request: REQUEST_LIMIT + 1 },
      { kind: 'ready' },
      { kind: 'exported', request: 1, bytes: new ArrayBuffer(1), count: 0, losses: [] },
    ]) {
      expect(readToEditor(wrong), JSON.stringify(wrong ?? null).slice(0, 120)).toBeUndefined();
    }
  });
});

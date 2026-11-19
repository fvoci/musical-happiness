/**
 * The saving of an edited document (lib/file-viewer/editing.svelte.ts), with
 * a frame and a file's versions stood in for: nothing is stored that is not a
 * document of the format opened, nothing that loses something without the
 * writer's choice, and nothing over a newer version of the file.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  EditingSession,
  editorAddress,
  type EditedFile,
  type EditLimits,
} from '$lib/file-viewer/editing.svelte.ts';
import type { FromEditor, Loss, ToEditor } from '$lib/file-viewer/editor-protocol.ts';
import type {
  FileVersion,
  VersionsSource,
  VersionUpload,
  WriteRefusal,
} from '$lib/file-viewer/versions.svelte.ts';

import { koreanHwp, koreanHwpx } from '../support/hwp-files.ts';
import { koreanPdf } from '../support/viewer-files.ts';

const LIMITS: EditLimits = {
  bytes: 1 << 20,
  ready: 1_000,
  open: 2_000,
  export: 3_000,
  engine: 4_000,
};

const LOSS: Loss = {
  code: 'binaryContentEmptied',
  subject: 'binaryData',
  reason: 'resourceReadFailedOrLimitExceeded',
  path: 'BinData/image1.png',
};

const bufferOf = (data: Uint8Array): ArrayBuffer => {
  const copy = new ArrayBuffer(data.byteLength);
  new Uint8Array(copy).set(data);
  return copy;
};

const HWPX = koreanHwpx();
const HWP = koreanHwp();

class Stale extends Error {}

/** A file's versions: what was uploaded, written and attached, and what the next write comes to. */
function versions() {
  const made = {
    uploads: [] as File[],
    added: [] as { base: number; upload: VersionUpload }[],
    apart: [] as VersionUpload[],
    /** The file's newest version. */
    head: 4,
    uploading: true,
    refusing: false,
  };
  const source: VersionsSource = {
    canWrite: true,
    list: () => Promise.resolve([]),
    bytes: () => Promise.resolve(new Blob()),
    upload: (file): Promise<VersionUpload | undefined> => {
      made.uploads.push(file);
      if (!made.uploading) return Promise.resolve(undefined);
      const fileId = `upload-${String(made.uploads.length)}`;
      return Promise.resolve({ fileId, name: file.name, size: file.size, contentType: file.type });
    },
    add: (base, upload) => {
      made.added.push({ base, upload });
      if (made.refusing) return Promise.reject(new Error('refused'));
      if (base !== made.head) return Promise.reject(new Stale());
      made.head += 1;
      return Promise.resolve({ version: made.head } as FileVersion);
    },
    restore: () => Promise.reject(new Error('not asked here')),
    attachApart: (upload) => {
      made.apart.push(upload);
      return made.refusing ? Promise.reject(new Error('refused')) : Promise.resolve();
    },
    refusalOf: (error): WriteRefusal => (error instanceof Stale ? 'stale' : 'failed'),
    userName: () => undefined,
    learnUsers: () => Promise.resolve(),
  };
  return { made, source };
}

/** A session on a frame in the page, with what it sent the frame and a way to answer as the frame. */
function editing(
  file: Partial<EditedFile> = {},
  engine = () => Promise.resolve(new ArrayBuffer(8)),
) {
  const left = vi.fn();
  const { made, source } = versions();
  const edited: EditedFile = {
    name: '회의록.hwpx',
    format: 'hwpx',
    bytes: bufferOf(HWPX),
    version: 4,
    ...file,
  };
  const session = new EditingSession(edited, source, {
    theme: () => 'dark',
    lang: () => 'ko',
    apartName: (name) => `copy of ${name}`,
    limits: LIMITS,
    engine,
    onLeave: left,
  });
  const frame = document.createElement('iframe');
  document.body.append(frame);
  const sent: { message: ToEditor; transfer: unknown[] }[] = [];
  const inside = frame.contentWindow;
  if (!inside) throw new Error('the frame has no window');
  vi.spyOn(inside, 'postMessage').mockImplementation(((message: ToEditor, _to, transfer) => {
    sent.push({ message, transfer: (transfer as unknown[] | undefined) ?? [] });
  }) as Window['postMessage']);
  const detach = session.attach(frame);
  const says = (data: FromEditor | Record<string, unknown>, source: Window | null = inside) => {
    window.dispatchEvent(new MessageEvent('message', { data, source }));
  };
  const settled = async (): Promise<void> => {
    await vi.advanceTimersByTimeAsync(0);
  };
  /** The document open, as far as the frame says. */
  const opened = async (): Promise<void> => {
    says({ kind: 'ready' });
    await settled();
    says({ kind: 'opened', pages: 2 });
    await settled();
  };
  /** The frame's answer to the last request to export. */
  const exports = (data: Uint8Array, losses: Loss[] = [], request = sent.length): void => {
    const asked = sent.findLast(({ message }) => message.kind === 'export')?.message;
    if (asked?.kind !== 'export') throw new Error(`no export was asked (${String(request)})`);
    says({
      kind: 'exported',
      request: asked.request,
      bytes: bufferOf(data),
      count: losses.length,
      losses,
    });
  };
  const kinds = (): string[] => sent.map(({ message }) => message.kind);
  return {
    session,
    made,
    frame,
    sent,
    says,
    settled,
    opened,
    exports,
    kinds,
    detach,
    edited,
    left,
  };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

describe('a document handed to the editor’s frame', () => {
  it('goes with rhwp’s engine once the frame is ready, both handed over, and opens', async () => {
    const { session, sent, says, settled, edited } = editing();
    expect(session.status).toEqual({ name: 'opening' });
    expect(sent).toEqual([]);
    says({ kind: 'ready' });
    await settled();
    expect(sent).toHaveLength(1);
    const [{ message, transfer }] = sent as [(typeof sent)[number]];
    expect(message).toMatchObject({
      kind: 'open',
      format: 'hwpx',
      name: '회의록.hwpx',
      theme: 'dark',
      lang: 'ko',
    });
    if (message.kind !== 'open') throw new Error('not opened');
    expect(message.bytes).toBe(edited.bytes);
    expect(transfer).toEqual([message.bytes, message.engine]);
    // A second "ready" hands nothing over again.
    says({ kind: 'ready' });
    await settled();
    expect(sent).toHaveLength(1);
    says({ kind: 'opened', pages: 2 });
    expect(session.status).toEqual({ name: 'ready', pages: 2 });
    says({ kind: 'changed', dirty: true });
    expect(session.dirty).toBe(true);
  });

  it('is not handed over when the engine does not come', async () => {
    const { session, sent, says, settled } = editing({}, () => Promise.reject(new Error('gone')));
    says({ kind: 'ready' });
    await settled();
    expect(session.status).toEqual({ name: 'failed', reason: 'editor' });
    expect(sent).toEqual([]);
  });

  it('is given up when the frame, the engine or the opening takes too long', async () => {
    const never = editing();
    await vi.advanceTimersByTimeAsync(LIMITS.ready);
    expect(never.session.status).toEqual({ name: 'failed', reason: 'slow' });

    const waiting = editing({}, () => new Promise<ArrayBuffer>(() => undefined));
    waiting.says({ kind: 'ready' });
    await vi.advanceTimersByTimeAsync(LIMITS.engine - 1);
    expect(waiting.session.status).toEqual({ name: 'opening' });
    await vi.advanceTimersByTimeAsync(1);
    expect(waiting.session.status).toEqual({ name: 'failed', reason: 'slow' });
    expect(waiting.sent).toEqual([]);

    const slow = editing();
    slow.says({ kind: 'ready' });
    await slow.settled();
    await vi.advanceTimersByTimeAsync(LIMITS.open);
    expect(slow.session.status).toEqual({ name: 'failed', reason: 'slow' });
    // What a failed frame says afterwards is not heard.
    slow.says({ kind: 'opened', pages: 2 });
    expect(slow.session.status).toEqual({ name: 'failed', reason: 'slow' });
  });

  it('fails as the frame says, and when the frame loads another document', async () => {
    const refused = editing();
    refused.says({ kind: 'ready' });
    await refused.settled();
    refused.says({ kind: 'failed', reason: 'locked' });
    expect(refused.session.status).toEqual({ name: 'failed', reason: 'locked' });

    const away = editing();
    await away.opened();
    away.frame.dispatchEvent(new Event('load'));
    expect(away.session.status.name).toBe('ready');
    away.frame.dispatchEvent(new Event('load'));
    expect(away.session.status).toEqual({ name: 'failed', reason: 'editor' });
  });

  it('hears only its own frame, and only the protocol’s messages', async () => {
    const { session, says, settled, sent, detach } = editing();
    says({ kind: 'ready' }, window);
    says({ kind: 'ready' }, null);
    says({ kind: 'ready', more: true });
    await settled();
    expect(sent).toEqual([]);
    detach();
    says({ kind: 'ready' });
    await settled();
    expect(sent).toEqual([]);
    expect(session.status).toEqual({ name: 'opening' });
  });

  it('lets the writer out to the app’s controls when the frame says they asked (F6)', async () => {
    const { opened, says, left } = editing();
    await opened();
    expect(left).not.toHaveBeenCalled();
    says({ kind: 'leave' });
    expect(left).toHaveBeenCalledOnce();
  });

  it('is at the editor’s address, with the profile of a host that saves', () => {
    expect(editorAddress(document)).toMatch(/\/editor-frame\.html\?chrome=embed$/);
  });
});

describe('saving what was edited', () => {
  it('makes it the file’s new version, written from the one opened, and then from that', async () => {
    const { session, made, opened, exports, settled, kinds, says, frame } = editing();
    session.save();
    expect(kinds()).toEqual([]);
    await opened();
    says({ kind: 'changed', dirty: true });
    const focus = vi.spyOn(frame, 'focus');
    says({ kind: 'save-asked' });
    expect(session.saving).toEqual({ name: 'exporting' });
    expect(session.busy).toBe(true);
    // Asked once: a second request while the first is under way is not sent.
    session.save();
    expect(kinds()).toEqual(['open', 'export']);
    exports(HWPX);
    await settled();
    expect(made.uploads).toHaveLength(1);
    expect(made.uploads[0]?.name).toBe('회의록.hwpx');
    expect(made.uploads[0]?.type).toBe('application/hwp+zip');
    expect(made.uploads[0]?.size).toBe(HWPX.length);
    expect(made.added.map(({ base }) => base)).toEqual([4]);
    expect(session.saved).toMatchObject({ as: 'version', version: { version: 5 } });
    expect(session.saving).toEqual({ name: 'idle' });
    expect(kinds()).toEqual(['open', 'export', 'stored']);
    expect(focus).toHaveBeenCalled();

    session.save();
    exports(HWPX);
    await settled();
    expect(made.added.map(({ base }) => base)).toEqual([4, 5]);
    expect(session.saved).toMatchObject({ as: 'version', version: { version: 6 } });
  });

  it('saves an HWP as an HWP', async () => {
    const { session, made, opened, exports, settled } = editing({
      name: '회의록.hwp',
      format: 'hwp',
      bytes: bufferOf(HWP),
    });
    await opened();
    session.save();
    exports(HWP);
    await settled();
    expect(made.uploads[0]?.type).toBe('application/x-hwp');
    expect(session.saved).toMatchObject({ as: 'version' });
  });

  it.each([
    ['a PDF', koreanPdf(1), 'not-a-document'],
    ['an HWP where an HWPX was opened', HWP, 'not-a-document'],
    ['nothing', new Uint8Array(0), 'not-a-document'],
    ['bytes of no kind', new Uint8Array(64).fill(7), 'not-a-document'],
    ['more than is saved', new Uint8Array(LIMITS.bytes + 1), 'too-large'],
  ])('stores nothing when the frame hands back %s', async (_, data, reason) => {
    const { session, made, opened, exports, settled, kinds } = editing();
    await opened();
    session.save();
    exports(data);
    await settled();
    expect(session.saving).toEqual({ name: 'failed', reason });
    expect(made.uploads).toEqual([]);
    expect(session.saved).toBeUndefined();
    expect(kinds()).not.toContain('stored');
    session.dismiss();
    expect(session.saving).toEqual({ name: 'idle' });
  });

  it('stores nothing when the export fails, does not come, or is not the one asked for', async () => {
    const { session, made, opened, says, settled, sent } = editing();
    await opened();
    session.save();
    says({ kind: 'export-failed', request: 7 });
    expect(session.saving).toEqual({ name: 'exporting' });
    says({ kind: 'exported', request: 7, bytes: bufferOf(HWPX), count: 0, losses: [] });
    await settled();
    expect(made.uploads).toEqual([]);
    says({ kind: 'export-failed', request: 1 });
    expect(session.saving).toEqual({ name: 'failed', reason: 'export' });

    session.dismiss();
    session.save();
    expect(sent.at(-1)?.message).toEqual({ kind: 'export', request: 2 });
    await vi.advanceTimersByTimeAsync(LIMITS.export);
    expect(session.saving).toEqual({ name: 'failed', reason: 'export' });
    // The answer to a request given up, or to the one before, stores nothing.
    says({ kind: 'exported', request: 2, bytes: bufferOf(HWPX), count: 0, losses: [] });
    says({ kind: 'exported', request: 1, bytes: bufferOf(HWPX), count: 0, losses: [] });
    await settled();
    expect(made.uploads).toEqual([]);
  });

  it('says so when the upload or the writing is refused', async () => {
    const up = editing();
    await up.opened();
    up.made.uploading = false;
    up.session.save();
    up.exports(HWPX);
    await up.settled();
    expect(up.session.saving).toEqual({ name: 'failed', reason: 'upload' });
    expect(up.made.added).toEqual([]);

    const refused = editing();
    await refused.opened();
    refused.made.refusing = true;
    refused.session.save();
    refused.exports(HWPX);
    await refused.settled();
    expect(refused.session.saving).toEqual({ name: 'failed', reason: 'refused' });
    expect(refused.kinds()).not.toContain('stored');
  });
});

describe('a save that loses something of the document', () => {
  it('stores nothing until the writer chose, and nothing when they chose neither', async () => {
    const { session, made, opened, exports, settled, kinds } = editing();
    await opened();
    session.save();
    exports(HWPX, [LOSS]);
    await settled();
    expect(session.saving).toMatchObject({ name: 'lossy', count: 1, losses: [LOSS] });
    expect(session.busy).toBe(true);
    expect(made.uploads).toEqual([]);
    session.dismiss();
    await settled();
    expect(session.saving).toEqual({ name: 'idle' });
    expect(made.uploads).toEqual([]);
    expect(kinds()).not.toContain('stored');
  });

  it('becomes a new version when the writer says so all the same', async () => {
    const { session, made, opened, exports, settled } = editing();
    await opened();
    session.save();
    exports(HWPX, [LOSS]);
    session.saveWithLosses();
    await settled();
    expect(made.added.map(({ base }) => base)).toEqual([4]);
    expect(made.apart).toEqual([]);
    expect(session.saved).toMatchObject({ as: 'version', version: { version: 5 } });
  });

  it('becomes a file of its own, the edited file left as it is', async () => {
    const { session, made, opened, exports, settled, kinds } = editing();
    await opened();
    session.save();
    exports(HWPX, [LOSS]);
    session.saveApart();
    await settled();
    expect(made.added).toEqual([]);
    // It went up under its own name, and is attached under it.
    expect(made.uploads.map((file) => file.name)).toEqual(['copy of 회의록.hwpx']);
    expect(made.uploads[0]?.type).toBe('application/hwp+zip');
    expect(made.apart).toMatchObject([{ name: 'copy of 회의록.hwpx', fileId: 'upload-1' }]);
    expect(session.saved).toEqual({ as: 'apart', name: 'copy of 회의록.hwpx' });
    expect(kinds().at(-1)).toBe('stored');
  });
});

describe('a file that got a newer version while it was edited', () => {
  it('is not written over: the writer is asked, and the edit kept as a file of its own', async () => {
    const { session, made, opened, exports, settled, kinds } = editing();
    await opened();
    made.head = 5;
    session.save();
    exports(HWPX);
    await settled();
    expect(session.saving).toMatchObject({ name: 'stale', upload: { fileId: 'upload-1' } });
    expect(made.added.map(({ base }) => base)).toEqual([4]);
    expect(made.head).toBe(5);
    expect(session.saved).toBeUndefined();
    expect(kinds()).not.toContain('stored');

    session.saveApart();
    await settled();
    // What was uploaded for the version is the file's: nothing is uploaded again.
    expect(made.uploads).toHaveLength(1);
    expect(made.apart).toMatchObject([{ name: 'copy of 회의록.hwpx', fileId: 'upload-1' }]);
    expect(session.saved).toEqual({ as: 'apart', name: 'copy of 회의록.hwpx' });
    expect(session.saving).toEqual({ name: 'idle' });
  });

  it('is left as it is when the writer chose not to keep the edit', async () => {
    const { session, made, opened, exports, settled } = editing();
    await opened();
    made.head = 9;
    session.save();
    exports(HWPX);
    await settled();
    session.dismiss();
    expect(session.saving).toEqual({ name: 'idle' });
    expect(made.apart).toEqual([]);
    expect(made.head).toBe(9);
  });

  it('keeps the edit as a file of its own when the editor was closed meanwhile', async () => {
    const { session, made, opened, exports, settled } = editing();
    await opened();
    made.head = 5;
    session.save();
    exports(HWPX);
    session.close();
    await settled();
    expect(made.apart).toMatchObject([{ name: 'copy of 회의록.hwpx' }]);
    expect(session.saved).toEqual({ as: 'apart', name: 'copy of 회의록.hwpx' });
  });

  it('says so when the file of its own is refused too', async () => {
    const { session, made, opened, exports, settled } = editing();
    await opened();
    made.head = 5;
    session.save();
    exports(HWPX);
    await settled();
    made.refusing = true;
    session.saveApart();
    await settled();
    expect(session.saving).toEqual({ name: 'failed', reason: 'refused' });
    expect(session.saved).toBeUndefined();
  });
});

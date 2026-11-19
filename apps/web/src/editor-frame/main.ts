/**
 * The program of the editor's frame (`static/editor-frame.html`).
 *
 * It runs as the viewer's frame does: in a document without an origin
 * (`sandbox="allow-scripts"`, and the server sends the same sandbox in a
 * header) whose policy keeps it off the network. It waits for its parent to
 * hand it a document, has the viewer's worker check the bytes as the viewer
 * checks them, and only then starts the editor, rhwp-studio (the package
 * `@fvoci/rhwp-studio`, built into this frame's bundle), on the checked bytes.
 *
 * The editor is rhwp's own program, written for a page of its own. Here it
 * gets its page's markup put into this document, its settings from a store
 * in memory (the frame has no storage), the engine's bytes as the app handed
 * them over (rhwp.ts), and the host's profile (`?chrome=embed` in the frame's address):
 * it opens, saves and prints nothing itself. Saving is the app's: when asked,
 * the frame answers with the document's bytes together with rhwp's report of
 * what they lose, in one message (`$lib/file-viewer/editor-protocol.ts`).
 *
 * It reads messages from its parent only, and only in the protocol's shapes.
 */
import './frame.css';

import {
  readToEditor,
  type EditFailure,
  type EditFormat,
  type FromEditor,
  type ToEditor,
} from '$lib/file-viewer/editor-protocol.ts';

import type { FromWorker, ToWorker } from '../viewer-frame/hwp-messages.ts';
import HwpWorker from '../viewer-frame/hwp-worker.ts?worker&inline';
import { WorkerClient } from '../viewer-frame/worker-client.ts';

import { lossesOf } from './losses.ts';
import { provide } from './rhwp.ts';
import { memoryStorage } from './storage.ts';

/** The Korean family the documents fall back to, as in the viewer's frame. */
import.meta.glob('../styles/fonts/faces-noto-sans-kr.css', { eager: true });

/** How long the check of the bytes may take, in milliseconds: the viewer's time for its worker. */
const CHECK_LIMIT = 30_000;

/** What the editor's page exposes for its host (our change in rhwp-studio's main.ts). */
interface Host {
  ready(): Promise<void>;
  load(data: Uint8Array, fileName: string): Promise<{ pageCount: number }>;
  exportWithReport(format: EditFormat): { bytes: Uint8Array; contentLoss: unknown };
  onDirtyChanged(listener: (dirty: boolean) => void): () => void;
  /** Called on every change to the document. */
  onMutated(listener: () => void): () => void;
  /** Puts the focus where the document is typed into. */
  focus(): void;
  notifySaved(): Promise<unknown>;
}

function tell(message: FromEditor, transfer: Transferable[] = []): void {
  // The parent's origin is not known here (ours is opaque); the parent checks
  // that a message came from this frame.
  window.parent.postMessage(message, '*', transfer);
}

/** The bytes as rhwp may read them, checked in the viewer's worker; the reason when they are refused. */
async function checked(format: EditFormat, bytes: ArrayBuffer): Promise<ArrayBuffer | EditFailure> {
  const client = new WorkerClient(new HwpWorker(), CHECK_LIMIT);
  try {
    const message: ToWorker = { kind: 'check', format, bytes };
    const answer = (await client.request(message, [bytes])) as FromWorker;
    if (answer.kind === 'checked') return answer.bytes;
    return answer.kind === 'failed' && answer.reason !== 'renderer' ? answer.reason : 'editor';
  } catch {
    return 'editor';
  } finally {
    client.close();
  }
}

/** The markup of the editor's page, as its own document has it in its body; its script left out. */
function pageMarkup(page: string): string {
  const body = /<body[^>]*>([\s\S]*)<\/body>/i.exec(page)?.[1];
  if (body === undefined) throw new Error("the editor's page has no body");
  return body.replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '');
}

/**
 * Starts the editor in this document: its settings (the app's theme and
 * language) in a store in memory, its page's markup, its style sheet, and
 * then its program, which finds the page as its own document would give it.
 */
async function startEditor(theme: string, lang: string): Promise<Host> {
  const settings = memoryStorage({
    'rhwp-settings': JSON.stringify({ theme: { mode: theme } }),
    'rhwp-locale': lang.toLowerCase().startsWith('ko') ? 'ko' : 'en',
  });
  // The frame has no storage (reading `localStorage` throws) and no database.
  for (const name of ['localStorage', 'sessionStorage'] as const) {
    Object.defineProperty(window, name, { configurable: true, value: settings });
  }
  const [{ default: page }] = await Promise.all([
    import('rhwp-studio:page'),
    import('rhwp-studio:style'),
  ]);
  document.body.innerHTML = pageMarkup(page);
  await import('rhwp-studio:theme-init');
  await import('rhwp-studio:locale-init');
  await import('rhwp-studio:main');
  const host = (window as unknown as { rhwpStudio?: { host?: Host } }).rhwpStudio?.host;
  if (!host) throw new Error('the editor has no hook for its host');
  await host.ready();
  return host;
}

/** The editor at work on one document: answers the app's requests to export and to mark it stored. */
function serve(host: Host, format: EditFormat): (message: ToEditor) => void {
  host.onDirtyChanged((dirty) => {
    tell({ kind: 'changed', dirty });
  });
  // How many changes the document has had, and how many it had at each export: what was
  // exported is the document only while nothing changed since.
  let changes = 0;
  const exportedAt = new Map<number, number>();
  host.onMutated(() => {
    changes += 1;
  });
  // When the app gives the frame the focus (as the document opens, and after a save asked with
  // the app's button), what is typed goes to the document again.
  window.addEventListener('focus', () => {
    if (document.activeElement === document.body) host.focus();
  });
  if (document.hasFocus()) host.focus();
  // Ctrl+S (Cmd+S) is the app's to answer. The editor's host profile swallows the key itself.
  document.addEventListener(
    'keydown',
    (event) => {
      const key = event.key.toLowerCase();
      if ((event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey) {
        if (key === 's' || key === 'ㄴ') tell({ kind: 'save-asked' });
      }
    },
    true,
  );
  return (message) => {
    if (message.kind === 'export') {
      const { request } = message;
      try {
        const exported = host.exportWithReport(format);
        const { count, losses } = lossesOf(exported.contentLoss);
        const bytes = exported.bytes.slice().buffer;
        exportedAt.set(request, changes);
        tell({ kind: 'exported', request, bytes, count, losses }, [bytes]);
      } catch {
        tell({ kind: 'export-failed', request });
      }
    } else if (message.kind === 'stored' && exportedAt.get(message.request) === changes) {
      // Only what was exported is stored: a change made since keeps the document changed.
      void host.notifySaved().catch(() => undefined);
    }
  };
}

/** Keeps a document's links and dropped files from taking the frame away or into the editor. */
function holdNavigation(): void {
  const holdLink = (event: MouseEvent): void => {
    const link = event.target instanceof Element ? event.target.closest('a, area') : null;
    if (link && !(link.getAttribute('href') ?? '').startsWith('#')) event.preventDefault();
  };
  document.addEventListener('click', holdLink, true);
  document.addEventListener('auxclick', holdLink, true);
  // A file dropped on the editor would be opened in place of the document being edited.
  for (const type of ['dragover', 'drop'] as const) {
    document.addEventListener(
      type,
      (event) => {
        if (!event.dataTransfer?.types.includes('Files')) return;
        event.preventDefault();
        event.stopImmediatePropagation();
      },
      true,
    );
  }
}

function listen(): void {
  let serving: ((message: ToEditor) => void) | undefined;
  let opening = false;
  window.addEventListener('message', (event) => {
    if (event.source !== window.parent) return;
    const message = readToEditor(event.data);
    if (!message) return;
    if (message.kind !== 'open') {
      serving?.(message);
      return;
    }
    if (opening) return;
    opening = true;
    const { format, name, bytes, engine, theme, lang } = message;
    void (async () => {
      const file = await checked(format, bytes);
      if (typeof file === 'string') {
        tell({ kind: 'failed', reason: file });
        return;
      }
      try {
        provide(engine);
        const host = await startEditor(theme, lang);
        const { pageCount } = await host.load(new Uint8Array(file), name);
        serving = serve(host, format);
        tell({ kind: 'opened', pages: pageCount });
      } catch {
        tell({ kind: 'failed', reason: 'editor' });
      }
    })();
  });
  tell({ kind: 'ready' });
}

// Only in a document without an origin: opened as a page of its own, with the
// app's origin, the editor would run beside the app's session.
if (self.origin === 'null') {
  holdNavigation();
  listen();
}

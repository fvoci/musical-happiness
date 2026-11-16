/**
 * The program of the viewer's frame (`static/viewer-frame.html`).
 *
 * It runs in a document without an origin (`sandbox="allow-scripts"`, and
 * the server sends the same sandbox in a header) whose policy keeps it off
 * the network. It waits for its parent to hand it a file, draws it with the
 * renderer of its kind, and reports back through the messages of
 * `$lib/file-viewer/protocol.ts`. It reads messages from its parent only, and
 * only in those shapes.
 */
import './frame.css';

import {
  readToFrame,
  VIEWER_KEYS,
  type FromFrame,
  type Theme,
  type ViewerKey,
} from '$lib/file-viewer/protocol.ts';

import { LockedFile, UnreadableFile, type Drawn, type Report } from './renderer.ts';
import { RENDERERS } from './renderers.ts';

/** The font rules the documents fall back to (lib/fonts.ts loads the same files in the app). */
import.meta.glob(
  ['../styles/fonts/faces-noto-sans.css', '../styles/fonts/faces-noto-sans-kr.css'],
  {
    eager: true,
  },
);

function tell(message: FromFrame): void {
  // The parent's origin is not known here (ours is opaque), and nothing sent
  // is secret: the parent checks that a message came from this frame.
  window.parent.postMessage(message, '*');
}

function setTheme(theme: Theme): void {
  document.documentElement.classList.toggle('dark', theme === 'dark');
  document.documentElement.style.colorScheme = theme;
}

const keys = new Set<string>(VIEWER_KEYS);

function forwardKeys(): void {
  document.addEventListener('keydown', (event) => {
    if (event.ctrlKey || event.metaKey || event.altKey || !keys.has(event.key)) return;
    event.preventDefault();
    tell({ kind: 'key', key: event.key as ViewerKey });
  });
}

const report: Report = {
  shown: (pages) => {
    tell({ kind: 'shown', pages });
  },
  page: (page) => {
    tell({ kind: 'page', page });
  },
};

function start(): void {
  const place = document.getElementById('document');
  if (!place) return;
  let drawn: Drawn | undefined;
  let opened = false;

  window.addEventListener('message', (event) => {
    if (event.source !== window.parent) return;
    const message = readToFrame(event.data);
    if (!message) return;
    switch (message.kind) {
      case 'open': {
        // One file per frame: the viewer makes a new frame for the next one.
        if (opened) return;
        opened = true;
        setTheme(message.theme);
        RENDERERS[message.format]()
          .then((render) => render(message.bytes, place, report))
          .then((result) => {
            drawn = result;
          })
          .catch((error: unknown) => {
            place.replaceChildren();
            tell({
              kind: 'failed',
              reason:
                error instanceof LockedFile
                  ? 'locked'
                  : error instanceof UnreadableFile
                    ? 'unreadable'
                    : 'renderer',
            });
          });
        break;
      }
      case 'page':
        drawn?.goTo(message.page);
        break;
      case 'zoom':
        drawn?.zoom(message.zoom);
        break;
      case 'theme':
        setTheme(message.theme);
        break;
    }
  });
  forwardKeys();
  tell({ kind: 'ready' });
}

// A document of the app's own origin would be opened as a page of the app,
// its storage and cookies within reach: this program runs sandboxed or not at all.
if (self.origin === 'null') start();

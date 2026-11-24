// Shared harness for the navigation measurements (spike: not run by CI). Measurement only: nothing
// here changes the app. It drives a server started on a seeded data directory (`RUN`, made by the
// seed script: summary.json and passwords.tsv), through a proxy that adds round-trip time.
//   RUN=<seeded run dir> SERVER_PORT=<server> PROXY_PORT=<proxy> OUT=<dir for the JSON> [PW_WS=<WebKit run-server>]
/* eslint-disable -- a measurement script run by node (not by CI): its in-page code reads what the harness puts on window. */
// @ts-nocheck: the same.
import fs from 'node:fs';
import net from 'node:net';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
export const { chromium, webkit, devices } = require('@playwright/test');

const need = (name) => {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set`);
  return value;
};
export const RUN = need('RUN');
export const OUT = need('OUT');
export const SERVER_PORT = Number(process.env.SERVER_PORT ?? 20610);
export const PROXY_PORT = Number(process.env.PROXY_PORT ?? 20611);
export const BASE = `http://127.0.0.1:${PROXY_PORT}`;
export const summary = JSON.parse(fs.readFileSync(`${RUN}/summary.json`, 'utf8'));
const accounts = fs
  .readFileSync(`${RUN}/passwords.tsv`, 'utf8')
  .split('\n')
  .filter((line) => line && !line.startsWith('#'))
  .map((line) => line.split('\t'));
export const password = (user) => accounts.find((cols) => cols[0] === user)[2];
/** The seed's first account: the one every measurement signs in as. */
export const FIRST_USER = accounts[0][0];

export const loadavg = () => fs.readFileSync('/proc/loadavg', 'utf8').trim();

/**
 * A TCP proxy that holds every chunk RTT/2 in each direction: HTTP and WebSocket alike,
 * for Chromium and WebKit alike. `state.rtt` is read per chunk; change it only between runs.
 */
export function delayProxy(listenPort = PROXY_PORT, targetPort = SERVER_PORT) {
  const state = { rtt: 0, connections: 0, maxOpen: 0, open: 0 };
  const sockets = new Set();
  const pipe = (from, to) => {
    from.on('data', (chunk) => {
      const half = state.rtt / 2;
      if (half <= 0) to.write(chunk);
      else setTimeout(() => to.destroyed || to.write(chunk), half);
    });
    from.on('end', () => {
      const half = state.rtt / 2;
      setTimeout(() => to.end(), half);
    });
    from.on('error', () => to.destroy());
    from.on('close', () => setTimeout(() => to.destroy(), state.rtt / 2 + 5));
  };
  const server = net.createServer((client) => {
    state.connections += 1;
    state.open += 1;
    state.maxOpen = Math.max(state.maxOpen, state.open);
    client.setNoDelay(true);
    const upstream = net.connect(targetPort, '127.0.0.1');
    upstream.setNoDelay(true);
    sockets.add(client);
    sockets.add(upstream);
    client.on('close', () => {
      state.open -= 1;
      sockets.delete(client);
    });
    upstream.on('close', () => sockets.delete(upstream));
    pipe(client, upstream);
    pipe(upstream, client);
  });
  return new Promise((resolve) => {
    server.listen(listenPort, '127.0.0.1', () =>
      resolve({
        state,
        resetCounts() {
          state.connections = 0;
          state.maxOpen = state.open;
        },
        close: () =>
          new Promise((done) => {
            for (const socket of sockets) socket.destroy();
            server.close(() => done());
          }),
      }),
    );
  });
}

/** Instrumentation put into every page before its scripts: WebSocket frames, clicks, long tasks. */
export const INIT = `(() => {
  const P = (window.__perf = { t0: null, ws: [], longtasks: [], frames: null });
  try { performance.setResourceTimingBufferSize(100000); } catch (e) {}
  // A heartbeat for blocked main thread in both browsers (WebKit has no longtask): gaps over 50 ms.
  P.gaps = [];
  let beat = performance.now();
  const heart = () => { const now = performance.now(); if (now - beat > 66) P.gaps.push({ s: beat, d: now - beat - 16 }); beat = now; setTimeout(heart, 16); };
  setTimeout(heart, 16);
  addEventListener('pointerdown', () => { P.t0 = performance.now(); }, true);
  addEventListener('click', () => { if (P.t0 == null) P.t0 = performance.now(); }, true);
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) P.longtasks.push({ s: e.startTime, d: e.duration });
    }).observe({ type: 'longtask', buffered: true });
  } catch (e) {}
  const describe = (data) => {
    if (typeof data === 'string') {
      let type;
      try { const j = JSON.parse(data); type = j.type; } catch (e) {}
      return { kind: 'text', size: data.length, type, head: data.slice(0, 160) };
    }
    let size = 0, b0;
    try {
      if (data instanceof Blob) size = data.size;
      else { const u = ArrayBuffer.isView(data) ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength) : new Uint8Array(data); size = u.length; b0 = u[0]; }
    } catch (e) {}
    return { kind: 'binary', size, b0 };
  };
  const Native = window.WebSocket;
  class Recorded extends Native {
    constructor(...args) {
      super(...args);
      P.ws.push({ t: performance.now(), dir: 'connect', url: String(args[0]) });
      this.addEventListener('open', () => P.ws.push({ t: performance.now(), dir: 'open' }));
      this.addEventListener('message', (e) => P.ws.push({ t: performance.now(), dir: 'in', ...describe(e.data) }));
      this.addEventListener('close', (e) => P.ws.push({ t: performance.now(), dir: 'close', code: e.code }));
    }
    send(data) { P.ws.push({ t: performance.now(), dir: 'out', ...describe(data) }); return super.send(data); }
  }
  window.WebSocket = Recorded;
  // Who listens to what, and how many observers watch how many targets.
  const L = (P.listeners = {});
  const WATCHED = ['scroll', 'wheel', 'touchstart', 'touchmove', 'touchend', 'pointermove', 'pointerdown', 'pointerover', 'pointerenter', 'mouseenter', 'mouseover', 'focusin', 'resize', 'keydown'];
  const desc = (t) => {
    if (t === window) return 'window';
    if (t === document) return 'document';
    if (!(t instanceof Element)) return t && t.constructor ? t.constructor.name : '?';
    const data = [...t.attributes].map((a) => a.name).find((n) => n.startsWith('data-') && n !== 'data-slot' && n !== 'data-state') || '';
    return t.tagName.toLowerCase() + (t.getAttribute('data-testid') ? '#' + t.getAttribute('data-testid') : '') + (t.getAttribute('role') ? '[role=' + t.getAttribute('role') + ']' : '') + (data ? '[' + data + ']' : '') + (t.getAttribute('data-slot') ? '{' + t.getAttribute('data-slot') + '}' : '');
  };
  const add = EventTarget.prototype.addEventListener;
  EventTarget.prototype.addEventListener = function (type, fn, opts) {
    if (WATCHED.includes(type)) {
      const passive = typeof opts === 'object' && opts ? opts.passive : undefined;
      const key = type + ' | passive=' + passive + ' | ' + desc(this);
      L[key] = (L[key] || 0) + 1;
    }
    return add.call(this, type, fn, opts);
  };
  const O = (P.observers = {});
  for (const name of ['ResizeObserver', 'IntersectionObserver', 'MutationObserver']) {
    const C = window[name];
    if (!C) continue;
    const observe = C.prototype.observe;
    O[name] = { made: 0, observe: 0 };
    C.prototype.observe = function (...a) { O[name].observe += 1; return observe.apply(this, a); };
    const Made = function (...a) { O[name].made += 1; return new C(...a); };
    Made.prototype = C.prototype;
    window[name] = Made;
  }
})();`;

export const IPHONE = {
  viewport: { width: 390, height: 844 },
  deviceScaleFactor: 3,
  isMobile: true,
  hasTouch: true,
  userAgent:
    'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
};

export async function launch(kind) {
  if (kind === 'webkit') {
    const ws = need('PW_WS');
    return webkit.connect(ws, { exposeNetwork: '<loopback>', timeout: 60000 });
  }
  return chromium.launch({ args: ['--enable-precise-memory-info'] });
}

/** A signed-in context with the instrumentation. `mobile`: the iPhone-like one, dark. */
export async function signedIn(browser, { user = FIRST_USER, mobile = false, dark = false } = {}) {
  const context = await browser.newContext({
    baseURL: BASE,
    locale: 'ko-KR',
    timezoneId: 'Asia/Seoul',
    colorScheme: dark || mobile ? 'dark' : 'light',
    ...(mobile ? IPHONE : { viewport: { width: 1440, height: 900 } }),
  });
  await context.addInitScript(INIT);
  const response = await context.request.post('/api/v1/login', {
    data: { username: user, email: user, password: password(user) },
    headers: { 'X-Requested-With': 'XMLHttpRequest' },
  });
  if (response.status() !== 204) throw new Error(`login ${response.status()}`);
  return context;
}

export const ws = (name) => summary.workspaces[name];
export const boardOf = (title) => summary.boards.find((b) => b.title === title);
export const wikiOf = (title) => summary.wikis.find((w) => w.title === title);
export const boardUrl = (b, viewId = '', cardId = '') =>
  `/w/${b.workspaceId}/boards/${b.id}${viewId ? `/${viewId}` : ''}${cardId ? `/${cardId}` : ''}`;

/**
 * Arms a waiter for the named predicate, then (if `click` is a selector) clicks that element in
 * the same task, so the old screen cannot satisfy it. Resolves with { t0, t } in the page clock:
 * t is taken after the paint of the first frame on which the predicate holds (rAF, then a task).
 */
export async function clickAndWait(page, { click, pred, arg, timeout = 120000, text, go }) {
  return page.evaluate(
    ([sel, name, a, ms, label, url]) =>
      new Promise((resolve, reject) => {
        const fn = window.__preds[name];
        const start = performance.now();
        const afterPaint = (cb) => {
          const ch = new MessageChannel();
          ch.port1.onmessage = () => cb(performance.now());
          requestAnimationFrame(() => ch.port2.postMessage(0));
        };
        let t0 = start;
        if (sel) {
          let el = document.querySelector(sel);
          if (label)
            el = [...document.querySelectorAll(sel)].find((e) =>
              (e.textContent || '').trim().includes(label),
            );
          if (!el) return reject(new Error(`no element ${sel} ${label || ''}`));
          window.__perf.t0 = t0 = performance.now();
          el.click();
        } else if (url) {
          // As the app's own links do: the router follows the history (router.svelte.ts listen).
          window.__perf.t0 = t0 = performance.now();
          history.pushState(null, '', url);
          dispatchEvent(new PopStateEvent('popstate'));
        }
        const tick = () => {
          let ok = false;
          try {
            ok = fn(a);
          } catch (e) {
            ok = false;
          }
          if (ok) afterPaint((t) => resolve({ t0, t, ms: t - t0 }));
          else if (performance.now() - start > ms) reject(new Error(`timeout ${name}`));
          else requestAnimationFrame(tick);
        };
        requestAnimationFrame(tick);
      }),
    [click, pred, arg, timeout, text, go],
  );
}

/**
 * A fixed in-page workload, to tell how fast this browser on this machine is right now:
 * a JS loop, and making and laying out 5,000 divs. Printed beside each table.
 */
export async function calibrate(page) {
  return page.evaluate(() => {
    const t0 = performance.now();
    let x = 0;
    for (let i = 0; i < 3e6; i += 1) x = (x + Math.sqrt(i) * 1.0001) % 1e9;
    const t1 = performance.now();
    const box = document.createElement('div');
    box.style.cssText = 'position:absolute;left:-99999px;top:0;width:300px';
    for (let i = 0; i < 5000; i += 1) {
      const d = document.createElement('div');
      d.textContent = `row ${i} ${x}`;
      d.style.padding = '2px';
      box.append(d);
    }
    document.body.append(box);
    void box.offsetHeight;
    const t2 = performance.now();
    box.remove();
    return { jsMs: Math.round(t1 - t0), domMs: Math.round(t2 - t1) };
  });
}

/** A snippet of the first longer text of a ProseMirror JSON body, to wait for on the screen. */
export function snippetOf(body) {
  let found;
  const walk = (n) => {
    if (found || !n) return;
    if (typeof n.text === 'string' && n.text.trim().length >= 15)
      found = n.text.trim().slice(0, 12);
    for (const c of n.content ?? []) walk(c);
  };
  walk(typeof body === 'string' ? JSON.parse(body) : body);
  return found;
}

/** Waits until the main thread has had no gap over 50 ms for `quiet` ms; returns the page time. */
export async function mainQuiet(page, quiet = 1000, timeout = 60000) {
  return page.evaluate(
    ([q, ms]) =>
      new Promise((resolve) => {
        const start = performance.now();
        let last = performance.now();
        let calm = performance.now();
        const ch = new MessageChannel();
        ch.port1.onmessage = () => {
          const now = performance.now();
          if (now - last > 50) calm = now;
          last = now;
          if (now - calm >= q || now - start > ms) resolve(calm);
          else setTimeout(() => ch.port2.postMessage(0), 10);
        };
        ch.port2.postMessage(0);
      }),
    [quiet, timeout],
  );
}

/** Predicates the harness waits on, by name (installed as an init script). */
export const PREDS = `(() => {
  const visible = (el) => !!el && el.getClientRects().length > 0;
  window.__preds = {
    // The board page shows the board titled a, with at least one card, row or bar of it drawn.
    // The board page of board id a is shown with a card, row or bar of it drawn (loading = no board-page).
    board: (id) => {
      if (id && !location.pathname.includes(id)) return false;
      const page = document.querySelector('[data-testid="board-page"]');
      return !!page && !!page.querySelector('[data-card], [data-card-id], [data-bar], [data-day]');
    },
    // A workspace a shows its first board (cards drawn) or wiki page (body text), whichever it opens on.
    wsShown: (ws) => {
      if (!location.pathname.includes(ws)) return false;
      const page = document.querySelector('[data-testid="board-page"]');
      if (page && page.querySelector('[data-card], [data-card-id], [data-bar], [data-day]')) return true;
      return [...document.querySelectorAll('[data-editor]')].some((b) => visible(b) && (b.textContent || '').trim().length > 20);
    },
    wsReady: (ws) => {
      if (!location.pathname.includes(ws)) return false;
      if (location.pathname.includes('/boards/')) return true;
      // The editor is in: writable, or reading where the reader may only read.
      return [...document.querySelectorAll('[data-editor="ready"], [data-editor="reading"]')].some(visible);
    },
    // The kanban's column a has at least one card drawn.
    column: (opt) => !!document.querySelector('[data-column-cards="' + opt + '"] [data-card]'),
    any: (sel) => visible(document.querySelector(sel)),
    text: (s) => (document.body.innerText || '').includes(s),
    // A card's or page's body (CardBody.svelte, data-editor) shows text: read-only view or editor.
    bodyShown: (where) => {
      if (where && !location.pathname.includes(where)) return false;
      const b = [...document.querySelectorAll('[data-editor]')].find(visible);
      return !!b && (b.textContent || '').trim().length > 20;
    },
    // The body shows this snippet of its real text (from the API), read-only view or editor.
    bodyHas: (a) => {
      if (a.where && !location.pathname.includes(a.where)) return false;
      return [...document.querySelectorAll('[data-editor]')].some((b) => visible(b) && (b.textContent || '').includes(a.snippet));
    },
    // ... and in the editor, writable.
    bodyReadyHas: (a) => {
      if (a.where && !location.pathname.includes(a.where)) return false;
      return [...document.querySelectorAll('[data-editor="ready"]')].some((b) => visible(b) && !!b.querySelector('[contenteditable="true"]') && (b.textContent || '').includes(a.snippet));
    },
    // ... and the editor is in, writable.
    bodyReady: (where) => {
      if (where && !location.pathname.includes(where)) return false;
      const b = [...document.querySelectorAll('[data-editor="ready"]')].find(visible);
      return !!b && !!b.querySelector('[contenteditable="true"]') && (b.textContent || '').trim().length > 20;
    },
    // A body editor that shows text and takes typing.
    editable: (minChars) => {
      const pm = [...document.querySelectorAll('.ProseMirror')].find(visible);
      return !!pm && pm.getAttribute('contenteditable') === 'true' && (pm.textContent || '').length >= (minChars || 1);
    },
    bodyText: (minChars) => {
      const pm = [...document.querySelectorAll('.ProseMirror')].find(visible);
      return !!pm && (pm.textContent || '').length >= (minChars || 1);
    },
  };
})();`;

/** Resource entries and WS frames after `since` (page clock), for the waterfall. */
export async function collect(page, since = 0) {
  return page.evaluate((from) => {
    const res = performance
      .getEntriesByType('resource')
      .filter((e) => e.startTime >= from - 1)
      .map((e) => ({
        url: e.name.replace(location.origin, ''),
        type: e.initiatorType,
        start: e.startTime,
        reqStart: e.requestStart,
        resStart: e.responseStart,
        end: e.responseEnd,
        transfer: e.transferSize,
        encoded: e.encodedBodySize,
        decoded: e.decodedBodySize,
      }));
    const ws = window.__perf.ws.filter((m) => m.t >= from - 1);
    const lt = window.__perf.longtasks.filter((m) => m.s >= from - 1);
    return { res, ws, lt, t0: window.__perf.t0 };
  }, since);
}

/** Waits until no resource has started or ended for `quiet` ms (page clock); returns the last end. */
export async function networkQuiet(page, quiet = 700, timeout = 30000) {
  return page.evaluate(
    ([q, ms]) =>
      new Promise((resolve) => {
        const start = performance.now();
        const last = () => {
          let t = 0;
          for (const e of performance.getEntriesByType('resource'))
            t = Math.max(t, e.responseEnd, e.startTime);
          for (const m of window.__perf.ws) t = Math.max(t, m.t);
          return t;
        };
        const check = () => {
          const t = last();
          if (performance.now() - t >= q || performance.now() - start > ms) resolve(t);
          else setTimeout(check, 100);
        };
        check();
      }),
    [quiet, timeout],
  );
}

export const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  const n = s.length;
  return n === 0 ? NaN : n % 2 ? s[(n - 1) / 2] : (s[n / 2 - 1] + s[n / 2]) / 2;
};
export const fmt = (xs) => {
  const r = xs.map((x) => Math.round(x));
  return `${Math.round(median(xs))} (${Math.min(...r)}–${Math.max(...r)})`;
};
export const save = (name, data) =>
  fs.writeFileSync(`${OUT}/${name}.json`, JSON.stringify(data, null, 1));
fs.mkdirSync(OUT, { recursive: true });

/**
 * Scrolls `sel` down by `px` each animation frame for `ms`, in the page (identical in both
 * browsers). Frame deltas and heartbeat gaps tell how smooth it was.
 */
export async function scrollFrames(page, sel, { px = 60, ms = 3000 } = {}) {
  return page.evaluate(
    ([s, step, dur]) =>
      new Promise((resolve) => {
        // 'auto': the board page's element with the most to scroll down.
        const el =
          s === 'auto'
            ? [...document.querySelectorAll('[data-testid="board-page"] *')]
                .filter(
                  (e) =>
                    ['auto', 'scroll'].includes(getComputedStyle(e).overflowY) &&
                    e.scrollHeight > e.clientHeight + 50,
                )
                .sort(
                  (a, b) => b.scrollHeight - b.clientHeight - (a.scrollHeight - a.clientHeight),
                )[0]
            : document.querySelector(s);
        if (!el) return resolve({ deltas: [], dur: 1, scrolled: 0, gaps: [], none: true });
        const deltas = [];
        const gapsBefore = window.__perf.gaps.length;
        const start = performance.now();
        const top0 = el.scrollTop;
        let last = start;
        const frame = () => {
          const now = performance.now();
          deltas.push(now - last);
          last = now;
          el.scrollTop += step;
          if (now - start < dur && el.scrollTop + el.clientHeight < el.scrollHeight - 2)
            requestAnimationFrame(frame);
          else {
            const end = performance.now();
            const gaps = window.__perf.gaps.slice(gapsBefore).filter((g) => g.s >= start);
            resolve({
              deltas: deltas.slice(1),
              dur: end - start,
              scrolled: el.scrollTop - top0,
              gaps,
            });
          }
        };
        requestAnimationFrame(frame);
      }),
    [sel, px, ms],
  );
}

export function frameStats(r) {
  const d = r.deltas;
  const s = [...d].sort((a, b) => a - b);
  const pct = (p) => s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))];
  return {
    frames: d.length,
    fps: (d.length / r.dur) * 1000,
    p50: pct(50),
    p95: pct(95),
    max: s[s.length - 1],
    over33: d.filter((x) => x > 33.4).length,
    over50: d.filter((x) => x > 50).length,
    pxPerS: (r.scrolled / r.dur) * 1000,
    blocked: r.gaps.reduce((a, g) => a + g.d, 0),
  };
}

export async function domCounts(page, opt) {
  return page.evaluate((o) => {
    const col = o && document.querySelector(`[data-column-cards="${o}"]`);
    return {
      nodes: document.getElementsByTagName('*').length,
      cards: document.querySelectorAll('[data-card]').length,
      columnNodes: col ? col.getElementsByTagName('*').length : undefined,
      columnCards: col ? col.querySelectorAll('[data-card]').length : undefined,
      perCard: col
        ? col.getElementsByTagName('*').length /
          Math.max(1, col.querySelectorAll('[data-card]').length)
        : undefined,
    };
  }, opt);
}

/** Remedy test: one Intl.DateTimeFormat per (locale, options), as `calendarFormatter` in logic/format.ts does. */
export const INTL_MEMO = `(() => {
  const Native = Intl.DateTimeFormat;
  const cache = new Map();
  function Cached(locale, options) {
    const key = JSON.stringify([locale, options]);
    let f = cache.get(key);
    if (!f) { f = new Native(locale, options); cache.set(key, f); }
    return f;
  }
  Cached.prototype = Native.prototype;
  Cached.supportedLocalesOf = Native.supportedLocalesOf.bind(Native);
  Intl.DateTimeFormat = Cached;
})();`;

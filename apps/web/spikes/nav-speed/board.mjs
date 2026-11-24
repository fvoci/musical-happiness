// The first screen of a board from a fresh, signed-in page (Chromium): to its cards drawn, under
// added RTT; how its cards are read (requests to the card list, and in how many waves: a request
// that starts before every request of the wave so far has ended is in that wave), how often the
// reader's workspaces are read, and how many setPresence go out until the network is quiet.
// node board.mjs <big|small> <rtt,rtt,...> [runs]
/* eslint-disable -- a measurement script run by node (not by CI): its in-page code reads what the harness puts on window. */
// @ts-nocheck: the same.
import {
  launch,
  signedIn,
  delayProxy,
  boardOf,
  boardUrl,
  PREDS,
  mainQuiet,
  networkQuiet,
  collect,
  loadavg,
  save,
  fmt,
} from './lib.mjs';

const [which = 'big', rttArg = '0,150', runsArg = '3'] = process.argv.slice(2);
const rtts = rttArg.split(',').map(Number);
const runs = Number(runsArg);
const board = boardOf(which === 'big' ? '대형 보드 (2,000장)' : '출시 준비');

/** Start times grouped into waves: a request that starts after the previous wave's last end. */
function waves(list) {
  const sorted = [...list].sort((a, b) => a.start - b.start);
  const out = [];
  for (const r of sorted) {
    const last = out.at(-1);
    if (last && r.start < last.end) {
      last.n += 1;
      last.end = Math.max(last.end, r.end);
    } else out.push({ start: r.start, end: r.end, n: 1 });
  }
  return out;
}

const proxy = await delayProxy();
const browser = await launch('chromium');
const out = { which, load: [loadavg()], byRtt: {} };
try {
  for (const rtt of rtts) {
    const list = [];
    for (let i = 0; i < runs; i += 1) {
      proxy.state.rtt = 0;
      const context = await signedIn(browser);
      await context.addInitScript(PREDS);
      const page = await context.newPage();
      proxy.state.rtt = rtt;
      await page.goto(boardUrl(board));
      await page.waitForSelector('[data-card], [data-card-id]', { timeout: 300000 });
      const shown = await page.evaluate(() => performance.now());
      await networkQuiet(page, 1000, 120000);
      await mainQuiet(page, 800, 300000);
      const c = await collect(page, 0);
      const cards = c.res.filter((r) => /\/boards\/[^/]+\/cards\?/.test(r.url));
      const run = {
        shownMs: shown,
        cardRequests: cards.length,
        cardWaves: waves(cards).length,
        cardPages: cards.map(
          (r) =>
            `${new URL(r.url, 'http://x').search} ${Math.round(r.start)}→${Math.round(r.end)} ${r.encoded}B`,
        ),
        workspaceLists: c.res.filter((r) => r.url.startsWith('/api/v1/users/me/workspaces')).length,
        presenceSent: c.ws
          .filter((m) => m.dir === 'out' && m.type === 'setPresence')
          .map((m) => m.size),
      };
      list.push(run);
      await context.close();
    }
    out.byRtt[rtt] = list;
    console.log(
      JSON.stringify({
        rtt,
        shown: fmt(list.map((r) => r.shownMs)),
        cardRequests: list[0].cardRequests,
        cardWaves: list[0].cardWaves,
        workspaceLists: list[0].workspaceLists,
        presenceSent: list[0].presenceSent,
        pages: list[0].cardPages,
      }),
    );
  }
} finally {
  await browser.close();
  await proxy.close();
}
out.load.push(loadavg());
console.log(JSON.stringify({ which, load: out.load }));
save(`board-${which}`, out);

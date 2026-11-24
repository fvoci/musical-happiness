// Item 1: navigation waterfalls in Chromium under added RTT (delay proxy: HTTP and WebSocket alike).
// node nav.mjs <b2b|b2w|ws> <rtt,rtt,...> [runs]
/* eslint-disable -- a measurement script run by node (not by CI): its in-page code reads what the harness puts on window. */
// @ts-nocheck: the same.
import {
  launch,
  signedIn,
  delayProxy,
  boardOf,
  wikiOf,
  boardUrl,
  PREDS,
  clickAndWait,
  mainQuiet,
  networkQuiet,
  collect,
  calibrate,
  snippetOf,
  loadavg,
  save,
  fmt,
  median,
  summary,
} from './lib.mjs';

const [scenario = 'b2b', rttArg = '0,50,100,150', runsArg = '3'] = process.argv.slice(2);
const rtts = rttArg.split(',').map(Number);
const runs = Number(runsArg);
const origin = boardOf('출시 준비');
const target = boardOf('버그');
const wiki = wikiOf('제품 위키');
const ops = summary.workspaces['운영팀'];
const prod = summary.workspaces['제품팀'];

const sidebar = (id) => `[data-testid="sidebar"] li[data-board="${id}"] > a`;

/** The measured step and the unmeasured way back, per scenario. */
const SCENARIOS = {
  b2b: {
    start: boardUrl(origin),
    go: { click: sidebar(target.id), pred: 'board', arg: target.id },
    back: { click: sidebar(origin.id), pred: 'board', arg: origin.id },
  },
  b2w: {
    start: boardUrl(origin),
    // pred and arg are set after the warm visit: the page the wiki opens on, and a snippet of its body.
    go: { click: sidebar(wiki.id), pred: 'bodyShown', arg: wiki.id, then: 'bodyReadyHas' },
    back: { click: sidebar(origin.id), pred: 'board', arg: origin.id },
  },
  ws: {
    start: boardUrl(origin),
    open: '[data-testid="workspace-switcher"]',
    go: { click: '[role="menuitem"]', text: '운영팀', pred: 'wsShown', arg: ops, then: 'wsReady' },
    back: { click: '[role="menuitem"]', text: '제품팀', pred: 'wsShown', arg: prod, open: true },
  },
};
const sc = SCENARIOS[scenario];

async function step(page, s, measure) {
  if ((s === sc.go && sc.open) || s.open) {
    await page.click(sc.open);
    await page.waitForSelector('[role="menuitem"]');
  }
  const got = await clickAndWait(page, s);
  let ready;
  if (s.then && (typeof s.arg === 'object' || s.then === 'wsReady'))
    ready = await clickAndWait(page, { pred: s.then, arg: s.arg, timeout: 120000 });
  const quietNet = await networkQuiet(page, 800);
  const quietMain = await mainQuiet(page, 800);
  if (!measure) return undefined;
  const c = await collect(page, got.t0);
  const rel = (t) => Math.round(t - got.t0);
  return {
    ms: got.ms,
    readyMs: ready ? ready.t - got.t0 : undefined,
    settledMs: Math.max(quietNet, quietMain) - got.t0,
    longtaskMs: c.lt.reduce((a, l) => a + l.d, 0),
    requests: c.res.map((r) => ({
      url: r.url,
      type: r.type,
      start: rel(r.start),
      queued: Math.round(r.reqStart - r.start),
      ttfb: Math.round(r.resStart - r.reqStart),
      end: rel(r.end),
      bytes: r.transfer,
      body: r.encoded,
    })),
    ws: c.ws.map((m) => ({ t: rel(m.t), dir: m.dir, type: m.type ?? m.kind, size: m.size })),
  };
}

const proxy = await delayProxy();
const browser = await launch('chromium');
const out = { scenario, load: [loadavg()], byRtt: {} };
try {
  for (const rtt of rtts) {
    proxy.state.rtt = 0;
    const context = await signedIn(browser);
    await context.addInitScript(PREDS);
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    page.on('console', (m) => m.type() === 'error' && errors.push(m.text().slice(0, 200)));
    page.on('response', (r) => r.status() >= 400 && errors.push(`${r.status()} ${r.url()}`));
    await page.goto(sc.start);
    await page.waitForSelector('[data-card]');
    await mainQuiet(page, 800);
    // Warm: the target once (its chunks cached, as on a visit that went on for a while), and back.
    out.calibration ??= await calibrate(page);
    await step(page, sc.go, false);
    if (scenario === 'b2w' && sc.go.pred === 'bodyShown') {
      const pageId = await page.evaluate(() => location.pathname.split('/').pop());
      const detail = await (
        await context.request.get(`/api/v1/w/${wiki.workspaceId}/wikis/${wiki.id}/pages/${pageId}`)
      ).json();
      sc.go.pred = 'bodyHas';
      sc.go.arg = { where: wiki.id, snippet: snippetOf(detail.body) };
      out.page = { pageId, snippet: sc.go.arg.snippet };
    }
    await step(page, sc.back, false);
    proxy.state.rtt = rtt;
    const list = [];
    for (let i = 0; i < runs; i += 1) {
      proxy.resetCounts();
      const r = await step(page, sc.go, true);
      r.newConnections = proxy.state.connections;
      list.push(r);
      await step(page, sc.back, false);
    }
    await context.close();
    out.byRtt[rtt] = { runs: list, errors };
    const api = list[0].requests.filter((r) => r.url.startsWith('/api'));
    console.log(
      JSON.stringify({
        rtt,
        shown: fmt(list.map((r) => r.ms)),
        ready: list[0].readyMs !== undefined ? fmt(list.map((r) => r.readyMs)) : undefined,
        settled: fmt(list.map((r) => r.settledMs)),
        api: api.length,
        all: list[0].requests.length,
        apiBytes: api.reduce((a, r) => a + r.bytes, 0),
        ws: list[0].ws.length,
        longtask: fmt(list.map((r) => r.longtaskMs)),
        errors: errors.length,
      }),
    );
  }
} finally {
  await browser.close();
  await proxy.close();
}
out.load.push(loadavg());
const shown = rtts.map((r) => median(out.byRtt[r].runs.map((x) => x.ms)));
const slope = (shown.at(-1) - shown[0]) / (rtts.at(-1) - rtts[0]);
out.roundTripsShown = slope;
if (out.byRtt[rtts[0]].runs[0].readyMs !== undefined) {
  const ready = rtts.map((r) => median(out.byRtt[r].runs.map((x) => x.readyMs)));
  out.roundTripsReady = (ready.at(-1) - ready[0]) / (rtts.at(-1) - rtts[0]);
}
console.log(
  JSON.stringify({
    scenario,
    roundTripsShown: slope.toFixed(2),
    roundTripsReady: out.roundTripsReady?.toFixed(2),
    load: out.load,
  }),
);
save(`nav-${scenario}`, out);

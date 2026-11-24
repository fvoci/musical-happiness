// Item 3: opening a card or a wiki page: click to the body's real text shown, and to the editor
// ready (writable, same text), under added RTT. Three states per run:
//   cold   = fresh context, empty HTTP cache (a first-ever visit);
//   warm   = a second, different one in the same page (chunks and modules in memory);
//   reload = after page.reload(): chunks in the HTTP cache, modules not in memory (the usual first open).
// node card.mjs <card|page|cardbig> <rtt,rtt,...> [runs] [chromium|webkit] [mobile]
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
} from './lib.mjs';

const [mode = 'card', rttArg = '0,50,100,150', runsArg = '3', kind = 'chromium', mobileArg] =
  process.argv.slice(2);
const rtts = rttArg.split(',').map(Number);
const runs = Number(runsArg);
const mobile = mobileArg === 'mobile';
const board = mode === 'cardbig' ? boardOf('대형 보드 (2,000장)') : boardOf('출시 준비');
const wiki = wikiOf('제품 위키');
const ws = (mode === 'page' ? wiki : board).workspaceId;
const start = mode === 'page' ? `/w/${ws}/wikis/${wiki.id}` : boardUrl(board);

/** Three cards with a body (by the API's hasBody) or three top pages, with a snippet of each body. */
async function targets(request) {
  const api = `/api/v1/w/${ws}`;
  let ids;
  if (mode === 'page') {
    const pages = await (await request.get(`${api}/wikis/${wiki.id}/pages`)).json();
    ids = pages.filter((p) => !p.parentId).map((p) => p.id);
  } else {
    const cards = await (
      await request.get(`${api}/boards/${board.id}/cards?page=0&perPage=1000`)
    ).json();
    ids = cards.filter((c) => c.hasBody).map((c) => c.id);
  }
  const picked = [];
  for (const id of ids.slice(1)) {
    const detail = await (
      await request.get(
        mode === 'page' ? `${api}/wikis/${wiki.id}/pages/${id}` : `${api}/cards/${id}`,
      )
    ).json();
    const snippet = detail.body && snippetOf(detail.body);
    if (snippet) picked.push({ id, snippet });
    if (picked.length === 3) break;
  }
  return picked;
}

async function open(page, t) {
  const arg = { where: t.id, snippet: t.snippet };
  const visibleRow =
    mode === 'page' &&
    (await page
      .locator(`[data-page="${t.id}"] a[href]`)
      .isVisible()
      .catch(() => false));
  const how =
    mode === 'page'
      ? visibleRow
        ? { click: `[data-page="${t.id}"] a[href]` }
        : { go: `/w/${ws}/wikis/${wiki.id}/${t.id}` }
      : { click: `[data-card="${t.id}"]` };
  const shown = await clickAndWait(page, { ...how, pred: 'bodyHas', arg, timeout: 120000 });
  const ready = await clickAndWait(page, { pred: 'bodyReadyHas', arg, timeout: 120000 });
  await networkQuiet(page, 800);
  await mainQuiet(page, 800);
  const c = await collect(page, shown.t0);
  const rel = (x) => Math.round(x - shown.t0);
  const requests = c.res.map((r) => ({
    url: r.url.replace(/^\/static\//, ''),
    start: rel(r.start),
    queued: Math.round(r.reqStart - r.start),
    ttfb: Math.round(r.resStart - r.reqStart),
    end: rel(r.end),
    bytes: r.transfer,
    body: r.encoded,
    decoded: r.decoded,
  }));
  const wsm = c.ws.map((m) => ({
    t: rel(m.t),
    dir: m.dir,
    type: m.type ?? m.kind,
    size: m.size,
  }));
  const detail = requests.find(
    (r) => r.url.includes(`/cards/${t.id}`) || r.url.includes(`/pages/${t.id}`),
  );
  const chunks = requests.filter((r) => r.url.endsWith('.js'));
  const lastChunk = Math.max(0, ...chunks.map((r) => r.end));
  const firstDoc = wsm.find((m) => m.dir === 'in' && m.type === 'binary');
  const joined = wsm.find((m) => m.dir === 'in' && m.type === 'docJoined');
  const join = wsm.find((m) => m.dir === 'out' && m.type === 'joinDoc');
  const readyMs = ready.t - shown.t0;
  return {
    id: t.id,
    shownMs: shown.ms,
    readyMs,
    detailEnd: detail?.end,
    shownBeforeDetail: detail ? shown.ms < detail.end : undefined,
    joinAt: join?.t,
    joinedAt: joined?.t,
    firstDocAt: firstDoc?.t,
    lastChunkEnd: lastChunk,
    editorInitMs: readyMs - Math.max(lastChunk, firstDoc?.t ?? 0),
    chunks: chunks.map(
      (r) => `${r.url.replace(/-[0-9a-f]{16}\.js$/, '')} ${r.bytes}B ${r.start}→${r.end}`,
    ),
    longtaskMs: c.lt.reduce((a, l) => a + l.d, 0),
    requests,
    ws: wsm,
  };
}

async function close(page) {
  if (mode === 'page') return;
  await page.keyboard.press('Escape');
  await page
    .waitForFunction(() => !document.querySelector('[data-testid="card-dialog"]'), null, {
      timeout: 30000,
    })
    .catch(() => {});
  await mainQuiet(page, 500);
}

async function ready(page) {
  await page.waitForSelector(
    mode === 'page' ? '[data-testid="page-tree"] [data-page]' : '[data-card]',
    { timeout: 300000 },
  );
  await mainQuiet(page, 1000, 300000);
}

const proxy = await delayProxy();
const browser = await launch(kind);
const out = { mode, kind, mobile, load: [loadavg()], byRtt: {} };
try {
  for (const rtt of rtts) {
    const list = [];
    const errors = [];
    for (let i = 0; i < runs; i += 1) {
      proxy.state.rtt = 0;
      const context = await signedIn(browser, { mobile });
      await context.addInitScript(PREDS);
      const page = await context.newPage();
      page.on('pageerror', (e) => errors.push(e.message));
      page.on('console', (m) => m.type() === 'error' && errors.push(m.text().slice(0, 200)));
      page.on('response', (r) => r.status() >= 400 && errors.push(`${r.status()} ${r.url()}`));
      const ts = await targets(context.request);
      await page.goto(start);
      await ready(page);
      const cal = i === 0 ? await calibrate(page) : undefined;
      proxy.state.rtt = rtt;
      const cold = await open(page, ts[0]);
      await close(page);
      const warm = await open(page, ts[1]);
      await close(page);
      proxy.state.rtt = 0;
      await page.reload();
      await ready(page);
      proxy.state.rtt = rtt;
      const reload = await open(page, ts[2]);
      list.push({ cold, warm, reload, cal });
      await context.close();
    }
    out.byRtt[rtt] = { runs: list, errors };
    const row = { rtt };
    for (const s of ['cold', 'warm', 'reload']) {
      row[`${s}Shown`] = fmt(list.map((r) => r[s].shownMs));
      row[`${s}Ready`] = fmt(list.map((r) => r[s].readyMs));
      row[`${s}Init`] = fmt(list.map((r) => r[s].editorInitMs));
    }
    row.early = list.some(
      (r) => r.cold.shownBeforeDetail || r.warm.shownBeforeDetail || r.reload.shownBeforeDetail,
    );
    row.errors = errors.length;
    console.log(JSON.stringify(row));
  }
} finally {
  await browser.close();
  await proxy.close();
}
out.load.push(loadavg());
const tag = `${mode}-${kind}${mobile ? '-mobile' : ''}`;
save(`open-${tag}`, out);
console.log(JSON.stringify({ tag, load: out.load, calibration: out.byRtt[rtts[0]].runs[0].cal }));

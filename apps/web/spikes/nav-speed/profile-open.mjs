// Where the main thread's time goes as a card opens warm (another card opened and closed first),
// Chromium, under added RTT: a CPU profile from the click to the editor ready, its time by script
// file (the build's chunks) and by function, and the long tasks with their start.
// node profile-open.mjs <rtt> [runs]
/* eslint-disable -- a measurement script run by node (not by CI): its in-page code reads what the harness puts on window. */
// @ts-nocheck: the same.
import {
  launch,
  signedIn,
  delayProxy,
  boardOf,
  boardUrl,
  PREDS,
  clickAndWait,
  mainQuiet,
  collect,
  loadavg,
  save,
  fmt,
} from './lib.mjs';

const [rttArg = '0', runsArg = '3'] = process.argv.slice(2);
const rtt = Number(rttArg);
const runs = Number(runsArg);
const board = boardOf('출시 준비');

const proxy = await delayProxy();
const browser = await launch('chromium');
const out = { rtt, load: [loadavg()], runs: [] };
try {
  for (let i = 0; i < runs; i += 1) {
    const context = await signedIn(browser);
    await context.addInitScript(PREDS);
    const page = await context.newPage();
    const cdp = await context.newCDPSession(page);
    await cdp.send('Profiler.enable');
    await cdp.send('Profiler.setSamplingInterval', { interval: 200 });
    await page.goto(boardUrl(board));
    await page.waitForSelector('[data-card]', { timeout: 120000 });
    await mainQuiet(page, 1000);
    // Cards with a body, as the API says (`hasBody`): the editor shows text once ready.
    const listed = await context.request.get(
      `/api/v1/w/${board.workspaceId}/boards/${board.id}/cards?page=0&perPage=1000`,
    );
    const ids = (await listed.json()).filter((card) => card.hasBody).map((card) => card.id);
    // Warm: one card opened and closed, so the chunks and modules are in.
    await clickAndWait(page, { click: `[data-card="${ids[1]}"]`, pred: 'bodyReady', arg: ids[1] });
    await page.keyboard.press('Escape');
    await page.waitForFunction(() => !document.querySelector('[data-testid="card-dialog"]'));
    await mainQuiet(page, 1000);
    proxy.state.rtt = rtt;
    await cdp.send('Profiler.start');
    const opened = await clickAndWait(page, {
      click: `[data-card="${ids[2]}"]`,
      pred: 'bodyReady',
      arg: ids[2],
    });
    const { profile } = await cdp.send('Profiler.stop');
    proxy.state.rtt = 0;
    const c = await collect(page, opened.t0);
    const nodes = new Map(profile.nodes.map((n) => [n.id, n]));
    const byFile = new Map();
    const byFn = new Map();
    // Samples from the click (profile time is in microseconds, from its own start).
    let t = profile.startTime;
    for (let s = 0; s < profile.samples.length; s += 1) {
      t += profile.timeDeltas[s] ?? 0;
      const dt = (profile.timeDeltas[s + 1] ?? 0) / 1000;
      const n = nodes.get(profile.samples[s]);
      const f = n.callFrame;
      const file = (f.url.split('/').pop() || f.functionName || '(native)').replace(
        /-[\w-]{8,}\.js$/,
        '',
      );
      byFile.set(file, (byFile.get(file) ?? 0) + dt);
      const fn = `${f.functionName || '(anon)'} ${file}:${f.lineNumber + 1}`;
      byFn.set(fn, (byFn.get(fn) ?? 0) + dt);
    }
    const top = (m, k) =>
      [...m]
        .sort((a, b) => b[1] - a[1])
        .slice(0, k)
        .map(([name, ms]) => `${Math.round(ms)} ${name}`);
    const rel = (x) => Math.round(x - opened.t0);
    const run = {
      readyMs: Math.round(opened.ms),
      byFile: top(byFile, 14),
      byFn: top(byFn, 20),
      longtasks: c.lt.map((l) => `${rel(l.s)}+${Math.round(l.d)}`),
      ws: c.ws
        .filter((m) => m.dir !== 'connect')
        .map((m) => `${rel(m.t)} ${m.dir} ${m.action ?? m.kind}`),
      requests: c.res.map(
        (r) =>
          `${rel(r.start)}→${rel(r.end)} ${r.url.split('?')[0].split('/').slice(-2).join('/')}`,
      ),
    };
    out.runs.push(run);
    console.log(JSON.stringify(run));
    await context.close();
  }
} finally {
  await browser.close();
  await proxy.close();
}
out.load.push(loadavg());
console.log(JSON.stringify({ rtt, ready: fmt(out.runs.map((r) => r.readyMs)), load: out.load }));
save(`profile-open-${rtt}`, out);

/**
 * What saving real documents asks the writer, as the editor's frame asks it:
 * run by hand against a folder of samples,
 *
 *   FVOCI_HWP_SAMPLES=<folder> pnpm --filter web exec vitest run --project logic tests/logic/hwp-save-samples.test.ts
 *
 * Each document is checked as the frame checks it, opened in rhwp as the
 * editor opens it, and saved unchanged with rhwp's report, as the editor
 * saves it; then what the frame compares (comparison.ts) is compared, and
 * timed. What each came to is written, in numbers and names of streams and
 * parts, to `FVOCI_HWP_SAVE_REPORT`. The samples are read as bytes, never run.
 */
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { HwpDocument } from '@rhwp/core';
import { describe, expect, it } from 'vitest';

import { leftOut, openedOf } from '../../src/editor-frame/comparison.ts';
import { lossesOf } from '../../src/editor-frame/losses.ts';
import { examine } from '../../src/lib/file-viewer/formats.ts';
import { HwpReader } from '../../src/viewer-frame/hwp-document.ts';
import type { HwpFormat } from '../../src/viewer-frame/hwp-messages.ts';

import { startRhwp } from '../support/hwp-files.ts';

const FOLDER = process.env.FVOCI_HWP_SAMPLES;
const REPORT = process.env.FVOCI_HWP_SAVE_REPORT;

/** What saving one sample unchanged came to. */
interface Outcome {
  file: string;
  bytes: number;
  format?: HwpFormat;
  /** Why it was not saved: not a document the editor edits, refused, or rhwp failed. */
  skipped?: string;
  /** What rhwp's report counts. */
  reported?: number;
  unprotected?: boolean;
  /** The streams or parts left out, each with why. */
  leftOut?: string[];
  /** How long rhwp took to export it, and the comparison after, in milliseconds. */
  exportMs?: number;
  compareMs?: number;
  /** What the comparison threw: the save would fail. */
  compareFailed?: string;
}

function samplesUnder(folder: string): string[] {
  const found: string[] = [];
  for (const entry of readdirSync(folder, { withFileTypes: true })) {
    if (entry.name === '.git') continue;
    const at = path.join(folder, entry.name);
    if (entry.isDirectory()) found.push(...samplesUnder(at));
    else if (/\.hwpx?$/i.test(entry.name) && entry.isFile()) found.push(at);
  }
  return found.sort();
}

const copyOf = (data: Uint8Array): ArrayBuffer => {
  const copy = new ArrayBuffer(data.byteLength);
  new Uint8Array(copy).set(data);
  return copy;
};

const said = (error: unknown): string =>
  (error instanceof Error ? error.message : String(error)).slice(0, 80);

async function tryOne(file: string): Promise<Outcome> {
  const data = readFileSync(file);
  const outcome: Outcome = { file, bytes: data.byteLength };
  const examined = examine(new Uint8Array(data));
  if (examined.kind !== 'format' || (examined.format !== 'hwp' && examined.format !== 'hwpx')) {
    return { ...outcome, skipped: 'not edited' };
  }
  const format = examined.format;
  outcome.format = format;
  const answer = await new HwpReader().answer({ kind: 'check', format, bytes: copyOf(data) });
  if (answer.kind !== 'checked') {
    return { ...outcome, skipped: answer.kind === 'failed' ? answer.reason : answer.kind };
  }
  const given = new Uint8Array(answer.bytes);
  let document: HwpDocument;
  try {
    document = new HwpDocument(given);
  } catch (error) {
    return { ...outcome, skipped: `rhwp: ${said(error)}` };
  }
  try {
    const opened = openedOf(format, given);
    const started = performance.now();
    const exported =
      format === 'hwp' ? document.exportHwpWithReport() : document.exportHwpxWithReport();
    let saved: Uint8Array;
    try {
      outcome.reported = lossesOf(JSON.parse(exported.contentLoss())).count;
      saved = exported.takeBytes();
    } finally {
      exported.free();
    }
    const comparing = performance.now();
    let left: ReturnType<typeof leftOut>;
    try {
      left = leftOut(opened, saved);
    } catch (error) {
      return { ...outcome, compareFailed: said(error) };
    }
    outcome.compareMs = Math.round(performance.now() - comparing);
    outcome.exportMs = Math.round(comparing - started);
    outcome.unprotected = left.unprotected;
    outcome.leftOut = left.losses.map((loss) => `${loss.path} (${loss.reason})`);
    return outcome;
  } finally {
    document.free();
  }
}

describe.skipIf(!FOLDER)('real documents saved unchanged from the editor', () => {
  it(
    'ask about what the save takes away, and the comparison never stands in the way',
    async () => {
      const folder = FOLDER ?? '';
      expect(statSync(folder).isDirectory(), folder).toBe(true);
      startRhwp();
      const outcomes: Outcome[] = [];
      for (const file of samplesUnder(folder)) {
        const outcome = await tryOne(file);
        outcomes.push({ ...outcome, file: path.relative(folder, file) });
      }
      const saved = outcomes.filter((one) => one.skipped === undefined);
      const asked = saved.filter(
        (one) => one.unprotected === true || (one.leftOut?.length ?? 0) > 0,
      );
      const slowest = [...saved]
        .sort((one, other) => (other.compareMs ?? 0) - (one.compareMs ?? 0))
        .slice(0, 5);
      const skipped = new Map<string, number>();
      for (const one of outcomes) {
        if (one.skipped !== undefined)
          skipped.set(one.skipped, (skipped.get(one.skipped) ?? 0) + 1);
      }
      const byFormat = (format: HwpFormat) => saved.filter((one) => one.format === format).length;
      const summary = {
        files: outcomes.length,
        saved: { hwp: byFormat('hwp'), hwpx: byFormat('hwpx') },
        skipped: Object.fromEntries([...skipped].sort()),
        reportedLosses: saved.filter((one) => (one.reported ?? 0) > 0).map((one) => one.file),
        unprotected: saved.filter((one) => one.unprotected === true).map((one) => one.file),
        leftOut: saved
          .filter((one) => (one.leftOut?.length ?? 0) > 0)
          .map((one) => ({ file: one.file, unprotected: one.unprotected, leftOut: one.leftOut })),
        compareFailed: saved
          .filter((one) => one.compareFailed !== undefined)
          .map((one) => one.file),
        asked: asked.length,
        notAsked: saved.length - asked.length,
        slowestComparisons: slowest.map(({ file, bytes, exportMs, compareMs }) => ({
          file,
          bytes,
          exportMs,
          compareMs,
        })),
      };
      if (REPORT) writeFileSync(REPORT, `${JSON.stringify({ summary, outcomes }, null, 2)}\n`);
      // eslint-disable-next-line no-console -- run by hand: the counts are what it is run for.
      console.log(JSON.stringify(summary, null, 2));
      expect(saved.length, 'the folder holds documents the editor saves').toBeGreaterThan(0);
      expect(summary.compareFailed, 'saves the comparison cannot read').toEqual([]);
    },
    2 * 60 * 60 * 1000,
  );
});

/**
 * Writes the tables of the two documents about the messages from what the
 * conversion reports, so that no text in them is copied by hand.
 *
 *   node tools/i18n/review.ts           write the generated parts
 *   node tools/i18n/review.ts --check   fail when they are not what would be written
 *
 * A generated part of a document is what stands between
 * `<!-- generated:<name> -->` and `<!-- /generated:<name> -->`. Everything
 * else in the documents is written by hand and left alone.
 *
 * - `docs/ko-review.md`: every Korean text that is not the original's (the
 *   overrides and the filled translations), for the person who reviews them.
 * - `docs/i18n-conversion.md`: the overrides of the other locales.
 *
 * `ko-review.json` holds what the conversion cannot know: the rows of the
 * table of odd Korean texts the review started from, and the first wording of
 * the filled translations that were changed since.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';

import type * as Prettier from 'prettier';

import { convertAll, type Outcome, OVERRIDE_KINDS, type OverrideKind } from './conversion.ts';
import notes from './ko-review.json' with { type: 'json' };
import { BASE_LOCALE, ORIGINAL_ROOT, WEB_ROOT } from './locales.ts';

// Loaded the way Node resolves it. Imported as a module, the test runner
// would hand over the build for browsers, which cannot read the configuration.
const { format, resolveConfig } = createRequire(import.meta.url)('prettier') as typeof Prettier;

const KIND_LABELS: Record<OverrideKind, string> = {
  name: '제품 이름',
  rewrite: '다시 씀',
  typo: '오타',
  mistranslation: '오역',
  omission: '빠진 말',
  term: '용어 통일',
  form: '명사구',
  tone: '어조',
  particle: '조사',
  punctuation: '문장부호·띄어쓰기',
};

/** A message text as the content of a table cell. */
function cell(text: string): string {
  return text.replace(/[\\|<*_~`[]/g, '\\$&');
}

function table(head: string[], rows: string[][]): string {
  const line = (cells: string[]): string => `| ${cells.join(' | ')} |`;
  return [line(head), line(head.map(() => '---')), ...rows.map(line)].join('\n');
}

const kindsOf = (kinds: OverrideKind[]): string =>
  kinds.map((kind) => KIND_LABELS[kind]).join(', ');
const firstKind = (kinds: OverrideKind[]): number =>
  Math.min(...kinds.map((kind) => OVERRIDE_KINDS.indexOf(kind)));

/** The generated parts of `docs/ko-review.md`, by name. */
export function koReview({ report, texts }: Outcome): Record<string, string> {
  const korean = report.locales.find((locale) => locale.tag === 'ko');
  const english = texts.get(BASE_LOCALE);
  const now = texts.get('ko');
  if (!korean || !english || !now) throw new Error('no Korean or no English messages');
  const overridden = new Map(korean.overridden.map((entry) => [entry.id, entry]));
  const removed = new Set(report.removed.map(({ id }) => id));
  const unchanged = new Set(notes.unchanged);
  const order = new Map(report.kept.map((id, at) => [id, at]));
  const englishOf = (id: string): string => cell(english[id] ?? '');

  // What became of each row of the table the review started from.
  type State = '고침' | '그대로' | '제거됨';
  const rowOf = new Map<string, number>();
  const states = notes.listed.map((ids, at) =>
    ids.map((id): { id: string; state: State } => {
      if (rowOf.has(id)) throw new Error(`ko-review.json: ${id} is listed twice`);
      rowOf.set(id, at + 1);
      const found = [overridden.has(id), unchanged.has(id), removed.has(id)];
      if (found.filter(Boolean).length !== 1) {
        throw new Error(`ko-review.json: ${id} has to be overridden, unchanged or removed`);
      }
      return { id, state: found[0] ? '고침' : found[1] ? '그대로' : '제거됨' };
    }),
  );
  for (const id of unchanged) {
    if (!rowOf.has(id) || !(id in now)) throw new Error(`ko-review.json: unchanged ${id}?`);
  }
  const listed = states.flat();
  const count = (state: State): number => listed.filter((one) => one.state === state).length;
  const rowsWith = (test: (row: State[]) => boolean): number =>
    states.filter((row) => test(row.map((one) => one.state))).length;
  const rowsFixed = rowsWith((row) => row.includes('고침'));
  const rowsPartly = rowsWith((row) => row.includes('고침') && row.includes('그대로'));
  const rowsGone = rowsWith((row) => row.every((state) => state === '제거됨'));
  if (rowsFixed + rowsGone !== states.length) {
    throw new Error('ko-review.json: a row is neither fixed nor gone');
  }

  // The overrides outside that table, by the decision they come from.
  const outside = korean.overridden.filter(({ id }) => !rowOf.has(id));
  const named = outside.filter(({ kind }) => kind.includes('name'));
  const rewritten = outside.filter(({ kind }) => kind.includes('rewrite'));
  const unified = outside
    .filter((entry) => !named.includes(entry) && !rewritten.includes(entry))
    .sort(
      (a, b) =>
        firstKind(a.kind) - firstKind(b.kind) || (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0),
    );

  const firstFilled: Record<string, string | undefined> = notes.firstFilled;
  for (const [id, first] of Object.entries(firstFilled)) {
    if (!korean.filled.includes(id) || now[id] === first) {
      throw new Error(`ko-review.json: firstFilled ${id} is not a filled text changed since`);
    }
  }

  const rows: string[][] = [];
  const add = (group: string, kind: string, id: string, was: string, text: string): void => {
    rows.push([String(rows.length + 1), group, kind, `\`${id}\``, englishOf(id), was, cell(text)]);
  };
  for (const { id } of listed) {
    const entry = overridden.get(id);
    if (entry) {
      add(`원 표 ${rowOf.get(id) ?? 0}행`, kindsOf(entry.kind), id, cell(entry.was), entry.now);
    }
  }
  for (const [group, entries] of [
    ['결정 1', named],
    ['결정 5', rewritten],
    ['통일', unified],
  ] as const) {
    for (const entry of entries) {
      add(group, kindsOf(entry.kind), entry.id, cell(entry.was), entry.now);
    }
  }
  for (const id of korean.filled) {
    const first = firstFilled[id];
    const was = first === undefined ? '(없음)' : `(없음) 처음 채운 값: ${cell(first)}`;
    add('채움', first === undefined ? '새로 채움' : '새로 채움, 명사구', id, was, now[id] ?? '');
  }

  const changedFills = Object.keys(firstFilled).length;
  // Rows that are there because of the product's name, which a decision of
  // its own covers.
  const rowsNamed = states.filter((row) =>
    row.some(({ id }) => overridden.get(id)?.kind.includes('name')),
  ).length;
  const counts = [
    `- 원 표("이상해 보이는 기존 문자열")는 ${states.length}행이고, 그 행들에 적힌 id는 ${listed.length}개다.`,
    `- 행으로 세면: 고친 행 ${rowsFixed}개(그 가운데 ${rowsPartly}개는 행의 id 일부만 고쳤다), 메시지가 제거되어 해당 없는 행 ${rowsGone}개. ${rowsFixed} + ${rowsGone} = ${states.length}.`,
    `- 고친 행 가운데 ${rowsNamed}개는 제품 이름(결정 1) 때문에 고친 행이다. 그 행을 따로 세면 나머지는 ${states.length - rowsNamed}행이고, 그 가운데 고친 행이 ${rowsFixed - rowsNamed}개, 해당 없는 행이 ${rowsGone}개다.`,
    `- id로 세면: 고친 것 ${count('고침')}개, 그대로 둔 것 ${count('그대로')}개, 메시지가 제거된 것 ${count('제거됨')}개. ${count('고침')} + ${count('그대로')} + ${count('제거됨')} = ${listed.length}.`,
    `- 원 표 밖에서 더 고친 id는 ${outside.length}개다: 제품 이름(결정 1) ${named.length}개, 영어를 다시 쓴 메시지(결정 5) ${rewritten.length}개, 용어·꼴·어조의 통일과 조사 때문에 ${unified.length}개.`,
    `- 덮어쓴 한국어 메시지는 모두 ${korean.overridden.length}개다(${count('고침')} + ${outside.length}).`,
    `- 새로 채운 것은 ${korean.filled.length}개이고, 그 가운데 ${changedFills}개는 처음 채운 값을 통일 때문에 고쳤다.`,
    `- 아래 표는 ${rows.length}행이다(${korean.overridden.length} + ${korean.filled.length}).`,
  ].join('\n');

  const rowTable = table(
    ['행', 'id와 결과', '행의 결과'],
    states.map((row, at) => {
      const all = [...new Set(row.map((one) => one.state))];
      return [
        String(at + 1),
        row.map(({ id, state }) => `\`${id}\` ${state}`).join('<br>'),
        all.length === 1 ? (all[0] ?? '') : '일부 고침',
      ];
    }),
  );

  return {
    counts,
    rows: rowTable,
    table: table(['#', '묶음', '구분', 'id', '영어', '이전', '이후'], rows),
  };
}

/** The groups of `removed.json` that were left out by a later decision. */
const REMOVED_BY_DECISION = ['guest-accounts', 'old-notice', 'upgrade-tour-step'];

/** The generated parts of `docs/i18n-conversion.md`, by name. */
export function conversionTables({ report }: Outcome): Record<string, string> {
  // Nothing here counts the app's own messages: other work adds to those, and
  // the document must not go stale with them.
  const kept = new Set(report.kept);
  const stats = report.locales.map((locale) => [
    `\`${locale.tag}\``,
    `\`${locale.file}.json\``,
    locale.original,
    locale.converted,
    locale.overridden.length,
    locale.notConverted.length,
    locale.stale.length,
    locale.removed,
    locale.dead,
    locale.filled.length,
    locale.missing.filter((id) => kept.has(id)).length,
  ]);
  const originalEnglish = JSON.parse(
    readFileSync(path.join(ORIGINAL_ROOT, 'i18n/en.json'), 'utf8'),
  ) as Record<string, string>;
  const removed = report.removed
    .filter(({ reason }) => REMOVED_BY_DECISION.includes(reason))
    .map(({ id, usedIn }) => [
      `\`${id}\``,
      cell(originalEnglish[id] ?? ''),
      usedIn.map((file) => `\`${file.replace(/^webapp\/src\//, '')}\``).join(', '),
    ]);
  const rows = report.locales
    .filter((locale) => locale.tag !== 'ko')
    .flatMap((locale) =>
      locale.overridden.map((entry) => [
        `\`${locale.tag}\``,
        `\`${entry.id}\``,
        kindsOf(entry.kind) + (entry.stale ? ' (번역을 내지 않음)' : ''),
        cell(entry.was),
        cell(entry.now),
      ]),
    );
  const stale = report.locales
    .filter((locale) => locale.stale.length > 0)
    .map((locale) => [`\`${locale.tag}\``, locale.stale.map((id) => `\`${id}\``).join(', ')]);
  return {
    stats: table(
      [
        '로케일',
        '원본 파일',
        '원본 메시지',
        '변환',
        '그 가운데 덮어씀',
        '변환 안 함',
        '영어가 바뀌어 내지 않음',
        '제거: 미포팅',
        '제거: 죽은 id',
        '채움',
        '남긴 id 가운데 빠짐',
      ],
      stats.map((row) => row.map(String)),
    ),
    'removed-by-decision': table(['id', '영어 문구', '쓰던 파일 (`webapp/src/` 아래)'], removed),
    overrides: table(['로케일', 'id', '구분', '이전', '이후'], rows),
    stale: table(['로케일', '번역을 내지 않은 id'], stale),
  };
}

const DOCUMENTS: [string, (outcome: Outcome) => Record<string, string>][] = [
  ['docs/ko-review.md', koReview],
  ['docs/i18n-conversion.md', conversionTables],
];

/** Path under the web app → the document with its generated parts written anew. */
export async function reviewDocuments(
  outcome: Outcome = convertAll(),
): Promise<Map<string, string>> {
  const documents = new Map<string, string>();
  for (const [name, generate] of DOCUMENTS) {
    const file = path.join(WEB_ROOT, name);
    let text = readFileSync(file, 'utf8');
    for (const [part, content] of Object.entries(generate(outcome))) {
      const open = `<!-- generated:${part} -->`;
      const close = `<!-- /generated:${part} -->`;
      const start = text.indexOf(open);
      const end = text.indexOf(close);
      if (start === -1 || end < start) throw new Error(`${name}: no part "${part}"`);
      text = `${text.slice(0, start + open.length)}\n\n${content}\n\n${text.slice(end)}`;
    }
    documents.set(name, await format(text, { ...(await resolveConfig(file)), filepath: file }));
  }
  return documents;
}

if (import.meta.main) {
  const mode = process.argv[2];
  if (mode !== undefined && mode !== '--check') {
    console.error(`unknown argument ${mode}`);
    process.exit(2);
  }
  const documents = await reviewDocuments();
  if (mode === '--check') {
    const stale = [...documents].filter(
      ([name, content]) => readFileSync(path.join(WEB_ROOT, name), 'utf8') !== content,
    );
    if (stale.length > 0) {
      console.error(
        `generated parts differ from the messages:\n  ${stale.map(([name]) => name).join('\n  ')}\n` +
          'Run "node tools/i18n/review.ts".',
      );
      process.exit(1);
    }
    console.log(`review documents: up to date (${documents.size})`);
  } else {
    for (const [name, content] of documents) writeFileSync(path.join(WEB_ROOT, name), content);
    console.log(`wrote ${[...documents.keys()].join(', ')}`);
  }
}

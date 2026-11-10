/**
 * The built-in templates in the form the new server takes them in: per
 * template a board, its views and its cards, each card with its body as one
 * document, plus the list of what the conversion changed in the texts.
 *
 * PROVISIONAL TYPES. The server's contract (`contracts/openapi.json`) does
 * not exist yet. The shapes below follow the server design (its section 5.7,
 * "the update of 3.10": board, blocks, body) and what the archive holds; when
 * the contract is published they are to be replaced by the types generated
 * from it, and the output regenerated.
 *
 * Identifiers are the ones of the archive. They only tie the parts of one
 * template together (a view's card order, a card's option); the server gives
 * everything new identifiers when it takes a template in.
 */
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';

import { validateDoc, type DocNode } from '../../src/lib/editor/schema.ts';
import { dateOfUtc } from '../../src/lib/logic/dates.ts';
import { formatDateValue, type StoredDateValue } from '../../src/lib/logic/wire.ts';

import { ARCHIVE, cardBody, readArchive, textOf, type ArchivedBlock } from './archive.ts';
import {
  hasOldName,
  maskOldNames,
  removeOutsideLinks,
  rename,
  renameInDoc,
  valueLeadsOutside,
  type Change,
  type ChangeRule,
} from './rewrite.ts';

/** The version of the archive format this output is the starting point of. */
export const FORMAT = 3;

export interface TemplateOption {
  id: string;
  value: string;
  color: string;
}

export interface TemplateProperty {
  id: string;
  name: string;
  type: string;
  options: TemplateOption[];
}

export interface TemplateBoard {
  id: string;
  title: string;
  description: string;
  icon: string;
  showDescription: boolean;
  cardProperties: TemplateProperty[];
}

export interface TemplateView {
  id: string;
  type: 'view';
  title: string;
  /** The members of a view as the original stores them: view type, grouping, order, filter, ... */
  fields: Record<string, unknown>;
}

export type TemplatePropertyValue = string | string[] | StoredDateValue;

export interface TemplateCard {
  id: string;
  type: 'card';
  title: string;
  fields: {
    icon: string;
    /** A card template of the board: offered when a card is added, not a card of its own. */
    isTemplate: boolean;
    properties: Record<string, TemplatePropertyValue>;
  };
  /** The body, one document; null when the card has none. */
  body: DocNode | null;
}

export interface TemplateFileReference {
  /** The identifier image nodes name; in the archive it is the name of the file. */
  fileId: string;
  /** Where the bytes are: the member of the original archive. They are not copied here. */
  archiveMember: string;
  bytes: number;
}

export interface Template {
  /** A name for the template that does not change with its title. */
  key: string;
  /**
   * The board a new user is given to learn on. The original found it by its
   * title; the title is one of the texts that changed, so it is marked here.
   */
  onboarding: boolean;
  board: TemplateBoard;
  views: TemplateView[];
  cards: TemplateCard[];
  /** The files the bodies of the cards show. */
  files: TemplateFileReference[];
}

/** What was left as it is but wants a look when the wording is reviewed. */
export interface ReviewNote {
  template: string;
  kind:
    /** An example address at a domain that exists. */
    | 'example-email'
    /** A picture from the archive; many show the original's screen. */
    | 'image'
    /** A text about something the original's sister products do: channels, direct messages. */
    | 'other-product';
  where: string;
  text: string;
}

export interface Output {
  index: {
    format: number;
    source: { path: string; sha256: string; bytes: number };
    templates: {
      key: string;
      file: string;
      title: string;
      onboarding: boolean;
      views: number;
      cards: number;
      bodies: number;
      files: number;
    }[];
    conversions: Record<string, number>;
    changes: Record<ChangeRule, number>;
    reviewNotes: Record<ReviewNote['kind'], number>;
  };
  templates: Template[];
  changes: Change[];
  reviewNotes: ReviewNote[];
}

/** The standalone product word was read in each of these places and is the product there. */
const REVIEWED_PRODUCT_WORDS = 5;

const KEY_OF_ONBOARDING = 'welcome';

const slug = (title: string): string =>
  title
    .toLowerCase()
    .replace(/&/g, ' ')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '');

/** The title the original shows: it drops a marker left in the archive. */
const cleanTitle = (title: string): string => title.replace('(NEW)', '').trim();

/**
 * A date value as the archive has it: the text of a JSON object with `from`
 * and `to` in milliseconds. Without `includeTime` each stands for a day, the
 * day those milliseconds fall on in UTC.
 */
function convertDateValue(value: unknown): StoredDateValue | undefined {
  if (typeof value !== 'string' || value === '') return undefined;
  let stored: { from?: unknown; to?: unknown; includeTime?: unknown };
  try {
    stored = JSON.parse(value) as typeof stored;
  } catch {
    return undefined;
  }
  const point = (milliseconds: unknown) => {
    if (typeof milliseconds !== 'number' || !Number.isFinite(milliseconds)) return undefined;
    return stored.includeTime === true
      ? { instant: milliseconds }
      : { date: dateOfUtc(milliseconds) };
  };
  const from = point(stored.from);
  const to = point(stored.to);
  return formatDateValue(from ? (to ? { from, to } : { from }) : {});
}

const word = (value: unknown): string => (typeof value === 'string' ? value : '');

function readProperties(raw: readonly unknown[]): TemplateProperty[] {
  return raw.map((entry) => {
    const property = entry as Record<string, unknown>;
    const options = Array.isArray(property.options) ? (property.options as unknown[]) : [];
    return {
      id: word(property.id),
      name: word(property.name),
      type: word(property.type) || 'text',
      options: options.map((item) => {
        const option = item as Record<string, unknown>;
        return { id: word(option.id), value: word(option.value), color: word(option.color) };
      }),
    };
  });
}

/** Converts the archive. Throws when the result is not what the rules promise. */
export function convertTemplates(file: string = ARCHIVE): Output {
  const source = readFileSync(file);
  const changes: Change[] = [];
  const reviewNotes: ReviewNote[] = [];
  const conversions = {
    /** Content blocks that became part of a body. */
    contentBlocks: 0,
    /** Date values rewritten from milliseconds in text to the new shape. */
    dateValues: 0,
    /** Identifiers in a card order that name no card of the template. */
    danglingCardOrder: 0,
    /** Identifiers in a content order that name no block. */
    missingContentBlocks: 0,
    /** Content blocks whose card is not in the archive. */
    orphanContentBlocks: 0,
    /** Titles that lost the marker `(NEW)` or outer spaces. */
    cleanedTitles: 0,
  };
  let productWords = 0;
  const templates: Template[] = [];

  for (const archived of readArchive(file)) {
    const originalTitle = cleanTitle(archived.board.title);
    if (originalTitle !== archived.board.title) conversions.cleanedTitles += 1;
    const onboarding = originalTitle.startsWith('Welcome to ');
    const key = onboarding ? KEY_OF_ONBOARDING : slug(rename(originalTitle).text);

    const record = (
      part: string,
      title: string,
      field: string,
      rule: ChangeRule,
      before: string,
      after: string,
    ): void => {
      changes.push({
        template: key,
        part,
        title,
        field,
        rule,
        before: maskOldNames(before),
        after,
      });
    };
    /**
     * Replaces the names in one text and records it. `title` names the part
     * the text belongs to; without it the text is itself that title.
     */
    const renamed = (
      part: string,
      title: string | undefined,
      field: string,
      text: string,
    ): string => {
      const result = rename(text);
      productWords += result.productWords;
      for (const rule of result.rules) {
        record(part, title ?? result.text, field, rule, text, result.text);
      }
      return result.text;
    };
    const note = (kind: ReviewNote['kind'], where: string, text: string): void => {
      reviewNotes.push({ template: key, kind, where, text: maskOldNames(text) });
    };

    // The board.
    const boardTitle = renamed('board', undefined, 'title', originalTitle);
    const board: TemplateBoard = {
      id: archived.board.id,
      title: boardTitle,
      description: renamed('board', boardTitle, 'description', archived.board.description),
      icon: archived.board.icon,
      showDescription: archived.board.showDescription,
      cardProperties: readProperties(archived.board.cardProperties).map((property) => {
        const name = renamed('property', undefined, 'name', property.name);
        return {
          ...property,
          name,
          options: property.options.map((option) => {
            const value = renamed('option', name, 'value', option.value);
            return { ...option, value };
          }),
        };
      }),
    };

    // Cards, each with its body.
    const byId = new Map(archived.blocks.map((block) => [block.id, block]));
    conversions.orphanContentBlocks += archived.blocks.filter(
      (block) =>
        !['card', 'view', 'comment'].includes(block.type) &&
        byId.get(block.parentId)?.type !== 'card',
    ).length;

    const files = new Map<string, TemplateFileReference>();
    const cards: TemplateCard[] = [];
    for (const block of archived.blocks.filter((candidate) => candidate.type === 'card')) {
      const title = renamed('card', undefined, 'title', cleanTitle(block.title));
      if (cleanTitle(block.title) !== block.title) conversions.cleanedTitles += 1;

      const properties: Record<string, TemplatePropertyValue> = {};
      const stored = (block.fields.properties ?? {}) as Record<string, unknown>;
      for (const [propertyId, value] of Object.entries(stored)) {
        const property = board.cardProperties.find((candidate) => candidate.id === propertyId);
        const field = `value of "${property?.name ?? propertyId}"`;
        if (property?.type === 'date') {
          const converted = convertDateValue(value);
          if (converted) {
            properties[propertyId] = converted;
            conversions.dateValues += 1;
          }
          continue;
        }
        if (Array.isArray(value)) {
          properties[propertyId] = value.map(String);
          continue;
        }
        if (typeof value !== 'string' || value === '') continue;
        if (property?.type === 'url') {
          if (valueLeadsOutside(value)) {
            record('card', title, field, 'outside-link-value', value, '');
            continue;
          }
        }
        // Identifiers of options and people are not texts; everything else may name the original.
        const isText = !['select', 'person', 'createdBy', 'updatedBy'].includes(
          property?.type ?? '',
        );
        const text = isText ? renamed('card', title, field, value) : value;
        if (property?.type === 'email') note('example-email', `${title} / ${field}`, text);
        properties[propertyId] = text;
      }

      const converted = cardBody(block, archived.blocks);
      conversions.contentBlocks += Object.values(converted.blocks).reduce((a, b) => a + b, 0);
      conversions.missingContentBlocks += converted.missingBlocks;
      let body: DocNode | null = null;
      if (converted.doc) {
        const pruned = removeOutsideLinks(converted.doc);
        for (const { before, after } of pruned.removed) {
          record('card', title, 'body', 'outside-link', before, rename(after).text);
        }
        if (pruned.doc) {
          const result = renameInDoc(pruned.doc);
          productWords += result.productWords;
          for (const { before, after, rules } of result.renamed) {
            for (const rule of rules) record('card', title, 'body', rule, before, after);
          }
          body = result.doc;
        }
      }
      if (body) {
        const problems = validateDoc(body);
        if (problems.length > 0) {
          throw new Error(`${board.title} / ${title}: ${problems.join('; ')}`);
        }
        const paragraphs = (node: DocNode): DocNode[] =>
          node.type === 'paragraph' || node.type === 'heading'
            ? [node]
            : (node.content ?? []).flatMap(paragraphs);
        for (const paragraph of paragraphs(body)) {
          const text = textOf(paragraph);
          if (/\bChannels?\b|Direct Message/.test(text)) {
            note('other-product', `${title} / body`, text);
          }
        }
        const images = (node: DocNode): string[] =>
          node.type === 'image'
            ? [String(node.attrs?.fileId)]
            : (node.content ?? []).flatMap(images);
        for (const fileId of images(body)) {
          const file = archived.files.find((candidate) => candidate.name === fileId);
          if (!file) throw new Error(`${board.title} / ${title}: no file ${fileId} in the archive`);
          files.set(fileId, {
            fileId,
            archiveMember: `${archived.folder}/${file.name}`,
            bytes: file.bytes,
          });
          note('image', `${title} / body`, fileId);
        }
      }

      cards.push({
        id: block.id,
        type: 'card',
        title,
        fields: {
          icon: typeof block.fields.icon === 'string' ? block.fields.icon : '',
          isTemplate: block.fields.isTemplate === true,
          properties,
        },
        body,
      });
    }

    // Views.
    const cardIds = new Set(cards.map((card) => card.id));
    const views: TemplateView[] = archived.blocks
      .filter((block: ArchivedBlock) => block.type === 'view')
      .map((block) => {
        const title = renamed('view', undefined, 'title', cleanTitle(block.title));
        const fields = { ...block.fields };
        if (Array.isArray(fields.cardOrder)) {
          const order = (fields.cardOrder as unknown[]).filter(
            (id): id is string => typeof id === 'string',
          );
          const known = order.filter((id) => cardIds.has(id));
          conversions.danglingCardOrder += order.length - known.length;
          fields.cardOrder = known;
        }
        if (typeof fields.defaultTemplateId === 'string' && fields.defaultTemplateId !== '') {
          if (!cardIds.has(fields.defaultTemplateId)) fields.defaultTemplateId = '';
        }
        return { id: block.id, type: 'view', title, fields };
      });

    templates.push({ key, onboarding, board, views, cards, files: [...files.values()] });
  }

  // What the rules promise, checked on the whole result.
  const everything = JSON.stringify(templates);
  if (hasOldName(everything)) throw new Error('a name of the original is left in the output');
  if (/https?:\/\/|\bwww\./i.test(everything)) {
    throw new Error('an address that leads outside is left in the output');
  }
  if (productWords !== REVIEWED_PRODUCT_WORDS) {
    throw new Error(
      `the product word was replaced ${productWords} times, not the ${REVIEWED_PRODUCT_WORDS} ` +
        'that were read and found to mean the product: read the new places, then change the number',
    );
  }
  const keys = new Set(templates.map((template) => template.key));
  if (keys.size !== templates.length) throw new Error('two templates share a key');
  if (templates.filter((template) => template.onboarding).length !== 1) {
    throw new Error('there is not exactly one onboarding template');
  }

  const count = <Kind extends string>(kinds: readonly Kind[], of: readonly Kind[]) =>
    Object.fromEntries(
      kinds.map((kind) => [kind, of.filter((item) => item === kind).length]),
    ) as Record<Kind, number>;

  return {
    index: {
      format: FORMAT,
      source: {
        path: 'server/assets/templates.boardarchive',
        sha256: createHash('sha256').update(source).digest('hex'),
        bytes: source.length,
      },
      templates: templates.map((template, index) => ({
        key: template.key,
        file: templateFileName(index, template.key),
        title: template.board.title,
        onboarding: template.onboarding,
        views: template.views.length,
        cards: template.cards.length,
        bodies: template.cards.filter((card) => card.body !== null).length,
        files: template.files.length,
      })),
      conversions,
      changes: count(
        ['old-name', 'product-word', 'outside-link', 'outside-link-value'],
        changes.map((change) => change.rule),
      ),
      reviewNotes: count(
        ['example-email', 'image', 'other-product'],
        reviewNotes.map((item) => item.kind),
      ),
    },
    templates,
    changes,
    reviewNotes,
  };
}

export const templateFileName = (index: number, key: string): string =>
  `${String(index + 1).padStart(2, '0')}-${key}.json`;

const cell = (text: string): string =>
  text === '' ? '(없음)' : text.replace(/\|/g, '\\|').replace(/\n/g, '<br>');

const RULE_NAMES: Record<ChangeRule, string> = {
  'old-name': '원본 이름 → fvoci',
  'product-word': '제품을 가리키는 "Boards" → fvoci',
  'outside-link': '바깥 링크와 그 문장을 뺌',
  'outside-link-value': '바깥 링크인 속성 값을 뺌',
};

const NOTE_NAMES: Record<ReviewNote['kind'], string> = {
  'example-email': '예시 이메일 주소(실제로 있는 도메인)',
  image: '그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다)',
  'other-product': '원본 회사의 다른 제품(채널, 다이렉트 메시지)을 말하는 글',
};

/** The list of changes as a page for the person who reviews the wording. */
export function changesPage(output: Output): string {
  const { index, changes, reviewNotes } = output;
  const lines: string[] = [
    '# 내장 템플릿: 변환이 바꾼 곳',
    '',
    '`node tools/templates/convert.ts --write tools/templates/output`이 쓴 파일이다. 손으로 고치지 않는다.',
    '',
    '원본의 내장 템플릿 13개를 새 형식으로 바꾸면서 글에서 바꾼 곳을 모두 적는다. 규칙은 넷이다. 글을 고쳐 쓰지는 않았다(검수는 나중에 사람이 한다). 이전 칸의 `{old product name}`, `{old company name}`은 원본의 두 이름이 있던 자리다(이 폴더에는 그 이름을 적지 않는다).',
    '',
    '| 규칙 | 건수 |',
    '| --- | --- |',
    ...(Object.keys(RULE_NAMES) as ChangeRule[]).map(
      (rule) => `| ${RULE_NAMES[rule]} | ${index.changes[rule]} |`,
    ),
    `| 합계 | ${changes.length} |`,
    '',
    '- **제품을 가리키는 "Boards"**는 결정문의 글자 그대로("원본 이름을 바꾼다")보다 한 걸음 나간 것이다. 번역에서 같은 낱말을 제품 이름일 때 fvoci로 바꾸기로 한 결정을 따랐다. 다섯 곳을 읽고 모두 제품을 가리킨다고 판단했다. 그대로 두려면 `rewrite.ts`의 규칙 하나를 끄면 된다.',
    '- 기계적으로 바꾼 결과가 어색한 곳이 있다(예: `fvoci plugin`, 한 문장에 fvoci가 두 번). 고쳐 쓰지 않고 그대로 두었다.',
    '',
  ];
  for (const template of index.templates) {
    const own = changes.filter((change) => change.template === template.key);
    lines.push(`## ${template.title} (\`${template.file}\`)`, '');
    if (own.length === 0) {
      lines.push('바뀐 곳이 없다.', '');
      continue;
    }
    lines.push('| 어디 | 무엇 | 규칙 | 이전 | 이후 |', '| --- | --- | --- | --- | --- |');
    for (const change of own) {
      lines.push(
        `| ${change.part}: ${cell(change.title)} | ${cell(change.field)} | ${RULE_NAMES[change.rule]} | ${cell(change.before)} | ${cell(change.after)} |`,
      );
    }
    lines.push('');
  }
  lines.push(
    '## 바꾸지 않았지만 검수할 때 볼 것',
    '',
    '규칙에 걸리지 않아 그대로 둔 것이다.',
    '',
    '| 종류 | 건수 |',
    '| --- | --- |',
    ...(Object.keys(NOTE_NAMES) as ReviewNote['kind'][]).map(
      (kind) => `| ${NOTE_NAMES[kind]} | ${index.reviewNotes[kind]} |`,
    ),
    '',
    '| 템플릿 | 종류 | 어디 | 글 |',
    '| --- | --- | --- | --- |',
    ...reviewNotes.map(
      (item) =>
        `| ${item.template} | ${NOTE_NAMES[item.kind]} | ${cell(item.where)} | ${cell(item.text)} |`,
    ),
    '',
  );
  return lines.join('\n');
}

/** Every file of the output, by name. The same input gives the same bytes. */
export function outputFiles(output: Output): Map<string, string> {
  const json = (value: unknown): string => `${JSON.stringify(value, null, 2)}\n`;
  const files = new Map<string, string>();
  files.set('index.json', json(output.index));
  output.templates.forEach((template, index) => {
    files.set(templateFileName(index, template.key), json(template));
  });
  files.set('changes.json', json({ changes: output.changes, reviewNotes: output.reviewNotes }));
  files.set('CHANGES.md', changesPage(output));
  return files;
}

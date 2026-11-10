/** Made-up data for the editor spike: people, emoji, files and two documents. */
import type { SuggestionEntry } from '$lib/editor/extensions.ts';
import type { DocNode } from '$lib/editor/schema.ts';

export const USERS: SuggestionEntry[] = [
  { value: 'user-ada', label: 'Ada Lovelace' },
  { value: 'user-kim', label: '김하늘' },
  { value: 'user-sato', label: '佐藤 花子' },
  { value: 'user-lee', label: '이도윤' },
];

export const EMOJI: SuggestionEntry[] = [
  ['smile', '😄'],
  ['thumbsup', '👍'],
  ['heart', '❤️'],
  ['tada', '🎉'],
  ['rocket', '🚀'],
  ['eyes', '👀'],
  ['fire', '🔥'],
  ['warning', '⚠️'],
  ['white_check_mark', '✅'],
  ['x', '❌'],
].map(([name, emoji]) => ({ value: emoji ?? '', label: `${emoji} :${name}:` }));

/** A picture for a file identifier: the spike has no server to fetch one from. */
export function fakeFileUrl(fileId: string): string | undefined {
  if (!fileId.startsWith('file-')) return undefined;
  let hue = 0;
  for (let index = 0; index < fileId.length; index += 1)
    hue = (hue + fileId.charCodeAt(index)) % 360;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="480" height="160">` +
    `<rect width="480" height="160" fill="hsl(${hue} 60% 80%)"/>` +
    `<text x="240" y="88" text-anchor="middle" font-size="24" font-family="sans-serif">${fileId}</text></svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

const text = (value: string, ...marks: string[]): DocNode =>
  marks.length > 0
    ? { type: 'text', text: value, marks: marks.map((type) => ({ type })) }
    : { type: 'text', text: value };
const paragraph = (...content: DocNode[]): DocNode => ({ type: 'paragraph', content });
const item = (...content: DocNode[]): DocNode => ({ type: 'listItem', content });
const task = (checked: boolean, label: string): DocNode => ({
  type: 'taskItem',
  attrs: { checked },
  content: [paragraph(text(label))],
});

export const SAMPLES: Record<string, DocNode> = {
  empty: { type: 'doc', content: [{ type: 'paragraph' }] },
  everything: {
    type: 'doc',
    content: [
      { type: 'heading', attrs: { level: 1 }, content: [text('출시 준비 회의')] },
      paragraph(
        text('다음 주 '),
        text('목요일', 'bold'),
        text('까지 끝낼 일을 정리했습니다. 담당은 '),
        { type: 'mention', attrs: { userId: 'user-kim' } },
        text(' 님, 검토는 '),
        { type: 'mention', attrs: { userId: 'user-ada' } },
        text(' 님입니다.'),
      ),
      { type: 'heading', attrs: { level: 2 }, content: [text('할 일')] },
      {
        type: 'taskList',
        content: [
          task(true, '번역 검수'),
          task(false, '설치 문서 고치기'),
          task(false, 'Release notes'),
        ],
      },
      { type: 'heading', attrs: { level: 2 }, content: [text('Notes')] },
      {
        type: 'bulletList',
        content: [
          item(
            paragraph(
              text('A list entry with '),
              text('italic', 'italic'),
              text(' and '),
              text('struck', 'strike'),
              text(' text.'),
            ),
          ),
          item(paragraph(text('두 번째 항목')), {
            type: 'orderedList',
            attrs: { start: 1 },
            content: [item(paragraph(text('안쪽 하나'))), item(paragraph(text('안쪽 둘')))],
          }),
        ],
      },
      { type: 'blockquote', content: [paragraph(text('인용한 문장은 이렇게 보입니다.'))] },
      {
        type: 'codeBlock',
        attrs: { language: 'ts' },
        content: [text('const answer: number = 42; // 주석')],
      },
      paragraph(
        text('Inline '),
        text('code', 'code'),
        text(' and a '),
        {
          type: 'text',
          text: 'link',
          marks: [{ type: 'link', attrs: { href: '/board/example' } }],
        },
        text('.'),
        { type: 'hardBreak' },
        text('A second line in the same paragraph.'),
      ),
      { type: 'horizontalRule' },
      { type: 'image', attrs: { fileId: 'file-chart', alt: '진행 그래프' } },
    ],
  },
};

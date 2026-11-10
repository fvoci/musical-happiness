/** Made-up data for the editor spike: people, files and a handful of documents. */
import type { DocNode } from '$lib/editor/schema.ts';

import { HAND_MADE, WITH_DIAGRAMS } from '../../tests/support/body-docs.ts';

export const USERS: { id: string; name: string }[] = [
  { id: 'user-ada', name: 'Ada Lovelace' },
  { id: 'user-kim', name: '김하늘' },
  { id: 'user-sato', name: '佐藤 花子' },
  { id: 'user-lee', name: '이도윤' },
];

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

/** Every node and mark a body had before formulas: no formula and no diagram in it. */
const PLAIN: DocNode[] = [
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
];

export const SAMPLES: Record<string, DocNode> = {
  empty: { type: 'doc', content: [{ type: 'paragraph' }] },
  // Neither a formula nor a diagram: opening it must fetch neither library.
  plain: { type: 'doc', content: PLAIN },
  everything: {
    type: 'doc',
    content: [
      ...PLAIN,
      paragraph(
        text('피타고라스의 정리 '),
        { type: 'inlineMath', attrs: { latex: 'a^2 + b^2 = c^2' } },
        text(' 를 씁니다.'),
      ),
      { type: 'blockMath', attrs: { latex: '\\int_0^1 x^2 \\, dx = \\frac{1}{3}' } },
    ],
  },
  diagrams: {
    type: 'doc',
    content: Object.values(WITH_DIAGRAMS).flatMap((sample) => sample.content ?? []),
  },
  // One of each, for a look at both in the light and in the dark.
  embeds: {
    type: 'doc',
    content: [
      { type: 'heading', attrs: { level: 2 }, content: [text('수식과 다이어그램')] },
      paragraph(
        text('문장 안의 수식 '),
        { type: 'inlineMath', attrs: { latex: 'E = mc^2' } },
        text(' 과 한 줄을 차지하는 수식:'),
      ),
      { type: 'blockMath', attrs: { latex: 'x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}' } },
      {
        type: 'codeBlock',
        attrs: { language: 'mermaid' },
        content: [
          text('flowchart LR\n  A[초안] --> B{검토}\n  B -->|통과| C[배포]\n  B -->|수정| A'),
        ],
      },
      {
        type: 'codeBlock',
        attrs: { language: 'mermaid' },
        content: [text('pie title 진행\n  "끝남" : 12\n  "하는 중" : 5\n  "남음" : 8')],
      },
      {
        type: 'codeBlock',
        attrs: { language: 'mermaid' },
        content: [
          text(
            'sequenceDiagram\n  participant 웹\n  participant 서버\n  웹->>서버: 저장\n  Note over 서버: 검증\n  서버-->>웹: 200',
          ),
        ],
      },
    ],
  },
  // The corners: every document the tests compare the read-only view on
  // (tests/support/body-docs.ts), one after the other.
  corners: {
    type: 'doc',
    content: Object.values(HAND_MADE).flatMap((sample) => sample.content ?? []),
  },
};

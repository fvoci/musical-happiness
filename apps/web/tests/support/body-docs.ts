/**
 * Documents written by hand for comparing the read-only view of a body with
 * the editor. The built-in templates use only part of what a body may
 * contain; these hold the rest, and the corners of what they do use: quotes,
 * code, line breaks, mentions, struck text, inline code, a list that starts
 * at 3, tasks inside tasks, images with a description, text under several
 * marks, and empty paragraphs.
 */
import type { DocMark, DocNode } from '$lib/editor/schema.ts';

type MarkLike = string | DocMark;

export const link = (href: string): DocMark => ({ type: 'link', attrs: { href } });

export const text = (value: string, ...marks: MarkLike[]): DocNode =>
  marks.length > 0
    ? {
        type: 'text',
        text: value,
        marks: marks.map((mark) => (typeof mark === 'string' ? { type: mark } : mark)),
      }
    : { type: 'text', text: value };

export const paragraph = (...content: DocNode[]): DocNode =>
  content.length > 0 ? { type: 'paragraph', content } : { type: 'paragraph' };
export const heading = (level: number, ...content: DocNode[]): DocNode =>
  content.length > 0
    ? { type: 'heading', attrs: { level }, content }
    : { type: 'heading', attrs: { level } };
export const doc = (...content: DocNode[]): DocNode => ({ type: 'doc', content });
export const item = (...content: DocNode[]): DocNode => ({ type: 'listItem', content });
export const bullets = (...content: DocNode[]): DocNode => ({ type: 'bulletList', content });
export const numbered = (start: number | undefined, ...content: DocNode[]): DocNode =>
  start === undefined
    ? { type: 'orderedList', content }
    : { type: 'orderedList', attrs: { start }, content };
export const tasks = (...content: DocNode[]): DocNode => ({ type: 'taskList', content });
export const task = (checked: boolean, ...content: DocNode[]): DocNode => ({
  type: 'taskItem',
  attrs: { checked },
  content,
});
export const quote = (...content: DocNode[]): DocNode => ({ type: 'blockquote', content });
export const code = (language: string | undefined, value?: string): DocNode => ({
  type: 'codeBlock',
  ...(language === undefined ? {} : { attrs: { language } }),
  ...(value === undefined ? {} : { content: [text(value)] }),
});
export const rule = (): DocNode => ({ type: 'horizontalRule' });
export const lineBreak = (): DocNode => ({ type: 'hardBreak' });
export const image = (fileId: string, alt?: string): DocNode => ({
  type: 'image',
  attrs: alt === undefined ? { fileId } : { fileId, alt },
});
export const mention = (userId: string): DocNode => ({ type: 'mention', attrs: { userId } });

/** The people the documents mention. `user-gone` is deliberately not among them. */
export const PEOPLE: Record<string, string> = {
  'user-kim': '김하늘',
  'user-ada': 'Ada Lovelace',
};

/** Files whose identifier starts like this have no address. */
export const UNRESOLVED_FILE_PREFIX = 'missing-';

export const HAND_MADE: Record<string, DocNode> = {
  'headings of every level': doc(
    heading(1, text('제목 하나')),
    heading(2, text('Heading two')),
    heading(3, text('Heading three with '), text('bold', 'bold')),
    paragraph(text('Text under the headings.')),
  ),

  quotes: doc(
    quote(paragraph(text('인용한 문장.'))),
    quote(
      paragraph(text('A quote with a list:')),
      bullets(item(paragraph(text('one'))), item(paragraph(text('two')))),
      quote(paragraph(text('and a quote inside the quote'))),
    ),
  ),

  'code blocks': doc(
    code('ts', 'const answer: number = 42; // 주석'),
    code(undefined, 'no language\n  two spaces in\n\ttab in'),
    code('c++', 'int main() { return 0; }'),
    code('rust'),
    code(undefined),
    code('sh', 'ends with a newline\n'),
    code(undefined, '<script>alert("not markup")</script> &amp; <b>'),
  ),

  'line breaks': doc(
    paragraph(text('first line'), lineBreak(), text('second line')),
    paragraph(text('a break at the end'), lineBreak()),
    paragraph(lineBreak(), text('a break at the start')),
    paragraph(text('two breaks'), lineBreak(), lineBreak(), text('in a row')),
    paragraph(lineBreak()),
    heading(2, text('In a heading'), lineBreak(), text('too')),
  ),

  mentions: doc(
    paragraph(
      text('담당 '),
      mention('user-kim'),
      text(' 님, 검토 '),
      mention('user-ada'),
      text(', 그리고 떠난 사람 '),
      mention('user-gone'),
    ),
    paragraph(mention('user-kim')),
    paragraph(mention('user-kim'), mention('user-ada')),
    heading(3, mention('user-ada'), text(' in a heading')),
    paragraph(text('bold', 'bold'), mention('user-kim'), text('bold again', 'bold')),
  ),

  'struck text and inline code': doc(
    paragraph(
      text('This is '),
      text('struck', 'strike'),
      text(' and this is '),
      text('code()', 'code'),
      text('.'),
    ),
    paragraph(text('<b>not bold</b> & "quoted"', 'code')),
  ),

  'numbered lists': doc(
    numbered(3, item(paragraph(text('three'))), item(paragraph(text('four')))),
    numbered(1, item(paragraph(text('one')))),
    numbered(undefined, item(paragraph(text('no start given')))),
    numbered(0, item(paragraph(text('zero')))),
    numbered(
      41,
      item(
        paragraph(text('forty-one')),
        numbered(7, item(paragraph(text('seven, inside'))), item(paragraph(text('eight')))),
      ),
    ),
  ),

  'bullet lists in each other': doc(
    bullets(
      item(
        paragraph(text('one')),
        bullets(
          item(
            paragraph(text('two')),
            bullets(
              item(
                paragraph(text('three')),
                bullets(item(paragraph(text('four')), paragraph(text('a second paragraph')))),
              ),
            ),
          ),
        ),
      ),
      item(paragraph(text('back at the top')), code('txt', 'code in a list'), rule()),
    ),
  ),

  'tasks inside tasks': doc(
    tasks(
      task(
        true,
        paragraph(text('done, with tasks inside')),
        tasks(
          task(false, paragraph(text('inner, open'))),
          task(
            true,
            paragraph(text('inner, done')),
            tasks(task(false, paragraph(text('innermost')))),
          ),
        ),
      ),
      task(false, paragraph(text('open, with a second paragraph')), paragraph(text('more'))),
      task(false, paragraph()),
      task(
        true,
        paragraph(
          text('for '),
          mention('user-kim'),
          lineBreak(),
          text('with '),
          text('marks', 'bold'),
        ),
        bullets(item(paragraph(text('a plain list in a task')))),
        image('file-in-task', 'in a task'),
      ),
    ),
    bullets(
      item(paragraph(text('a plain item')), tasks(task(true, paragraph(text('a task in it'))))),
    ),
  ),

  images: doc(
    image('file-first', '진행 그래프'),
    paragraph(text('between')),
    image('file-no-alt'),
    image('file-empty-alt', ''),
    image(`${UNRESOLVED_FILE_PREFIX}file`, 'no address for this one'),
    image(`${UNRESOLVED_FILE_PREFIX}other`),
    image('file-quotes', 'a "quoted" <description> & more'),
    quote(image('file-in-quote', 'in a quote')),
  ),

  'text under several marks': doc(
    paragraph(
      text('all four', link('https://example.com/a?b=1&c="d"'), 'bold', 'italic', 'strike'),
      text(' bold italic', 'bold', 'italic'),
      text(' italic', 'italic'),
      text(' plain '),
      text('italic', 'italic'),
      text(' italic bold', 'italic', 'bold'),
      text(' bold', 'bold'),
    ),
    // The stored order of marks has no meaning: these are out of the editor's order.
    paragraph(
      text('strike first', 'strike', 'italic', 'bold', link('/board/1')),
      text(' then code', 'code', link('/board/1')),
      text(' same link', link('/board/1')),
      text(' other link', link('/board/2')),
      text(' mail', link('mailto:someone@example.com')),
    ),
    paragraph(
      text('bold ', 'bold'),
      text('bold struck', 'bold', 'strike'),
      text(' bold', 'bold'),
      text(' struck', 'strike'),
      text(' struck italic', 'strike', 'italic'),
    ),
    // Two texts with the same marks, which the editor joins into one.
    paragraph(text('joined ', 'bold'), text('together', 'bold'), text(' and '), text('plain')),
    heading(2, text('Marks in a '), text('heading', 'italic', 'code')),
  ),

  'empty paragraphs': doc(
    paragraph(),
    paragraph(text('after an empty paragraph')),
    paragraph(),
    paragraph(),
    heading(2),
    paragraph(text('before the last, empty one')),
    paragraph(),
  ),

  'only empty paragraphs': doc(paragraph(), paragraph()),

  'spaces and characters that look like markup': doc(
    paragraph(text('two  spaces,   three, and a trailing one ')),
    paragraph(text('  leading spaces')),
    paragraph(text('a\ttab and a line\nfeed inside one text')),
    paragraph(text('ends with a line feed\n')),
    paragraph(text('<script>alert(1)</script> <img src=x onerror=alert(1)> &lt; &amp; "\'`')),
    paragraph(text('한글 조합 문자 한글, emoji 🚀👍🏽, and a zero width​space')),
  ),

  'rules and what follows them': doc(
    rule(),
    rule(),
    paragraph(text('between rules')),
    rule(),
    image('file-after-rule'),
    rule(),
  ),

  'an image first': doc(image('file-lead', 'first'), paragraph(text('after the image'))),
};

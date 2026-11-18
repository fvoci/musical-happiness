/**
 * Small Word documents that make docx-preview do a great deal: the cases the
 * reviews of the viewer found (each a few kilobytes, or less). The viewer
 * must end each within its limits: drawn, or refused as too large, without
 * the app's page waiting on it.
 */
import { zip } from './viewer-files.ts';

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const V = 'urn:schemas-microsoft-com:vml';
const PACKAGE_RELS = 'http://schemas.openxmlformats.org/package/2006/relationships';
const TYPE = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>';
const NAMESPACES = `xmlns:w="${W}" xmlns:r="${R}" xmlns:v="${V}"`;

const p = (text: string): string => `<w:p><w:r><w:t>${text}</w:t></w:r></w:p>`;
/** A paragraph that starts a page, where Word last broke it. */
const page = `<w:p><w:r><w:lastRenderedPageBreak/></w:r><w:r><w:t>쪽</w:t></w:r></w:p>`;

interface Parts {
  /** The body's paragraphs and tables, without the section's properties. */
  body: string;
  /** What the section's properties hold: header and footer references. */
  section?: string;
  /** Other parts, by name under word/, with their relationship type. */
  parts?: { name: string; type: string; xml: string }[];
  /** More of the document's relationships, as XML. */
  links?: string;
}

/** A Word document of the parts; relationship ids are the parts' names. */
export function wordDocx({ body, section = '', parts = [], links = '' }: Parts): Buffer {
  const rels =
    parts
      .map(
        (part) =>
          `<Relationship Id="${part.name}" Type="${TYPE}/${part.type}" Target="${part.name}.xml"/>`,
      )
      .join('') + links;
  return zip([
    {
      name: '_rels/.rels',
      data: Buffer.from(
        `${XML}<Relationships xmlns="${PACKAGE_RELS}"><Relationship Id="rId1" Type="${TYPE}/officeDocument" Target="word/document.xml"/></Relationships>`,
      ),
    },
    {
      name: 'word/document.xml',
      data: Buffer.from(
        `${XML}<w:document ${NAMESPACES}><w:body>${body}<w:sectPr>${section}</w:sectPr></w:body></w:document>`,
      ),
    },
    {
      name: 'word/_rels/document.xml.rels',
      data: Buffer.from(`${XML}<Relationships xmlns="${PACKAGE_RELS}">${rels}</Relationships>`),
    },
    ...parts.map((part) => ({
      name: `word/${part.name}.xml`,
      data: Buffer.from(`${XML}${part.xml}`),
    })),
  ]);
}

/** Runs set in superscript, each holding a text box whose paragraph holds the next: docx-preview draws each run's content twice. */
export function nestedSuperscriptDocx(depth: number): Buffer {
  let run = '<w:r><w:t>x</w:t></w:r>';
  for (let level = 0; level < depth; level += 1) {
    run = `<w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:pict><v:shape><v:textbox><w:txbxContent><w:p>${run}</w:p></w:txbxContent></v:textbox></v:shape></w:pict></w:r>`;
  }
  return wordDocx({ body: `<w:p>${run}</w:p>` });
}

/** A header holding a table of many columns, on many pages: a `col` for each, on each page. */
export function wideTableHeaderDocx(columns: number, pages: number): Buffer {
  const grid = '<w:gridCol w:w="10"/>'.repeat(columns);
  const header = `<w:hdr ${NAMESPACES}><w:tbl><w:tblGrid>${grid}</w:tblGrid><w:tr><w:tc>${p('a')}</w:tc></w:tr></w:tbl></w:hdr>`;
  return wordDocx({
    body: p('본문') + page.repeat(pages - 1),
    section: '<w:headerReference w:type="default" r:id="header1"/>',
    parts: [{ name: 'header1', type: 'header', xml: header }],
  });
}

/** Footnote references, each to a footnote of endnote references, each to a long endnote. */
export function notesInNotesDocx(references: number, inner: number, lines: number): Buffer {
  const footnotes = `<w:footnotes ${NAMESPACES}><w:footnote w:id="1"><w:p>${'<w:r><w:endnoteReference w:id="1"/></w:r>'.repeat(inner)}</w:p></w:footnote></w:footnotes>`;
  const endnotes = `<w:endnotes ${NAMESPACES}><w:endnote w:id="1">${p('미주').repeat(lines)}</w:endnote></w:endnotes>`;
  return wordDocx({
    body: `<w:p>${'<w:r><w:footnoteReference w:id="1"/></w:r>'.repeat(references)}</w:p>`,
    parts: [
      { name: 'footnotes', type: 'footnotes', xml: footnotes },
      { name: 'endnotes', type: 'endnotes', xml: endnotes },
    ],
  });
}

/** One list of many levels, used by many numberings: a style rule for each level of each. */
export function manyListLevelsDocx(levels: number, numberings: number): Buffer {
  const lvl = Array.from(
    { length: levels },
    (_, at) => `<w:lvl w:ilvl="${at}"><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl>`,
  ).join('');
  const nums = Array.from(
    { length: numberings },
    (_, at) => `<w:num w:numId="${at + 1}"><w:abstractNumId w:val="0"/></w:num>`,
  ).join('');
  const numbering = `<w:numbering ${NAMESPACES}><w:abstractNum w:abstractNumId="0">${lvl}</w:abstractNum>${nums}</w:numbering>`;
  return wordDocx({
    body: p('목록'),
    parts: [{ name: 'numbering', type: 'numbering', xml: numbering }],
  });
}

/** A long footer that the section names as its header too, on many pages. */
export function footerAsHeaderDocx(lines: number, pages: number): Buffer {
  const footer = `<w:ftr ${NAMESPACES}>${p('바닥글').repeat(lines)}</w:ftr>`;
  return wordDocx({
    body: p('본문') + page.repeat(pages - 1),
    section:
      '<w:headerReference w:type="default" r:id="footer1"/><w:footerReference w:type="default" r:id="footer1"/>',
    parts: [{ name: 'footer1', type: 'footer', xml: footer }],
  });
}

/** Styles each based on the one before: a long chain. */
export function styleChainDocx(length: number): Buffer {
  const styles = Array.from(
    { length },
    (_, at) =>
      `<w:style w:type="paragraph" w:styleId="s${at}">${at > 0 ? `<w:basedOn w:val="s${at - 1}"/>` : ''}<w:pPr><w:pageBreakBefore/></w:pPr></w:style>`,
  ).join('');
  return wordDocx({
    body: `<w:p><w:pPr><w:pStyle w:val="s${length - 1}"/></w:pPr><w:r><w:t>끝</w:t></w:r></w:p>`,
    parts: [
      { name: 'styles', type: 'styles', xml: `<w:styles ${NAMESPACES}>${styles}</w:styles>` },
    ],
  });
}

/** Many links, each with its own relationship: docx-preview looks each up among all of them. */
export function manyLinksDocx(count: number): Buffer {
  const indices = Array.from({ length: count }, (_, at) => at);
  return wordDocx({
    body: `<w:p>${indices.map((at) => `<w:hyperlink r:id="link${at}"><w:r><w:t>링크</w:t></w:r></w:hyperlink>`).join('')}</w:p>`,
    links: indices
      .map(
        (at) =>
          `<Relationship Id="link${at}" Type="${TYPE}/hyperlink" Target="https://example.invalid/${at}" TargetMode="External"/>`,
      )
      .join(''),
  });
}

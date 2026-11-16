/**
 * Files for the tests of the file viewer, made here rather than kept as
 * binaries: a PDF of several pages with Korean text, a Word document in
 * Korean with a header, a table and a picture, and the ZIP and PNG writers
 * they need. Every file is ours, so the tests carry no third-party content.
 */
import { crc32, deflateRawSync, deflateSync } from 'node:zlib';

/** Korean text as the UCS-2 codes of the `UniKS-UCS2-H` encoding, written as a PDF hex string. */
function ucs2Hex(text: string): string {
  let hex = '';
  for (const character of text) {
    hex += (character.codePointAt(0) ?? 0).toString(16).padStart(4, '0');
  }
  return `<${hex}>`;
}

/**
 * A PDF of `pages` A4 pages. Each says "Page k of n" in Helvetica, has a
 * line of Korean in a CID font that is not embedded (so drawing it needs the
 * Korean CMap, which the viewer supplies without the network), and a coloured
 * box that tells the pages apart in a picture.
 */
export function koreanPdf(pages: number): Buffer {
  const objects: string[] = [];
  const add = (body: string): number => objects.push(body);

  const catalog = add('');
  const tree = add('');
  const latin = add('<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>');
  const descriptor = add(
    '<< /Type /FontDescriptor /FontName /HYGoThic-Medium /Flags 4 /FontBBox [-6 -145 1003 880] /ItalicAngle 0 /Ascent 880 /Descent -120 /CapHeight 880 /StemV 93 >>',
  );
  const cid = add(
    `<< /Type /Font /Subtype /CIDFontType0 /BaseFont /HYGoThic-Medium /CIDSystemInfo << /Registry (Adobe) /Ordering (Korea1) /Supplement 2 >> /FontDescriptor ${descriptor} 0 R /DW 1000 >>`,
  );
  const korean = add(
    `<< /Type /Font /Subtype /Type0 /BaseFont /HYGoThic-Medium /Encoding /UniKS-UCS2-H /DescendantFonts [${cid} 0 R] >>`,
  );

  const kids: number[] = [];
  for (let page = 1; page <= pages; page += 1) {
    const hue = (page * 0.27) % 1;
    const stream = [
      `BT /F1 28 Tf 72 760 Td (Page ${page} of ${pages}) Tj ET`,
      `BT /F2 22 Tf 72 700 Td ${ucs2Hex(`한국어 견본 ${page}쪽`)} Tj ET`,
      `${hue.toFixed(2)} 0.45 0.80 rg 72 520 300 120 re f`,
    ].join('\n');
    const content = add(
      `<< /Length ${Buffer.byteLength(stream, 'latin1')} >>\nstream\n${stream}\nendstream`,
    );
    kids.push(
      add(
        `<< /Type /Page /Parent ${tree} 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 ${latin} 0 R /F2 ${korean} 0 R >> >> /Contents ${content} 0 R >>`,
      ),
    );
  }
  objects[catalog - 1] = `<< /Type /Catalog /Pages ${tree} 0 R >>`;
  objects[tree - 1] =
    `<< /Type /Pages /Kids [${kids.map((kid) => `${kid} 0 R`).join(' ')}] /Count ${pages} >>`;

  let text = '%PDF-1.7\n%âãÏÓ\n';
  const offsets: number[] = [];
  objects.forEach((body, index) => {
    offsets.push(Buffer.byteLength(text, 'latin1'));
    text += `${index + 1} 0 obj\n${body}\nendobj\n`;
  });
  const xref = Buffer.byteLength(text, 'latin1');
  text += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const offset of offsets) text += `${String(offset).padStart(10, '0')} 00000 n \n`;
  text += `trailer\n<< /Size ${objects.length + 1} /Root ${catalog} 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(text, 'latin1');
}

/** A PNG of one colour. */
export function png(
  width: number,
  height: number,
  [red, green, blue]: [number, number, number],
): Buffer {
  const chunk = (type: string, data: Buffer): Buffer => {
    const length = Buffer.alloc(4);
    length.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type, 'latin1'), data]);
    const sum = Buffer.alloc(4);
    sum.writeUInt32BE(crc32(body));
    return Buffer.concat([length, body, sum]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 2, 0, 0, 0], 8);
  const row = Buffer.alloc(1 + width * 3);
  for (let x = 0; x < width; x += 1) row.set([red, green, blue], 1 + x * 3);
  const pixels = Buffer.concat(Array.from({ length: height }, () => row));
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(pixels)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

/** A ZIP archive of the members, deflated. */
export function zip(members: { name: string; data: Buffer }[]): Buffer {
  const locals: Buffer[] = [];
  const directory: Buffer[] = [];
  let offset = 0;
  for (const { name, data } of members) {
    const packed = deflateRawSync(data);
    const fileName = Buffer.from(name, 'utf8');
    const sum = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0x0800, 6);
    local.writeUInt16LE(8, 8);
    local.writeUInt32LE(sum, 14);
    local.writeUInt32LE(packed.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(fileName.length, 26);
    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(20, 4);
    entry.writeUInt16LE(20, 6);
    entry.writeUInt16LE(0x0800, 8);
    entry.writeUInt16LE(8, 10);
    entry.writeUInt32LE(sum, 16);
    entry.writeUInt32LE(packed.length, 20);
    entry.writeUInt32LE(data.length, 24);
    entry.writeUInt16LE(fileName.length, 28);
    entry.writeUInt32LE(offset, 42);
    locals.push(local, fileName, packed);
    directory.push(entry, fileName);
    offset += local.length + fileName.length + packed.length;
  }
  const directoryBytes = Buffer.concat(directory);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(members.length, 8);
  end.writeUInt16LE(members.length, 10);
  end.writeUInt32LE(directoryBytes.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, directoryBytes, end]);
}

const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const PACKAGE_RELS = 'http://schemas.openxmlformats.org/package/2006/relationships';

/** A run of text in the font Korean Word documents use most. */
const run = (text: string, bold = false): string =>
  `<w:r><w:rPr><w:rFonts w:ascii="맑은 고딕" w:hAnsi="맑은 고딕" w:eastAsia="맑은 고딕"/>${bold ? '<w:b/>' : ''}<w:sz w:val="22"/></w:rPr><w:t xml:space="preserve">${text}</w:t></w:r>`;
const paragraph = (text: string, bold = false): string => `<w:p>${run(text, bold)}</w:p>`;
const cell = (text: string): string =>
  `<w:tc><w:tcPr><w:tcW w:w="3000" w:type="dxa"/></w:tcPr>${paragraph(text)}</w:tc>`;

/** The text the Word document's body, header and table carry, for the tests to look for. */
export const DOCX_TEXT = {
  header: '머리글: 회의록',
  title: '분기 회의록',
  body: '이 문서는 한국어 견본입니다. 표와 그림이 들어 있습니다.',
  cells: ['항목', '담당', '기한', '설계 검토', '김민지', '11월 20일'],
  /** A link to a page on the network, which the viewer must not follow. */
  link: '원본 열기',
};

/** Where the document's link points. */
export const DOCX_LINK = 'https://example.invalid/login';

/** A Word document in Korean with a header, a table of two rows, a picture and a link. */
export function koreanDocx(): Buffer {
  const [h1, h2, h3, c1, c2, c3] = DOCX_TEXT.cells;
  const picture = `<w:p><w:r><w:drawing><wp:inline xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><wp:extent cx="1905000" cy="952500"/><wp:docPr id="1" name="그림 1"/><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:nvPicPr><pic:cNvPr id="1" name="image1.png"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImage"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1905000" cy="952500"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>`;
  const table = `<w:tbl><w:tblPr><w:tblW w:w="9000" w:type="dxa"/><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="000000"/><w:left w:val="single" w:sz="4" w:space="0" w:color="000000"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="000000"/><w:right w:val="single" w:sz="4" w:space="0" w:color="000000"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="000000"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="000000"/></w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid><w:tr>${cell(h1 ?? '')}${cell(h2 ?? '')}${cell(h3 ?? '')}</w:tr><w:tr>${cell(c1 ?? '')}${cell(c2 ?? '')}${cell(c3 ?? '')}</w:tr></w:tbl>`;
  const link = `<w:p><w:hyperlink r:id="rIdLink">${run(DOCX_TEXT.link)}</w:hyperlink></w:p>`;
  const document = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="${W}" xmlns:r="${R}"><w:body>${paragraph(DOCX_TEXT.title, true)}${paragraph(DOCX_TEXT.body)}${link}${table}${picture}<w:sectPr><w:headerReference w:type="default" r:id="rIdHeader"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>`;
  const header = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="${W}" xmlns:r="${R}">${paragraph(DOCX_TEXT.header)}</w:hdr>`;
  const types = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>`;
  const rootRels = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="${PACKAGE_RELS}"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>`;
  const documentRels = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="${PACKAGE_RELS}"><Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/><Relationship Id="rIdLink" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="${DOCX_LINK}" TargetMode="External"/></Relationships>`;
  const xml = (text: string): Buffer => Buffer.from(text, 'utf8');
  return zip([
    { name: '[Content_Types].xml', data: xml(types) },
    { name: '_rels/.rels', data: xml(rootRels) },
    { name: 'word/document.xml', data: xml(document) },
    { name: 'word/_rels/document.xml.rels', data: xml(documentRels) },
    { name: 'word/header1.xml', data: xml(header) },
    { name: 'word/media/image1.png', data: png(200, 100, [46, 125, 210]) },
  ]);
}

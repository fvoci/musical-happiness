/**
 * The vectors of a body as a Y document (`contracts/ydoc-vectors.json`),
 * which the server's conversion is held against (`crates/collab`): that the
 * file is what the tool writes from the editor's schema and binding, and
 * what the file says about the two.
 */
import { readFileSync } from 'node:fs';

import { getSchema } from '@tiptap/core';
import { Collaboration } from '@tiptap/extension-collaboration';
import { Node } from '@tiptap/pm/model';
import { describe, expect, it } from 'vitest';

import { bodyExtensions } from '$lib/editor/extensions.ts';
import { BODY_VERSION, toStoredDoc, validateDoc, type DocNode } from '$lib/editor/schema.ts';

import {
  buildVectors,
  FIELD,
  fromBase64,
  readBodyCases,
  readerOf,
  readStored,
  serialize,
  YDOC_VECTORS_FILE,
  type YdocVectors,
} from '../../tools/ydoc/vectors.ts';

const schema = getSchema(bodyExtensions());
const committed = readFileSync(YDOC_VECTORS_FILE, 'utf8');
const vectors = JSON.parse(committed) as YdocVectors;

/** Whether the editor's own strict check takes a document. */
function passesStrictCheck(json: DocNode): boolean {
  try {
    Node.fromJSON(schema, json).check();
    return true;
  } catch {
    return false;
  }
}

describe('the vectors of a body as a Y document', () => {
  it('are what the tool writes', () => {
    // Making them also holds each case to what it is filed under: an editor
    // keeps every document and reading as it is, and deletes from every repair.
    expect(serialize(buildVectors(schema, readBodyCases()))).toBe(committed);
  });

  it('are written for the version of the list the editor has, and name the shared type its collaboration extension binds to', () => {
    expect(vectors.bodyVersion).toBe(BODY_VERSION);
    expect(Collaboration.options.field).toBe(FIELD);
    expect(vectors.field).toBe(FIELD);
  });

  it('hold every accepted body, as an editor holds it', () => {
    const accepted = readBodyCases().filter(
      (one) => one.expect === 'accept' && one.body !== undefined,
    );
    expect(vectors.documents.map((one) => one.name)).toEqual(accepted.map((one) => one.name));
    expect(vectors.documents.length).toBeGreaterThan(40);
    for (const [index, one] of vectors.documents.entries()) {
      const held = toStoredDoc(Node.fromJSON(schema, accepted[index]?.body).toJSON() as DocNode);
      expect(one.json, one.name).toEqual(held);
    }
  });

  it('hold more bodies, each as an editor holds it', () => {
    expect(vectors.bodies.length).toBeGreaterThan(5);
    for (const one of vectors.bodies) {
      expect(validateDoc(one.body), one.name).toEqual([]);
      const held = toStoredDoc(Node.fromJSON(schema, one.body).toJSON() as DocNode);
      expect(one.json, one.name).toEqual(held);
    }
  });

  it.each(
    [...vectors.documents, ...vectors.bodies, ...vectors.readings, ...vectors.repairs].map(
      (one) => [one.name, one] as const,
    ),
  )('%s: an editor reads the update as the JSON beside it, which is a body', (_name, one) => {
    expect(readStored(readerOf(fromBase64(one.update)), schema)).toEqual(one.json);
    if (one.json !== null) expect(validateDoc(one.json)).toEqual([]);
  });

  it.each(vectors.refused.map((one) => [one.name, one] as const))(
    '%s: an editor reads the update as the JSON beside it, which is no body',
    (_name, one) => {
      expect(readStored(readerOf(fromBase64(one.update)), schema)).toEqual(one.json);
      expect(validateDoc(one.json)).not.toEqual([]);
    },
  );

  it("the editor's strict check refuses what the binding keeps: code beside another mark", () => {
    // The server accepts such a body, and a merge of two edits can make one.
    // An editor made with its content check on would stop at it.
    const marksOf = (node: DocNode): string[][] => [
      ...(node.marks ? [node.marks.map((mark) => mark.type)] : []),
      ...(node.content ?? []).flatMap(marksOf),
    ];
    const hasCodeBesideAnother = (json: DocNode): boolean =>
      marksOf(json).some((marks) => marks.includes('code') && marks.length > 1);
    const kept = [...vectors.documents, ...vectors.bodies, ...vectors.readings].flatMap((one) =>
      one.json === null ? [] : [{ name: one.name, json: one.json }],
    );
    const refused = kept.filter((one) => !passesStrictCheck(one.json)).map((one) => one.name);
    expect(refused).toEqual(
      kept.filter((one) => hasCodeBesideAnother(one.json)).map((one) => one.name),
    );
    expect(refused).toEqual([
      'every mark once',
      'marks come in the order of the schema, however they were written',
      'bold and code together, which the editor never sets, are both kept',
    ]);
  });
});

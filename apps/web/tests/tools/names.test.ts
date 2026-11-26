/**
 * The name the original gives the workspace (tools/names/workspace.ts): which
 * paths an exception is for, why it must have a reason, and how a line passes.
 */
import { describe, expect, it } from 'vitest';

import {
  FORMER,
  checkFile,
  matches,
  namesFormer,
  parseExceptions,
  staleExceptions,
} from '../../tools/names/workspace.ts';

/** `text` with `@` standing for the former name, so that this file passes the check. */
const named = (text: string): string => text.split('@').join(FORMER);

describe('the exceptions of the workspace name', () => {
  it('stand for names with * and for folders with **', () => {
    expect(matches('messages/*.json', 'messages/ko.json')).toBe(true);
    expect(matches('messages/*.json', 'messages/x/ko.json')).toBe(false);
    expect(matches('**', 'e2e/sidebar.spec.ts')).toBe(true);
    expect(matches('src/**/seed.ts', 'src/seed.ts')).toBe(true);
    expect(matches('src/**/seed.ts', 'src/lib/state/memory/seed.ts')).toBe(true);
    expect(matches('docs/a*.md', 'docs/b.md')).toBe(false);
  });

  it('each need a reason, and a text with the name in it', () => {
    expect(() => parseExceptions('a.ts\n')).toThrow(/reason/);
    expect(() => parseExceptions('# why\n\na.ts\n')).toThrow(/reason/);
    expect(() => parseExceptions('# why\na.ts | workspace\n')).toThrow(/no/);
    expect(parseExceptions(named('# why\na.ts | @Id\nb.ts\n'))).toEqual([
      { line: 2, pattern: 'a.ts', text: named('@id') },
      { line: 3, pattern: 'b.ts', text: null },
    ]);
  });

  it('let a line pass when the name is within the texts of its file only', () => {
    const exceptions = parseExceptions(
      named("# why\na.ts | 'Move to @'\nb.ts\n# why\nc.ts | @mate\n"),
    );
    const used = exceptions.map(() => false);
    const text = named(
      "const x = 1;\nmenu('Move to @');\nmenu('Move to @', @Id);\nconst @mate = 2;\n",
    );
    expect(checkFile('a.ts', text, exceptions, used)).toEqual(['a.ts:3', 'a.ts:4']);
    expect(used).toEqual([true, false, false]);
    expect(checkFile(named('@s/b.ts'), named('@\n'), exceptions, used)).toEqual([
      named('@s/b.ts: in its name'),
      named('@s/b.ts:1'),
    ]);
    expect(checkFile('b.ts', named('@S\n'), exceptions, used)).toEqual([]);
    expect(staleExceptions(exceptions, used)).toEqual([
      'xtask/exceptions/names.txt:5: nothing needs it: remove it',
    ]);
  });

  it('check the ids of a file of messages, not what the messages say', () => {
    const exceptions = parseExceptions(named('# why\nmessages/*.json | "old.@-id"\n'));
    const used = exceptions.map(() => false);
    const text = named('{\n  "a": "my @",\n  "old.@-id": "x",\n  "new_@_id": "x"\n}\n');
    expect(checkFile('messages/en.json', text, exceptions, used)).toEqual(['messages/en.json:4']);
    expect(checkFile('src/en.json', text, exceptions, used)).toEqual([
      'src/en.json:2',
      'src/en.json:3',
      'src/en.json:4',
    ]);
  });
});

describe('the former name by itself', () => {
  it('is a word, or a camel-case part of one, not letters inside another word', () => {
    expect(namesFormer(named('@'))).toBe(true);
    expect(namesFormer(named('my@Id'))).toBe(false);
    expect(namesFormer(named('my' + FORMER.charAt(0).toUpperCase() + FORMER.slice(1)))).toBe(true);
    expect(namesFormer(named('s@ing kettle'))).toBe(false);
    expect(namesFormer(named('S@'))).toBe(false);
    expect(namesFormer(named('A_@_B'))).toBe(true);
  });

  it('is not what an address says', () => {
    expect(namesFormer(named('crate (https://example.com/blake3-@/b3)'))).toBe(false);
    expect(namesFormer(named('blake3-@'))).toBe(true);
  });
});

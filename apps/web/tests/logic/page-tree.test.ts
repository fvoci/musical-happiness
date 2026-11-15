import { describe, expect, it } from 'vitest';

import {
  ancestorsOf,
  comparePlaces,
  firstPage,
  pageRows,
  pageTree,
  withDescendants,
  type PlacedPage,
  type PageTreeNode,
} from '../../src/lib/logic/page-tree.ts';

const page = (id: string, parentId = '', order = 'V'): PlacedPage => ({ id, parentId, order });

const shape = (nodes: readonly PageTreeNode<PlacedPage>[]): unknown[] =>
  nodes.map((node) =>
    node.children.length > 0 ? [node.page.id, shape(node.children)] : node.page.id,
  );

describe('the tree of a wiki', () => {
  it('puts each page below its parent, and siblings by their order', () => {
    const tree = pageTree([
      page('c', '', 'X'),
      page('a', '', 'V'),
      page('a2', 'a', 'W'),
      page('a1', 'a', 'U'),
      page('b', 'a1', 'V'),
    ]);
    expect(shape(tree)).toEqual([['a', [['a1', ['b']], 'a2']], 'c']);
  });

  it('compares orders by code unit, not by a locale', () => {
    // Uppercase before lowercase, digits before both: what the server's order says.
    const tree = pageTree([page('x', '', 'a'), page('y', '', 'Z'), page('z', '', '9')]);
    expect(shape(tree)).toEqual(['z', 'y', 'x']);
    // A locale would put "a" before "Z".
    expect(['a', 'Z'].sort((one, other) => one.localeCompare(other))).toEqual(['a', 'Z']);
  });

  it('breaks a tie of orders by the identifiers, as code units', () => {
    const tree = pageTree([page('b', '', 'V'), page('B', '', 'V'), page('a', '', 'V')]);
    expect(shape(tree)).toEqual(['B', 'a', 'b']);
    expect(comparePlaces(page('a', '', 'V'), page('a', '', 'V'))).toBe(0);
  });

  it('sorts siblings whose order is empty first, and by identifier among them', () => {
    const tree = pageTree([page('q', '', 'V'), page('p', '', ''), page('o', '', '')]);
    expect(shape(tree)).toEqual(['o', 'p', 'q']);
  });

  it('shows at the top a page whose parent is not in the list', () => {
    const tree = pageTree([page('a', '', 'V'), page('lost', 'gone', 'U'), page('b', 'lost')]);
    expect(shape(tree)).toEqual([['lost', ['b']], 'a']);
  });

  it('shows pages that are each other’s parents rather than losing them', () => {
    const tree = pageTree([page('a', 'b'), page('b', 'a'), page('top')]);
    expect(tree.map((node) => node.page.id)).toContain('top');
    const seen = new Set<string>();
    const walk = (nodes: readonly PageTreeNode<PlacedPage>[]): void => {
      for (const node of nodes) {
        seen.add(node.page.id);
        walk(node.children);
      }
    };
    walk(tree);
    expect([...seen].sort()).toEqual(['a', 'b', 'top']);
  });

  it('draws the children of the open pages only', () => {
    const tree = pageTree([page('a'), page('a1', 'a'), page('a11', 'a1'), page('b', '', 'W')]);
    const rows = pageRows(tree, (id) => id === 'a');
    expect(rows.map((row) => [row.page.id, row.depth, row.hasChildren, row.open])).toEqual([
      ['a', 0, true, true],
      ['a1', 1, true, false],
      ['b', 0, false, false],
    ]);
    expect(rows[1]?.parentId).toBe('a');
    expect(rows.map((row) => [row.position, row.siblings])).toEqual([
      [1, 2],
      [1, 1],
      [2, 2],
    ]);
  });

  it('finds the pages above a page, nearest first', () => {
    const pages = [page('a'), page('a1', 'a'), page('a11', 'a1')];
    expect(ancestorsOf(pages, 'a11')).toEqual(['a1', 'a']);
    expect(ancestorsOf(pages, 'a')).toEqual([]);
    expect(ancestorsOf(pages, 'nowhere')).toEqual([]);
    expect(ancestorsOf([page('x', 'y'), page('y', 'x')], 'x')).toEqual(['y']);
  });

  it('takes a page with everything below it', () => {
    const pages = [page('a'), page('a1', 'a'), page('a11', 'a1'), page('b')];
    expect([...withDescendants(pages, 'a')].sort()).toEqual(['a', 'a1', 'a11']);
    expect([...withDescendants(pages, 'b')]).toEqual(['b']);
  });

  it('names the first page as the tree draws it', () => {
    const first = firstPage([page('z', '', 'W'), page('y', 'z', 'A'), page('x', '', 'X')]);
    expect(first?.id).toBe('z');
    expect(firstPage<PlacedPage>([])).toBeUndefined();
  });
});

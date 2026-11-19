/**
 * The built-in templates (`crates/server/assets/templates/`): what has to
 * hold of the files that are edited by hand. Their bodies pass the editor's
 * validator, they name neither the original nor an address outside the app
 * (the decision on links: none in a body, only example addresses as the value
 * of a link property), dates have the shape of the wire, and every reference
 * inside a template leads somewhere.
 */
import { describe, expect, it } from 'vitest';

import { validateDoc, type DocNode } from '$lib/editor/schema.ts';
import type { SeedTemplate } from '$lib/state/memory/seed.ts';
import { parseDateValue } from '$lib/logic/wire.ts';

import { readTemplates, templateDocuments } from '../support/templates.ts';

/** The values of the link properties of a template's cards. */
function linkValues(template: SeedTemplate): string[] {
  const linkIds = template.board.cardProperties
    .filter((property) => property.type === 'url')
    .map((property) => property.id);
  return template.cards.flatMap((card) =>
    linkIds.flatMap((id) => {
      const value = card.fields.properties[id];
      return typeof value === 'string' ? [value] : [];
    }),
  );
}

/**
 * Whether the value of a link property leads outside the app. Such a value
 * is opened as an address whatever it looks like, so everything but a path
 * inside the app does.
 */
function valueLeadsOutside(value: string): boolean {
  const address = value.trim();
  return address !== '' && !/^\/(?!\/)/.test(address);
}

/**
 * Whether a value is an address under the domain reserved for examples
 * (RFC 2606) and names no other site: the only kind of address that leads
 * outside and may stay in a template.
 */
function isExampleAddress(value: string): boolean {
  const host = /^https:\/\/(?:[a-z0-9-]+\.)*example\.com(?![^/?#])/i.exec(value)?.[0];
  return host !== undefined && !/:\/\/|\bwww\./i.test(value.slice(host.length));
}

/** One line per address that leads outside and may not; none when there is none. */
function addressProblems(templates: readonly SeedTemplate[]): string[] {
  const problems: string[] = [];
  // The values of link properties are judged one by one, then left out of the
  // search through everything else.
  const rest = templates.map((template) => {
    const linkIds = new Set(
      template.board.cardProperties.filter(({ type }) => type === 'url').map(({ id }) => id),
    );
    const cards = template.cards.map((card) => {
      const properties = Object.fromEntries(
        Object.entries(card.fields.properties).filter(([id, value]) => {
          if (!linkIds.has(id) || typeof value !== 'string') return true;
          if (valueLeadsOutside(value) && !isExampleAddress(value)) {
            problems.push(`${template.key} / ${card.title}: a link value leads outside`);
          }
          return false;
        }),
      );
      return { ...card, fields: { ...card.fields, properties } };
    });
    return { ...template, cards };
  });
  if (/https?:\/\/|\bwww\./i.test(JSON.stringify(rest))) {
    problems.push('an address that leads outside is left outside a link value');
  }
  return problems;
}

describe('the built-in templates', () => {
  const templates = readTemplates();
  const everything = JSON.stringify(templates);

  it('are thirteen, one of them for onboarding', () => {
    expect(templates.map((template) => template.key)).toEqual([
      'meeting-agenda',
      'sales-pipeline-crm',
      'personal-tasks',
      'project-tasks',
      'company-goals-okrs',
      'personal-goals',
      'sprint-planner',
      'user-research-sessions',
      'competitive-analysis',
      'content-calendar',
      'team-retrospective',
      'roadmap',
      'welcome',
    ]);
    expect(templates.filter((template) => template.onboarding).map((t) => t.key)).toEqual([
      'welcome',
    ]);
  });

  it('hold 77 cards, 58 of them with a body that the validator accepts, and 38 views', () => {
    const cards = templates.flatMap((template) => template.cards);
    expect(cards).toHaveLength(77);
    const bodies = cards.filter((card) => card.body !== null);
    expect(bodies).toHaveLength(58);
    expect(templateDocuments()).toHaveLength(58);
    for (const card of bodies) expect(validateDoc(card.body), card.title).toEqual([]);
    expect(templates.flatMap((template) => template.views)).toHaveLength(38);
  });

  it('name no address outside, but for example addresses in link values', () => {
    expect(addressProblems(templates)).toEqual([]);
    const values = templates.flatMap(linkValues).filter(valueLeadsOutside);
    expect(values).toHaveLength(10);
    for (const value of values) expect(isExampleAddress(value), value).toBe(true);
    const bodies = JSON.stringify(templates.flatMap((t) => t.cards.map((card) => card.body)));
    expect(bodies).not.toMatch(/https?:\/\/|\bwww\./i);
  });

  it('would be found out with an address that is not an example, or one outside a link value', () => {
    const copy = (): SeedTemplate[] => JSON.parse(everything) as SeedTemplate[];

    const real = copy();
    const template = real.find((one) => linkValues(one).length > 0);
    const linkId = template?.board.cardProperties.find((property) => property.type === 'url')?.id;
    const card = template?.cards.find(
      (one) => typeof one.fields.properties[linkId ?? ''] === 'string',
    );
    if (!card || linkId === undefined) throw new Error('no card with a link value');
    card.fields.properties[linkId] = 'liminarycorp.com';
    expect(addressProblems(real)).toHaveLength(1);
    card.fields.properties[linkId] = 'https://www.example.com.evil.test/';
    expect(addressProblems(real)).toHaveLength(1);

    const elsewhere = copy();
    const first = elsewhere[0];
    if (!first) throw new Error('no template');
    first.board.description = 'See https://example.com for more.';
    expect(addressProblems(elsewhere)).toEqual([
      'an address that leads outside is left outside a link value',
    ]);
  });

  it('write date values the way the wire has them', () => {
    let dates = 0;
    for (const template of templates) {
      const dateIds = template.board.cardProperties
        .filter((property) => property.type === 'date')
        .map((property) => property.id);
      for (const card of template.cards) {
        for (const id of dateIds) {
          const value = card.fields.properties[id];
          if (value === undefined) continue;
          dates += 1;
          expect(parseDateValue(value).from?.date, card.title).toMatch(/^\d{4}-\d{2}-\d{2}$/);
        }
      }
    }
    expect(dates).toBe(19);
  });

  it('keep every reference inside a template whole, and carry no pictures', () => {
    const images = (node: DocNode): number =>
      (node.type === 'image' ? 1 : 0) +
      (node.content ?? []).reduce((sum, child) => sum + images(child), 0);
    for (const template of templates) {
      const cardIds = new Set(template.cards.map((card) => card.id));
      const properties = new Map(
        template.board.cardProperties.map((property) => [property.id, property]),
      );
      for (const view of template.views) {
        for (const id of view.fields.cardOrder as string[]) expect(cardIds.has(id)).toBe(true);
      }
      for (const card of template.cards) {
        for (const [id, value] of Object.entries(card.fields.properties)) {
          const property = properties.get(id);
          if (property?.type !== 'select') continue;
          expect(
            property.options.some((option) => option.id === value),
            `${template.key} / ${card.title}`,
          ).toBe(true);
        }
        expect(card.body ? images(card.body) : 0, card.title).toBe(0);
      }
      expect(template.files).toEqual([]);
    }
  });
});

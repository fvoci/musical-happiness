import type { Meta, StoryObj } from '@storybook/svelte';
import { expect } from 'storybook/test';
import ViewSample from './ViewSample.svelte';
import { clickStory, storyElement, visibleStoryElement } from './interactions.ts';

const meta = {
  title: 'Views/Header and tools',
  component: ViewSample,
  args: { kind: 'table', headerOnly: true },
  play: async ({ canvasElement, args }) => {
    await expect(await storyElement(canvasElement, '[data-testid="view-header"]')).toBeVisible();
    const editing = canvasElement.querySelectorAll(
      '[data-tool]:not([data-tool="search"]):not([data-tool="search-close"]):not([data-tool="views"]), [data-testid="new-card"]',
    );
    if (args.readOnly) await expect(editing).toHaveLength(0);
    else await expect(editing.length).toBeGreaterThan(0);
  },
} satisfies Meta<typeof ViewSample>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Tools: Story = {};
export const ReadOnly: Story = { args: { readOnly: true } };
export const Narrow: Story = { args: { narrow: true } };
export const Filter: Story = {
  play: async ({ canvasElement }) => {
    await clickStory(canvasElement, '[data-tool="filter"]');
    await expect(
      await visibleStoryElement(canvasElement, '[data-testid="filter-editor"]'),
    ).toBeVisible();
  },
};
async function openAside(canvas: HTMLElement, tool: string): Promise<void> {
  const header = await storyElement(canvas, '[data-testid="view-header"]');
  if (header.dataset.layout === 'row') await clickStory(canvas, `[data-tool="${tool}"]`);
  else {
    await clickStory(canvas, '[data-tool="more"]');
    await clickStory(canvas, `[data-action="${tool}"]`);
  }
}
export const Sort: Story = {
  play: async ({ canvasElement }) => {
    await openAside(canvasElement, 'sort');
    await expect(
      await visibleStoryElement(canvasElement, '[data-testid="sort-editor"]'),
    ).toBeVisible();
  },
};
export const Properties: Story = {
  play: async ({ canvasElement }) => {
    await openAside(canvasElement, 'properties');
    await expect(
      await visibleStoryElement(canvasElement, '[data-testid="properties-panel"]'),
    ).toBeVisible();
  },
};

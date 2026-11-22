import type { Meta, StoryObj } from '@storybook/svelte';
import { expect, waitFor } from 'storybook/test';
import ViewSample from './ViewSample.svelte';
import { clickStory, storyElement } from './interactions.ts';

const meta = {
  title: 'Views/Kanban',
  component: ViewSample,
  args: { kind: 'board' },
  play: async ({ canvasElement, args }) => {
    const root = await storyElement(canvasElement, '[data-testid="kanban"]');
    const count = typeof args.count === 'number' ? args.count : 8;
    await waitFor(() => expect(root.querySelectorAll('[data-card]')).toHaveLength(count));
    if (count > 0) await expect(root).toHaveTextContent('Example card 001');
    if (args.readOnly)
      await expect(root.querySelectorAll('[data-column-add], [data-column-new]')).toHaveLength(0);
    else await expect(root.querySelector('[data-column-add], [data-column-new]')).not.toBeNull();
  },
} satisfies Meta<typeof ViewSample>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Cards: Story = {};
export const Empty: Story = { args: { count: 0 } };
export const ThreeHundredCards: Story = { args: { count: 300 } };
export const ReadOnly: Story = { args: { readOnly: true } };
/** A column of 400 cards on a phone's width: the cards out of sight are not laid out or painted. */
export const LongColumn: Story = {
  args: { count: 600, narrow: true },
  play: async (context) => {
    await meta.play(context);
    const column = await storyElement(context.canvasElement, '[data-column-cards="todo"]');
    await expect(column.querySelectorAll('[data-card]')).toHaveLength(400);
    const last = column.lastElementChild;
    if (!last) throw new Error('the column has no cards');
    await expect(getComputedStyle(last).contentVisibility).toBe('auto');
  },
};

export const OpenCard: Story = {
  play: async ({ canvasElement }) => {
    await clickStory(canvasElement, '[data-card="example-card-1"]');
    await waitFor(() =>
      expect(canvasElement.querySelector('[data-testid="opened-card"]')).toHaveTextContent(
        'example-card-1',
      ),
    );
  },
};

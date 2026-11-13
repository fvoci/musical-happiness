import type { Meta, StoryObj } from '@storybook/svelte';
import { expect, waitFor } from 'storybook/test';
import WorkspaceSample from './WorkspaceSample.svelte';
import { clickStory, storyElement } from './interactions.ts';

const meta = { title: 'Workspace/Switcher', component: WorkspaceSample } satisfies Meta<
  typeof WorkspaceSample
>;
export default meta;
type Story = StoryObj<typeof meta>;

export const CurrentWorkspace: Story = {};
export const Empty: Story = { args: { count: 0 } };
export const OneWorkspace: Story = { args: { count: 1 } };
export const LongName: Story = { args: { long: true } };
export const TwoHundredWorkspaces: Story = {
  args: { count: 200, long: true },
  play: async ({ canvasElement }) => {
    await clickStory(canvasElement, '[data-testid="workspace-switcher"]');
    await storyElement(canvasElement, '[data-action="example-workspace-199"]');
  },
};
export const SwitchWorkspace: Story = {
  play: async ({ canvasElement }) => {
    await clickStory(canvasElement, '[data-testid="workspace-switcher"]');
    await clickStory(canvasElement, '[data-action="example-workspace-2"]');
    await waitFor(() =>
      expect(canvasElement.querySelector('[data-testid="workspace-choice"]')).toHaveTextContent(
        'example-workspace-2',
      ),
    );
  },
};

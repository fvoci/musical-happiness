import type { Meta, StoryObj } from '@storybook/svelte';

import BodySample from './BodySample.svelte';
import { FROM_TEMPLATE, INVALID, MIXED } from './body-samples.ts';

/** The read-only view of a document body (src/lib/editor/BodyView.svelte). */
const meta = {
  title: 'Editor/Body',
  component: BodySample,
} satisfies Meta<typeof BodySample>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Every kind of node and mark a body may contain. */
export const Mixed: Story = { args: { doc: MIXED } };
/** One document of the built-in templates. */
export const FromTemplate: Story = { args: { doc: FROM_TEMPLATE } };
/** A body the validator refuses: nothing of it is drawn, the caller's notice is. */
export const Invalid: Story = { args: { doc: INVALID } };
/** No body at all. */
export const Empty: Story = { args: { doc: null } };

import type { Meta, StoryObj } from '@storybook/svelte';

import BodyEditorSample from './BodyEditorSample.svelte';
import { DIAGRAMS, FORMULAS, INVALID, MIXED } from './body-samples.ts';

/** The editor of a document body (src/lib/editor/BodyEditor.svelte), with the view of what it would store. */
const meta = {
  title: 'Editor/BodyEditor',
  component: BodyEditorSample,
} satisfies Meta<typeof BodyEditorSample>;

export default meta;
type Story = StoryObj<typeof meta>;

/** An empty body: type `@` for people, `:` for emoji, `$x^2$` for a formula, ```` ```mermaid ```` for a diagram. */
export const Empty: Story = { args: { doc: null } };
/** Every kind of node and mark a body had before formulas. */
export const Mixed: Story = { args: { doc: MIXED } };
/** Formulas: press one to change its source. */
export const Formulas: Story = { args: { doc: FORMULAS } };
/** Diagrams: the source and the picture drawn from it. */
export const Diagrams: Story = { args: { doc: DIAGRAMS } };
/** Not editable. */
export const ReadOnly: Story = { args: { doc: FORMULAS, editable: false } };
/** A body the validator refuses is not loaded: the editor stays empty and read-only, and says so. */
export const Unreadable: Story = { args: { doc: INVALID } };

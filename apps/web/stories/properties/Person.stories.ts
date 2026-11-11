/**
 * One person, picked from the people of the board. Typing narrows the list by
 * user name, nickname, first and last name.
 */
import type { Meta, StoryObj } from '@storybook/svelte';

import PropertyStory from './PropertyStory.svelte';

const meta = {
  title: 'Properties/Person',
  component: PropertyStory,
  args: { type: 'person' },
} satisfies Meta<typeof PropertyStory>;

export default meta;
type Story = StoryObj<typeof meta>;

export const WithValue: Story = { args: { value: 'u-jisoo' } };

export const Empty: Story = {};

export const ReadOnly: Story = { args: { value: 'u-ada', readOnly: true } };

/** Starts with the list open. */
export const Editing: Story = { args: { value: 'u-minho', startOpen: true } };

/** Someone who is no longer among the people of the board. */
export const UnknownUser: Story = { args: { value: 'u-gone' } };

/** Somebody without a display name: the login name alone. */
export const LoginNameOnly: Story = { args: { value: 'u-bot' } };

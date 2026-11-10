/** The value of a person property: one person, picked from the people the caller passes. */
import { describe, expect, it } from 'vitest';

import {
  closePicker,
  fireEvent,
  mount,
  press,
  screen,
  type,
  USERS,
  valueButton,
  within,
} from './support.ts';

const button = (): HTMLElement => valueButton('My person');
const search = (): HTMLInputElement => screen.getByRole('combobox', { name: 'My person' });
const list = (): HTMLElement => screen.getByRole('listbox', { name: 'My person' });
const row = (name: string | RegExp): HTMLElement => within(list()).getByRole('option', { name });
const names = (): (string | null)[] =>
  within(list())
    .queryAllByRole('option')
    .map((option) => option.textContent.trim());
const isOpen = (): boolean => button().getAttribute('aria-expanded') === 'true';

async function open(): Promise<void> {
  await fireEvent.click(button());
}

describe('a person property, closed', () => {
  it('shows the person by user name', () => {
    mount('person', 'user-2');
    expect(button()).toHaveAccessibleName('My person minho');
  });

  it('shows the person the way names are set to be shown', () => {
    mount('person', 'user-2', { nameFormat: 'nickname_full_name' });
    expect(button()).toHaveAccessibleName('My person Mino');
  });

  it('shows the full name when that is the format, and the user name when there is none', async () => {
    const mounted = mount('person', 'user-1', { nameFormat: 'full_name' });
    expect(button()).toHaveAccessibleName('My person 지수 김');
    await mounted.setProps({
      users: [{ id: 'user-1', username: 'jisoo', nickname: '', firstname: '', lastname: '' }],
    });
    expect(button()).toHaveAccessibleName('My person jisoo');
  });

  it('shows a placeholder without a value', () => {
    mount('person', '');
    expect(within(button()).getByText('Empty')).toBeInTheDocument();
  });

  it('shows someone who is not among the people as an unknown user', () => {
    mount('person', 'user-gone');
    expect(button()).toHaveAccessibleName('My person Unknown user');
  });
});

describe('a person property, opened', () => {
  it('lists the people and marks the one in the value', async () => {
    mount('person', 'user-2');
    await open();
    expect(names()).toEqual(['jisoo', 'minho', 'ada']);
    expect(row('minho')).toHaveAttribute('aria-selected', 'true');
  });

  it('has no entry for the reader themselves: the list is the people passed in', async () => {
    mount('person', '');
    await open();
    expect(names()).toEqual(USERS.map((user) => user.username));
    expect(within(list()).queryByText('Me')).toBeNull();
  });

  it('stores the person that is pressed, once, and closes', async () => {
    const { onChange } = mount('person', '');
    await open();
    await fireEvent.click(row('ada'));
    expect(onChange).toHaveBeenCalledExactlyOnceWith('user-3');
    expect(isOpen()).toBe(false);
  });

  it('closes without storing when the same person is pressed', async () => {
    const { onChange } = mount('person', 'user-3');
    await open();
    await fireEvent.click(row('ada'));
    expect(onChange).not.toHaveBeenCalled();
    expect(isOpen()).toBe(false);
  });

  it('narrows the list by user name, nickname, first and last name', async () => {
    mount('person', '');
    await open();
    await type(search(), 'MINO');
    expect(names()).toEqual(['minho']);
    await type(search(), 'lovelace');
    expect(names()).toEqual(['ada']);
    await type(search(), '지수');
    expect(names()).toEqual(['jisoo']);
    await type(search(), 'ada lovelace');
    expect(names()).toEqual(['ada']);
  });

  it('says so when nobody matches, and offers nothing to create', async () => {
    mount('person', '');
    await open();
    await type(search(), 'nobody');
    expect(names()).toEqual([]);
    expect(screen.getByRole('status')).toHaveTextContent('No matches');
  });

  it('picks with the keyboard', async () => {
    const { onChange } = mount('person', '');
    await open();
    await press(search(), 'ArrowDown');
    await press(search(), 'Enter');
    expect(onChange).toHaveBeenCalledExactlyOnceWith('user-2');
  });

  it('clears the value', async () => {
    const { onChange } = mount('person', 'user-2');
    await open();
    await fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
    expect(onChange).toHaveBeenCalledExactlyOnceWith('');
    expect(isOpen()).toBe(false);
  });

  it('can clear an unknown user', async () => {
    const { onChange } = mount('person', 'user-gone');
    await open();
    await fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
    expect(onChange).toHaveBeenCalledExactlyOnceWith('');
  });

  it('closes on Escape and stores nothing', async () => {
    const { onChange } = mount('person', 'user-2');
    await open();
    await closePicker();
    expect(isOpen()).toBe(false);
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe('a person property that may not be edited', () => {
  it('shows the name and cannot be opened', () => {
    const { container, onChange } = mount('person', 'user-2', { readOnly: true });
    expect(screen.queryByRole('button')).toBeNull();
    expect(container).toHaveTextContent('minho');
    expect(onChange).not.toHaveBeenCalled();
  });

  it('is the same in display mode', () => {
    const { container } = mount('person', 'user-2', { mode: 'display' });
    expect(screen.queryByRole('button')).toBeNull();
    expect(container).toHaveTextContent('minho');
  });

  it('shows the placeholder, or nothing, without a value', async () => {
    const mounted = mount('person', '', { readOnly: true });
    expect(mounted.container).toHaveTextContent('Empty');
    await mounted.setProps({ showEmptyPlaceholder: false });
    expect(mounted.container).toHaveTextContent('');
  });
});

describe('a person property that starts in the editing state', () => {
  it('has its list open at once', () => {
    mount('person', '', { startOpen: true });
    expect(names()).toEqual(['jisoo', 'minho', 'ada']);
  });
});

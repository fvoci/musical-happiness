/**
 * The parts of the shell that have behaviour of their own, in a simulated
 * page: a setting with a few answers, the title of a board as a field, and
 * the sidebar over a workspace on the in-memory backend. Layout, the stages
 * and dragging are the browser tests' business (e2e/sidebar.spec.ts).
 */
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { routePath } from '../../../src/app/router.svelte.ts';
import BoardIcon from '$lib/features/shell/BoardIcon.svelte';
import BoardTitle from '$lib/features/shell/BoardTitle.svelte';
import SettingChoice from '$lib/features/shell/SettingChoice.svelte';
import SettingsDialog from '$lib/features/shell/SettingsDialog.svelte';
import Sidebar from '$lib/features/shell/Sidebar.svelte';
import type { ContainerPlace } from '$lib/logic/urls.ts';

import { loadTemplates, settle, setup } from '../../state/support.ts';

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe('a setting with a few answers', () => {
  const OPTIONS = [
    { value: 'center', label: 'In a dialog' },
    { value: 'side', label: 'Beside the board', disabled: true, note: 'Coming soon' },
    { value: 'full', label: 'Full screen' },
  ];

  function mount(value = 'center'): { onChange: ReturnType<typeof vi.fn> } {
    const onChange = vi.fn();
    render(SettingChoice, {
      props: { name: 'cardOpen', label: 'Cards open', value, options: OPTIONS, onChange },
    });
    return { onChange };
  }

  it('is a radio group named by its label, with the chosen answer checked', () => {
    mount();
    const group = screen.getByRole('radiogroup', { name: 'Cards open' });
    expect(within(group).getAllByRole('radio')).toHaveLength(3);
    expect(within(group).getByRole('radio', { name: 'In a dialog' })).toBeChecked();
    expect(within(group).getByRole('radio', { name: 'Full screen' })).not.toBeChecked();
  });

  it('pressing another answer asks for it; pressing the chosen one asks for nothing', async () => {
    const { onChange } = mount();
    await fireEvent.click(screen.getByRole('radio', { name: 'In a dialog' }));
    expect(onChange).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByRole('radio', { name: 'Full screen' }));
    expect(onChange).toHaveBeenCalledExactlyOnceWith('full');
  });

  it('an answer that is not offered is shown with its note and cannot be pressed', async () => {
    const { onChange } = mount();
    const side = screen.getByRole('radio', { name: /Beside the board/ });
    expect(side).toBeDisabled();
    expect(side).toHaveTextContent('Coming soon');
    await fireEvent.click(side);
    expect(onChange).not.toHaveBeenCalled();
  });

  it('Tab stops at the chosen answer only', () => {
    mount('full');
    expect(screen.getByRole('radio', { name: 'Full screen' })).toHaveAttribute('tabindex', '0');
    expect(screen.getByRole('radio', { name: 'In a dialog' })).toHaveAttribute('tabindex', '-1');
  });

  it('Tab stops at the first answer on offer when the chosen one is not', () => {
    mount('side');
    expect(screen.getByRole('radio', { name: 'In a dialog' })).toHaveAttribute('tabindex', '0');
  });

  it('the arrow keys move the choice, past an answer that is not offered and around the ends', async () => {
    const { onChange } = mount();
    const first = screen.getByRole('radio', { name: 'In a dialog' });
    const last = screen.getByRole('radio', { name: 'Full screen' });

    await fireEvent.keyDown(first, { key: 'ArrowRight' });
    expect(onChange).toHaveBeenLastCalledWith('full');
    expect(last).toHaveFocus();

    await fireEvent.keyDown(last, { key: 'ArrowDown' });
    // The value is the caller's: it is still "center", so going on to it asks for nothing new.
    expect(first).toHaveFocus();
    await fireEvent.keyDown(first, { key: 'ArrowLeft' });
    expect(onChange).toHaveBeenLastCalledWith('full');
    expect(last).toHaveFocus();
    expect(onChange).toHaveBeenCalledTimes(2);
  });

  it('other keys are left alone', async () => {
    const { onChange } = mount();
    await fireEvent.keyDown(screen.getByRole('radio', { name: 'In a dialog' }), { key: 'a' });
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe('the title of a board as a field', () => {
  function mount(title = 'Roadmap'): {
    onChange: ReturnType<typeof vi.fn>;
    field: HTMLInputElement;
  } {
    const onChange = vi.fn();
    render(BoardTitle, { props: { title, onChange } });
    return { onChange, field: screen.getByRole('textbox', { name: 'Board title' }) };
  }

  const type = (field: HTMLInputElement, text: string): Promise<boolean> =>
    fireEvent.input(field, { target: { value: text } });

  it('shows the title, and a placeholder for a board without one', () => {
    expect(mount().field).toHaveValue('Roadmap');
    cleanup();
    const { field } = mount('');
    expect(field).toHaveValue('');
    expect(field).toHaveAttribute('placeholder', 'Untitled board');
  });

  it('hands the text on when the field is left, once, without the spaces around it', async () => {
    const { onChange, field } = mount();
    await type(field, '  Launch  ');
    expect(onChange).not.toHaveBeenCalled();
    await fireEvent.blur(field);
    expect(onChange).toHaveBeenCalledExactlyOnceWith('Launch');
  });

  it('Enter leaves the field, and so saves', async () => {
    const { onChange, field } = mount();
    field.focus();
    await type(field, 'Launch');
    await fireEvent.keyDown(field, { key: 'Enter' });
    expect(field).not.toHaveFocus();
    expect(onChange).toHaveBeenCalledExactlyOnceWith('Launch');
  });

  it('Escape puts the title back and saves nothing', async () => {
    const { onChange, field } = mount();
    field.focus();
    await type(field, 'Something else');
    await fireEvent.keyDown(field, { key: 'Escape' });
    expect(field).not.toHaveFocus();
    expect(field).toHaveValue('Roadmap');
    expect(onChange).not.toHaveBeenCalled();
  });

  it('a text that is the title again saves nothing', async () => {
    const { onChange, field } = mount();
    await type(field, 'Roadmap ');
    await fireEvent.blur(field);
    expect(onChange).not.toHaveBeenCalled();
    expect(field).toHaveValue('Roadmap');
  });

  it('the Enter that confirms a syllable of an input method does not save', async () => {
    const { onChange, field } = mount();
    field.focus();
    await type(field, '로드맵');
    await fireEvent.keyDown(field, { key: 'Enter', isComposing: true });
    expect(field).toHaveFocus();
    expect(onChange).not.toHaveBeenCalled();
  });

  it('a title can be emptied', async () => {
    const { onChange, field } = mount();
    await type(field, '   ');
    await fireEvent.blur(field);
    expect(onChange).toHaveBeenCalledExactlyOnceWith('');
  });
});

describe('what stands for a board', () => {
  it('its icon, when it has one', () => {
    const { container } = render(BoardIcon, {
      props: { icon: '🎯', title: 'Roadmap', locale: 'en' },
    });
    expect(container).toHaveTextContent('🎯');
    expect(container.querySelector('[data-board-icon="icon"]')).toHaveAttribute('aria-hidden');
  });

  it('else the first character of its title: a letter in capitals, a whole syllable of Hangul', () => {
    const first = render(BoardIcon, { props: { icon: '', title: 'roadmap', locale: 'en' } });
    expect(first.container).toHaveTextContent('R');
    cleanup();
    const second = render(BoardIcon, { props: { icon: undefined, title: '회의록', locale: 'ko' } });
    expect(second.container).toHaveTextContent('회');
    expect(second.container.querySelector('[data-board-icon="initial"]')).not.toBeNull();
  });
});

describe('the sidebar over a workspace', () => {
  async function mount(): Promise<{
    workspace: ReturnType<typeof setup>['workspace'];
    onOpenBoard: ReturnType<typeof vi.fn>;
    onOpenSettings: ReturnType<typeof vi.fn>;
    onAddBoard: ReturnType<typeof vi.fn>;
    onClose: ReturnType<typeof vi.fn>;
  }> {
    const { workspace } = setup(await loadTemplates());
    await workspace.load();
    const first = workspace.sidebar.flatMap((entry) => entry.boards)[0];
    const handlers = {
      onOpenBoard: vi.fn(),
      onOpenSettings: vi.fn(),
      onAddBoard: vi.fn(),
      onClose: vi.fn(),
    };
    render(Sidebar, {
      props: {
        workspace,
        boardId: first?.id,
        boardHref: (board: ContainerPlace) =>
          routePath({ workspaceId: board.workspaceId, boardId: board.id }, ''),
        beside: true,
        ...handlers,
      },
    });
    return { workspace, ...handlers };
  }

  const titles = (): string[] =>
    screen.getAllByRole('link').map((link) => link.lastElementChild?.textContent.trim() ?? '');

  it('lists the categories in the reader’s order, each with its boards in the reader’s order', async () => {
    const { workspace } = await mount();
    const expected = workspace.sidebar.flatMap((entry) => entry.boards.map((board) => board.title));
    expect(expected).toHaveLength(13);
    expect(titles()).toEqual(expected);
    expect(
      [...document.querySelectorAll('[data-category-toggle]')].map((toggle) =>
        toggle.textContent.trim(),
      ),
    ).toEqual(['Team', 'Research', 'Personal', 'General']);
  });

  it('marks the board on the page, and a link is its address', async () => {
    const { workspace } = await mount();
    const first = workspace.sidebar[0]?.boards[0];
    const link = screen.getByRole('link', { name: first?.title });
    expect(link).toHaveAttribute('aria-current', 'page');
    expect(link).toHaveAttribute(
      'href',
      routePath({ workspaceId: workspace.info?.id, boardId: first?.id }, ''),
    );
    expect(screen.getAllByRole('link').filter((one) => one.hasAttribute('aria-current'))).toEqual([
      link,
    ]);
  });

  it('a plain click opens the board in place; a click with a modifier is left to the browser', async () => {
    const { workspace, onOpenBoard } = await mount();
    const board = workspace.sidebar[0]?.boards[2];
    const link = screen.getByRole('link', { name: board?.title });

    const plain = new MouseEvent('click', { bubbles: true, cancelable: true });
    link.dispatchEvent(plain);
    expect(plain.defaultPrevented).toBe(true);
    expect(onOpenBoard).toHaveBeenCalledExactlyOnceWith(board);

    const modified = new MouseEvent('click', { bubbles: true, cancelable: true, ctrlKey: true });
    // Keep the simulated page from trying to follow the link.
    link.addEventListener(
      'click',
      (event) => {
        event.preventDefault();
      },
      { once: true },
    );
    link.dispatchEvent(modified);
    expect(onOpenBoard).toHaveBeenCalledTimes(1);
  });

  it('the search narrows the boards and leaves out the categories without a match', async () => {
    await mount();
    const search = screen.getByRole('textbox', { name: 'Find a board or wiki' });
    await fireEvent.input(search, { target: { value: 'goal' } });
    expect(titles()).toEqual(['Personal Goals', 'Company Goals & OKRs']);

    await fireEvent.input(search, { target: { value: 'nothing like this' } });
    expect(screen.queryAllByRole('link')).toHaveLength(0);
    // Said once the backend's search has found nothing either.
    await vi.waitFor(() => {
      expect(screen.getByRole('status')).toHaveTextContent(
        'No board or wiki has that in its title.',
      );
    });

    // Escape empties the field first.
    await fireEvent.keyDown(search, { key: 'Escape' });
    expect(search).toHaveValue('');
    expect(screen.getAllByRole('link')).toHaveLength(13);
  });

  it('a board can be picked up only while the list is not narrowed', async () => {
    await mount();
    expect(document.querySelectorAll('[data-drag-item]')).toHaveLength(13);
    expect(document.querySelectorAll('[data-drag-list]')).toHaveLength(4);
    await fireEvent.input(screen.getByRole('textbox', { name: 'Find a board or wiki' }), {
      target: { value: 'goal' },
    });
    expect(document.querySelectorAll('[data-drag-item]')).toHaveLength(0);
    expect(document.querySelectorAll('[data-drag-list]')).toHaveLength(0);
  });

  it('folding a category hides its boards, saves the fold, and makes its heading a place to drop on', async () => {
    const { workspace } = await mount();
    const toggle = screen.getByRole('button', { name: 'Research' });
    expect(toggle).toHaveAttribute('aria-expanded', 'true');

    await fireEvent.click(toggle);
    await settle();
    await tick();
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(screen.getAllByRole('link')).toHaveLength(10);
    const research = workspace.categories.find((category) => category.name === 'Research');
    expect(research?.collapsed).toBe(true);
    expect(document.querySelector('[data-drag-zone]')).toHaveAttribute(
      'data-drag-zone',
      research?.id,
    );
  });

  it('the button at the top folds a sidebar that stands beside the board', async () => {
    const { onClose } = await mount();
    await fireEvent.click(screen.getByRole('button', { name: 'Collapse sidebar' }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('the foot has "Add board or wiki" and the reader’s account, named by the reader', async () => {
    const { onAddBoard, onOpenSettings } = await mount();
    await fireEvent.click(screen.getByRole('button', { name: 'Add board or wiki' }));
    expect(onAddBoard).toHaveBeenCalledOnce();
    await fireEvent.click(
      screen.getByRole('button', { name: '김하나 (@hana): account and settings' }),
    );
    expect(onOpenSettings).toHaveBeenCalledOnce();
  });

  it('every button that is only an icon has a name', async () => {
    await mount();
    for (const button of screen.getAllByRole('button')) {
      expect(button, button.outerHTML.slice(0, 120)).toHaveAccessibleName();
    }
  });
});

describe('an import from the settings', () => {
  it('says in a line how many members it left out, nothing when none, and forgets it when closed', async () => {
    const { workspace } = setup(await loadTemplates());
    await workspace.load();
    const answers = [{ membersLeftOut: 2 }, { membersLeftOut: 0 }, { membersLeftOut: 1 }];
    vi.spyOn(workspace, 'importArchive').mockImplementation(() => Promise.resolve(answers.shift()));
    const dialog = render(SettingsDialog, { props: { open: true, workspace } });
    const choose = async () => {
      const field = await screen.findByTestId('import-archive');
      const file = new File(['PK'], 'boards.boardarchive');
      Object.defineProperty(field, 'files', { value: [file], configurable: true });
      await fireEvent.change(field);
      await settle();
      await tick();
    };
    const line = () => screen.getByTestId('import-left-out');

    await choose();
    expect(line()).toHaveTextContent(
      /^Members left out of the archive's boards and wikis, .*: 2\.$/,
    );
    expect(line()).toHaveAttribute('role', 'status');
    await choose();
    expect(line()).toBeEmptyDOMElement();
    await choose();
    expect(line()).toHaveTextContent(/: 1\.$/);
    await fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
    await tick();
    await dialog.rerender({ open: true, workspace });
    await tick();
    expect(line()).toBeEmptyDOMElement();
  });
});

<!--
  The editor of a document body: the description of a card, a comment.

  Everything comes in as values and callbacks; nothing here knows the server
  or the app's state. The caller fetches this component when a body is first
  edited (`await import('$lib/editor/BodyEditor.svelte')`): the editing
  library is large and no part of the first screen.

  The document is read when the component is made and again whenever
  `revision` goes up, and at no other time: a change of `doc` alone never
  overwrites what is being written. Every edit is reported through `onChange`
  in the stored form, `null` for an empty body.

  A body the validator refuses is not loaded. Editing it would mean saving
  the editor's guess at what it meant over the original (which may come from
  a newer version of the app). The editor then stays empty and read-only and
  says so; `onChange` is never called.
-->
<script lang="ts">
  import { Editor } from '@tiptap/core';
  import { onMount, untrack } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import { getLocale } from '$lib/i18n/paraglide/runtime.js';

  import type { BodyEditorProps } from './body-editor.ts';
  import './editor.css';
  import './editing-place.css';
  import { loadEmoji, searchEmoji } from './emoji.ts';
  import { bodyExtensions } from './extensions.ts';
  import { cancelMathEditing } from './math-editing.ts';
  import { emptyDoc, isEmptyDoc, toStoredDoc, validateDoc, type DocNode } from './schema.ts';
  import { suggestionList } from './suggestion-list.ts';
  import { ImageUpload } from './uploads.ts';

  let {
    doc,
    revision = 0,
    editable = true,
    users = [],
    resolveUser,
    onChange,
    onFocus,
    onBlur,
    uploadImage,
    placeholder = '',
    class: className,
    label,
    toolbar,
    onLoad,
  }: BodyEditorProps = $props();

  let host: HTMLDivElement;
  let editor = $state.raw<Editor>();
  let changes = $state(0);
  /** Whether the document that was last read could not be loaded. */
  let unreadable = $state(false);
  let uploads = $state(0);
  let uploadFailed = $state(false);

  const status = $derived.by(() => {
    if (unreadable) return m.editor_body_unreadable();
    if (uploads > 0) return m.editor_image_uploading();
    return uploadFailed ? m.editor_image_failed() : '';
  });

  const nameOf = (userId: string): string =>
    resolveUser?.(userId) ?? users.find((user) => user.id === userId)?.name ?? userId;

  /** The document to load for a stored body; undefined when it must not be edited. */
  function loadable(stored: DocNode | null): DocNode | undefined {
    if (stored === null) return emptyDoc();
    return validateDoc(stored).length === 0 ? stored : undefined;
  }

  /** What an editor holds, as it would be stored; `null` for an empty body. */
  function storedOf(current: Editor): DocNode | null {
    const stored = toStoredDoc(current.getJSON() as DocNode);
    return isEmptyDoc(stored) ? null : stored;
  }

  const rootAttributes = (): Record<string, string> => ({
    class: ['doc-body', className].filter(Boolean).join(' '),
    'aria-label': label ?? m.editor_body_label(),
    'aria-multiline': 'true',
  });

  /**
   * The editor's own events, run as from outside any effect. A focused
   * editor that is taken out of the page (its card closes) blurs while
   * Svelte removes it, inside the effect that removes it; a state change
   * there is otherwise refused (`state_unsafe_mutation`).
   */
  const fromEditor =
    <A extends unknown[]>(handle: (...args: A) => void) =>
    (...args: A): void => {
      untrack(() => {
        handle(...args);
      });
    };

  onMount(() => {
    const first = loadable(doc);
    unreadable = first === undefined;
    const created = new Editor({
      element: host,
      // The styles the library would put into the page are in editor.css.
      injectCSS: false,
      editable: editable && first !== undefined,
      content: first ?? emptyDoc(),
      extensions: [
        ...bodyExtensions({
          placeholder,
          mention: {
            search: (query) => {
              const wanted = query.toLowerCase();
              return users
                .filter((user) => user.name.toLowerCase().includes(wanted))
                .slice(0, 8)
                .map((user) => ({ value: user.id, label: user.name }));
            },
            labelOf: nameOf,
            render: suggestionList({ kind: 'mention', label: m.editor_suggest_people }),
          },
          emoji: {
            search: async (query) => {
              const found = searchEmoji(await loadEmoji(getLocale()), query);
              return found.map(({ emoji, name }) => ({ value: emoji, label: `${emoji} ${name}` }));
            },
            render: suggestionList({ kind: 'emoji', label: m.editor_suggest_emoji }),
          },
        }),
        ImageUpload.configure({
          upload: () => uploadImage,
          uploadingLabel: m.editor_image_uploading,
          onProgress: (pending, failed) => {
            uploads = pending;
            uploadFailed = failed;
          },
        }),
      ],
      editorProps: { attributes: rootAttributes() },
      onTransaction: fromEditor(() => {
        changes += 1;
      }),
      onUpdate: fromEditor(({ editor: current }: { editor: Editor }) =>
        onChange?.(storedOf(current)),
      ),
      onFocus: fromEditor(() => onFocus?.()),
      onBlur: fromEditor(() => onBlur?.()),
    });
    editor = created;
    onLoad?.(first === undefined ? undefined : storedOf(created));
    return () => {
      cancelMathEditing();
      created.destroy();
    };
  });

  // A change from outside: read `doc` again. Not on the first run, which the
  // editor's own creation covers, and never because `doc` itself changed.
  let seen: number | undefined;
  $effect(() => {
    const next = revision;
    const current = editor;
    if (!current) return;
    if (seen === undefined || seen === next) {
      seen = next;
      return;
    }
    seen = next;
    untrack(() => {
      const fresh = loadable(doc);
      unreadable = fresh === undefined;
      cancelMathEditing();
      current.commands.setContent(fresh ?? emptyDoc(), { emitUpdate: false });
      current.setEditable(editable && fresh !== undefined, false);
      onLoad?.(fresh === undefined ? undefined : storedOf(current));
    });
  });

  $effect(() => {
    editor?.setEditable(editable && !unreadable, false);
  });

  $effect(() => {
    const attributes = rootAttributes();
    editor?.setOptions({ editorProps: { attributes } });
  });

  // Names that become known later: a mention shows the name it had when it
  // was drawn, so the ones in the page are brought up to date.
  function renameMentions(
    current: Editor | undefined,
    name: (userId: string) => string,
    ..._known: unknown[]
  ): void {
    if (!current) return;
    for (const mention of current.view.dom.querySelectorAll<HTMLElement>(
      'span[data-mention][data-user-id]',
    )) {
      const shown = `@${name(mention.dataset.userId ?? '')}`;
      if (mention.textContent !== shown) mention.textContent = shown;
    }
  }
  $effect(() => {
    // Handing both over is what makes this run again when either changes.
    renameMentions(editor, nameOf, users, resolveUser);
  });
</script>

<div class="doc-editor" data-unreadable={unreadable || undefined}>
  {#if toolbar && editor}
    {@render toolbar({ editor, changes })}
  {/if}
  <div bind:this={host} class="doc-editor-text"></div>
  <!-- Said aloud when it changes: an upload under way, an upload that failed, a body that cannot be edited. -->
  <p class="doc-editor-status" role="status" data-shown={status === '' ? undefined : ''}>
    {status}
  </p>
</div>

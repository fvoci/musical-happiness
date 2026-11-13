<!--
  The editor of a document body: the description of a card, a comment.

  Everything comes in as values and callbacks; nothing here knows the server
  or the app's state. The caller fetches this component when a body is first
  edited (`await import('$lib/editor/BodyEditor.svelte')`): the editing
  library is large and no part of the first screen.

  Bound to a shared document (`collab`, a card's body in its room), the
  editor shows what the document holds and writes into it: `doc`,
  `revision`, `onLoad` and `onChange` are not read. What the editor cannot
  draw is kept, the editor says so and stops editing until it fits again.

  Without one (the stories, and the editor's own tests), the document is
  read when the component is made and again whenever `revision` goes up,
  and at no other time: a change of `doc` alone never overwrites what is
  being written. Every edit is reported through `onChange` in the stored
  form, `null` for an empty body. A body the validator refuses is not
  loaded: editing it would mean saving the editor's guess at what it meant
  over the original. The editor then stays empty and read-only and says so.
-->
<script lang="ts">
  import { Editor } from '@tiptap/core';
  import { Collaboration } from '@tiptap/extension-collaboration';
  import { ySyncPluginKey } from '@tiptap/y-tiptap';
  import { onMount, untrack } from 'svelte';

  import { m } from '$lib/i18n/paraglide/messages.js';
  import { getLocale } from '$lib/i18n/paraglide/runtime.js';

  import { BODY_FIELD, type BodyEditorProps } from './body-editor.ts';
  import { setMayWrite } from './shared-place.ts';
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
    collab,
    onEditor,
    canWrite = false,
    onImageDropped,
    onFitsAgain,
  }: BodyEditorProps = $props();

  let host: HTMLDivElement;
  let editor = $state.raw<Editor>();
  let changes = $state(0);
  /** Whether the document that was last read could not be loaded. */
  let unreadable = $state(false);
  /** A formula's source the reader wrote was not kept: they may no longer write. */
  let formulaDropped = $state(false);
  let uploads = $state(0);
  /** How the upload that ended last did, when that is to be said. */
  let uploadEnded = $state<'failed' | 'gone' | 'refused'>();

  const status = $derived.by(() => {
    if (unreadable) return m.editor_body_unreadable();
    if (formulaDropped) return m.editor_formula_refused();
    if (uploads > 0) return m.editor_image_uploading();
    if (uploadEnded === 'gone') return m.editor_image_gone();
    if (uploadEnded === 'refused') return m.editor_image_refused();
    return uploadEnded === 'failed' ? m.editor_image_failed() : '';
  });

  const nameOf = (userId: string): string =>
    resolveUser?.(userId) ?? users.find((user) => user.id === userId)?.name ?? userId;

  /** The document to load for a stored body; undefined when it must not be edited. */
  function loadable(stored: DocNode | null): DocNode | undefined {
    if (stored === null) return emptyDoc();
    return validateDoc(stored).length === 0 ? stored : undefined;
  }

  /** Whether a shared document the editor could not draw fits it again. */
  function fitsAgain(current: Editor): boolean {
    const sync = ySyncPluginKey.getState(current.state);
    return sync?.binding.invalidContentError === null;
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
    // A field for the app's keys even while read-only for now (undo-keys.ts).
    'data-body-editor': '',
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
    // Bound to a shared document, the editor takes what is in it; `doc` is not read.
    const shared = collab;
    const first = shared ? emptyDoc() : loadable(doc);
    unreadable = first === undefined;
    const created = new Editor({
      element: host,
      // The styles the library would put into the page are in editor.css.
      injectCSS: false,
      editable: editable && first !== undefined,
      ...(shared
        ? {
            // What the editor cannot draw (written by a newer version of the app) is
            // kept, not deleted for everyone: the editor says so and stops editing
            // until the shared document fits again (the server mends what it may).
            enableContentCheck: true,
            onContentError: fromEditor(() => {
              unreadable = true;
            }),
          }
        : { content: first ?? emptyDoc() }),
      extensions: [
        ...(shared ? [Collaboration.configure({ document: shared, field: BODY_FIELD })] : []),
        ...bodyExtensions({
          history: !shared,
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
          onProgress: (pending, outcome, destroyed) => {
            uploads = pending;
            if (destroyed) {
              if (outcome === 'gone') onImageDropped?.();
              return;
            }
            if (outcome !== undefined) uploadEnded = outcome === 'done' ? undefined : outcome;
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
    setMayWrite(created.view, {
      may: () => shared !== undefined && canWrite,
      dropped: fromEditor(() => {
        formulaDropped = true;
      }),
    });
    // What it could not draw is gone again: the editor, which took nothing from the shared
    // document meanwhile, is made anew from it (never made editable on what it last showed).
    const fits = (): void => {
      if (unreadable && fitsAgain(created)) onFitsAgain?.();
    };
    shared?.on('afterTransaction', fits);
    if (!shared) onLoad?.(first === undefined ? undefined : storedOf(created));
    onEditor?.(created);
    return () => {
      shared?.off('afterTransaction', fits);
      cancelMathEditing();
      created.destroy();
    };
  });

  // A change from outside: read `doc` again. Not on the first run, which the
  // editor's own creation covers, and never because `doc` itself changed.
  // A shared document is its own: nothing is read into it from outside.
  let seen: number | undefined;
  $effect(() => {
    const next = revision;
    const current = editor;
    if (!current || untrack(() => collab)) return;
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

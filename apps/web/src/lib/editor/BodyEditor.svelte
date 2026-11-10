<!--
  The editor of a body. A STAND-IN: the full editor (suggestions for people
  and emoji, images, formulas, diagrams) is being built to these same props
  and replaces this file. Fetch it late, when a card is first opened:

    const { default: BodyEditor } = await import('$lib/editor/BodyEditor.svelte');
-->
<script lang="ts">
  import { Editor } from '@tiptap/core';
  import { onMount } from 'svelte';

  import './editor.css';
  import { bodyExtensions } from './extensions.ts';
  import { emptyDoc, toStoredDoc, type DocNode } from './schema.ts';

  interface Props {
    /** The stored document, or null for an empty body. Read when the component is made and when `revision` changes. */
    doc: DocNode | null;
    /** Raise it to make the editor take `doc` again. Until then what is being edited is never overwritten. */
    revision?: number;
    editable?: boolean;
    /** People that can be mentioned. */
    users?: readonly { id: string; name: string }[];
    /** The name of a mentioned person; undefined when unknown. */
    resolveUser?: (userId: string) => string | undefined;
    /** Called on every change with the document in its stored form; null when it is empty. */
    onChange?: (doc: DocNode | null) => void;
    onFocus?: () => void;
    onBlur?: () => void;
    /** Uploads a pasted or dropped image and answers with its file identifier. */
    uploadImage?: (file: File) => Promise<string>;
    placeholder?: string;
    class?: string;
  }

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
    class: className = '',
  }: Props = $props();

  let host = $state<HTMLElement>();
  let editor: Editor | undefined;
  let taken = -1;

  function isEmpty(stored: DocNode): boolean {
    const only = stored.content?.length === 1 ? stored.content[0] : undefined;
    return only?.type === 'paragraph' && (only.content?.length ?? 0) === 0;
  }

  onMount(() => {
    editor = new Editor({
      element: host,
      editable,
      content: doc ?? emptyDoc(),
      extensions: bodyExtensions({
        placeholder,
        mention: {
          search: (query) =>
            users
              .filter((user) => user.name.toLowerCase().includes(query.toLowerCase()))
              .map((user) => ({ value: user.id, label: user.name })),
          labelOf: (userId) => resolveUser?.(userId) ?? userId,
        },
      }),
      editorProps: {
        attributes: { class: `doc-body ${className}` },
        // The stand-in takes no images; the full editor uploads what is pasted or dropped.
        handlePaste: () => uploadImage === undefined && false,
      },
      onUpdate: ({ editor: current }) => {
        const stored = toStoredDoc(current.getJSON() as DocNode);
        onChange?.(isEmpty(stored) ? null : stored);
      },
      onFocus: () => onFocus?.(),
      onBlur: () => onBlur?.(),
    });
    taken = revision;
    return () => {
      editor?.destroy();
      editor = undefined;
    };
  });

  $effect(() => {
    if (!editor || revision === taken) return;
    taken = revision;
    editor.commands.setContent(doc ?? emptyDoc(), { emitUpdate: false });
  });

  $effect(() => {
    editor?.setEditable(editable);
  });
</script>

<div bind:this={host} data-testid="body-editor"></div>

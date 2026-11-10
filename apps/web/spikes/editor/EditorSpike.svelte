<script lang="ts">
  import { Editor } from '@tiptap/core';
  import BoldIcon from '@lucide/svelte/icons/bold';
  import CodeIcon from '@lucide/svelte/icons/code';
  import Heading1Icon from '@lucide/svelte/icons/heading-1';
  import Heading2Icon from '@lucide/svelte/icons/heading-2';
  import ImageIcon from '@lucide/svelte/icons/image';
  import ItalicIcon from '@lucide/svelte/icons/italic';
  import ListIcon from '@lucide/svelte/icons/list';
  import ListChecksIcon from '@lucide/svelte/icons/list-checks';
  import ListOrderedIcon from '@lucide/svelte/icons/list-ordered';
  import MinusIcon from '@lucide/svelte/icons/minus';
  import QuoteIcon from '@lucide/svelte/icons/quote';
  import SquareCodeIcon from '@lucide/svelte/icons/square-code';
  import StrikethroughIcon from '@lucide/svelte/icons/strikethrough';
  import { onMount, type Component } from 'svelte';

  import BodyView from '$lib/editor/BodyView.svelte';
  import { bodyExtensions } from '$lib/editor/extensions.ts';
  import { setFileUrlResolver } from '$lib/editor/files.ts';
  import { markdownToBlocks } from '$lib/editor/markdown.ts';
  import { storedSize, toStoredDoc, validateDoc, type DocNode } from '$lib/editor/schema.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { EMOJI, fakeFileUrl, SAMPLES, USERS } from './fixtures.ts';
  import { suggestionList } from './suggestion-list.ts';

  let host: HTMLDivElement;
  let editor = $state<Editor>();
  let stored = $state<DocNode>(SAMPLES.empty ?? { type: 'doc' });
  // Bumped on every transaction so that the toolbar re-reads what is active.
  let revision = $state(0);
  let markdown = $state('');
  let images = 0;

  const problems = $derived(validateDoc(stored));
  const bytes = $derived(storedSize(stored));

  // Before anything is drawn: the read-only view asks for addresses as soon as it has a document.
  setFileUrlResolver(fakeFileUrl);
  const nameOf = (userId: string): string | undefined =>
    USERS.find((user) => user.value === userId)?.label;

  onMount(() => {
    const created = new Editor({
      element: host,
      extensions: bodyExtensions({
        placeholder: '내용을 입력하세요. @ 로 사람을, : 로 이모지를 찾습니다.',
        mention: {
          search: (query) =>
            USERS.filter((user) => user.label.toLowerCase().includes(query.toLowerCase())),
          labelOf: (userId) => nameOf(userId) ?? userId,
          render: suggestionList('mention'),
        },
        emoji: {
          search: (query) =>
            EMOJI.filter((emoji) => emoji.label.includes(`:${query.toLowerCase()}`)),
          render: suggestionList('emoji'),
        },
      }),
      editorProps: { attributes: { class: 'doc-body min-h-64 p-4', 'aria-label': '본문' } },
      onTransaction: () => {
        revision += 1;
      },
      onUpdate: ({ editor: current }) => {
        stored = toStoredDoc(current.getJSON() as DocNode);
      },
      onCreate: ({ editor: current }) => {
        stored = toStoredDoc(current.getJSON() as DocNode);
      },
    });
    editor = created;
    return () => {
      created.destroy();
    };
  });

  function load(name: string): void {
    const sample = SAMPLES[name];
    if (sample) editor?.commands.setContent(sample, { emitUpdate: true });
  }

  function importMarkdown(): void {
    const { content } = markdownToBlocks(markdown);
    editor?.commands.setContent(
      { type: 'doc', content: content.length > 0 ? content : [{ type: 'paragraph' }] },
      { emitUpdate: true },
    );
  }

  function isActive(name: string, attrs?: Record<string, unknown>): boolean {
    // Reading `revision` makes this depend on the editor's state.
    return revision >= 0 && (editor?.isActive(name, attrs) ?? false);
  }

  type Tool = { label: string; icon: Component; active: () => boolean; run: () => void };
  const tools: Tool[] = [
    {
      label: '굵게',
      icon: BoldIcon,
      active: () => isActive('bold'),
      run: () => editor?.chain().focus().toggleBold().run(),
    },
    {
      label: '기울임',
      icon: ItalicIcon,
      active: () => isActive('italic'),
      run: () => editor?.chain().focus().toggleItalic().run(),
    },
    {
      label: '취소선',
      icon: StrikethroughIcon,
      active: () => isActive('strike'),
      run: () => editor?.chain().focus().toggleStrike().run(),
    },
    {
      label: '코드',
      icon: CodeIcon,
      active: () => isActive('code'),
      run: () => editor?.chain().focus().toggleCode().run(),
    },
    {
      label: '제목 1',
      icon: Heading1Icon,
      active: () => isActive('heading', { level: 1 }),
      run: () => editor?.chain().focus().toggleHeading({ level: 1 }).run(),
    },
    {
      label: '제목 2',
      icon: Heading2Icon,
      active: () => isActive('heading', { level: 2 }),
      run: () => editor?.chain().focus().toggleHeading({ level: 2 }).run(),
    },
    {
      label: '목록',
      icon: ListIcon,
      active: () => isActive('bulletList'),
      run: () => editor?.chain().focus().toggleBulletList().run(),
    },
    {
      label: '번호 목록',
      icon: ListOrderedIcon,
      active: () => isActive('orderedList'),
      run: () => editor?.chain().focus().toggleOrderedList().run(),
    },
    {
      label: '할 일 목록',
      icon: ListChecksIcon,
      active: () => isActive('taskList'),
      run: () => editor?.chain().focus().toggleTaskList().run(),
    },
    {
      label: '인용',
      icon: QuoteIcon,
      active: () => isActive('blockquote'),
      run: () => editor?.chain().focus().toggleBlockquote().run(),
    },
    {
      label: '코드 블록',
      icon: SquareCodeIcon,
      active: () => isActive('codeBlock'),
      run: () => editor?.chain().focus().toggleCodeBlock().run(),
    },
    {
      label: '구분선',
      icon: MinusIcon,
      active: () => false,
      run: () => editor?.chain().focus().setHorizontalRule().run(),
    },
    {
      label: '이미지',
      icon: ImageIcon,
      active: () => false,
      run: () => {
        images += 1;
        editor
          ?.chain()
          .focus()
          .setFileImage({ fileId: `file-${images}` })
          .run();
      },
    },
  ];
</script>

<main class="mx-auto grid max-w-6xl grid-cols-1 gap-6 p-4 lg:grid-cols-2">
  <section class="grid grid-cols-1 content-start gap-3">
    <h1 class="text-xl font-semibold">편집기 스파이크</h1>
    <div class="flex flex-wrap gap-2">
      {#each Object.keys(SAMPLES) as name (name)}
        <Button
          variant="outline"
          size="sm"
          onclick={() => {
            load(name);
          }}
          data-sample={name}
        >
          예제: {name}
        </Button>
      {/each}
    </div>
    <div class="rounded-lg border border-input-border">
      <div class="flex flex-wrap gap-1 border-b p-1" role="toolbar" aria-label="서식">
        {#each tools as tool (tool.label)}
          <Button
            variant={tool.active() ? 'secondary' : 'ghost'}
            size="icon-sm"
            aria-label={tool.label}
            aria-pressed={tool.active()}
            title={tool.label}
            onclick={tool.run}
          >
            <tool.icon />
          </Button>
        {/each}
      </div>
      <div bind:this={host} data-testid="editor"></div>
    </div>
    <details class="text-sm">
      <summary class="cursor-pointer">마크다운 가져오기</summary>
      <textarea
        class="mt-2 h-32 w-full rounded-md border border-input-border p-2 font-mono text-base md:text-sm"
        aria-label="마크다운"
        bind:value={markdown}></textarea>
      <Button class="mt-2" size="sm" onclick={importMarkdown} data-testid="import-markdown">
        문서로 바꾸기
      </Button>
    </details>
  </section>

  <section class="grid grid-cols-1 content-start gap-3">
    <h2 class="text-lg font-semibold">읽기 전용 표시기</h2>
    <p class="text-sm text-muted-foreground">
      같은 문서를 편집기 없이 그린 것입니다. 편집하면 따라 바뀝니다.
    </p>
    <div class="rounded-lg border border-input-border" data-testid="view">
      <BodyView
        doc={stored}
        resolveUser={nameOf}
        taskLabel={(label: string, checked: boolean) =>
          `${checked ? '끝낸 일' : '할 일'}: ${label || '내용 없음'}`}
        class="min-h-64 p-4"
      >
        {#snippet invalid()}
          <p class="p-4 text-sm text-destructive" data-testid="view-invalid">
            표시할 수 없는 본문입니다.
          </p>
        {/snippet}
        {#snippet empty()}
          <p class="min-h-64 p-4 text-sm text-muted-foreground" data-testid="view-empty">
            빈 본문입니다.
          </p>
        {/snippet}
      </BodyView>
    </div>

    <h2 class="text-lg font-semibold">저장되는 문서</h2>
    <p class="text-sm" data-testid="status" data-valid={problems.length === 0}>
      {bytes.toLocaleString()} 바이트 ·
      {#if problems.length === 0}
        서버가 받는 문서입니다.
      {:else}
        <span class="text-destructive">허용 목록에 어긋납니다: {problems.join(' / ')}</span>
      {/if}
    </p>
    <pre
      class="max-h-[70vh] overflow-auto rounded-lg bg-muted p-3 text-xs"
      data-testid="stored">{JSON.stringify(stored, null, 2)}</pre>
  </section>
</main>

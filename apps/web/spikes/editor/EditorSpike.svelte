<script lang="ts">
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
  import type { Component } from 'svelte';

  import type { BodyEditorTools } from '$lib/editor/body-editor.ts';
  import BodyEditor from '$lib/editor/BodyEditor.svelte';
  import BodyView from '$lib/editor/BodyView.svelte';
  import { setFileUrlResolver } from '$lib/editor/files.ts';
  import { markdownToBlocks } from '$lib/editor/markdown.ts';
  import { emptyDoc, storedSize, validateDoc, type DocNode } from '$lib/editor/schema.ts';
  import { applyTheme, theme } from '$lib/theme.svelte.ts';
  import { Button } from '$lib/ui/button/index.js';

  import { fakeFileUrl, SAMPLES, USERS } from './fixtures.ts';

  /**
   * The product's editor, as a card would use it: the document goes in, with
   * a revision that is raised to have it read again, and every change comes
   * back through `onChange`. What comes back is shown on the right, as the
   * JSON that would be stored and as the read-only view of it.
   */
  /** What the editor was last told to load. */
  let doc = $state.raw<DocNode | null>(null);
  let revision = $state(0);
  /** What would be stored now; `null` is an empty body. */
  let stored = $state.raw<DocNode | null>(null);
  let markdown = $state('');
  let uploaded = 0;
  /** How long the made-up upload takes; a test stretches it to see the placeholder. */
  const uploadDelay = Number(new URLSearchParams(location.search).get('upload-delay') ?? 0);

  const problems = $derived(stored === null ? [] : validateDoc(stored));
  const bytes = $derived(stored === null ? 0 : storedSize(stored));

  // Before anything is drawn: the read-only view asks for addresses as soon as it has a document.
  setFileUrlResolver(fakeFileUrl);
  const nameOf = (userId: string): string | undefined =>
    USERS.find((user) => user.id === userId)?.name;

  /** Hands the editor a document as if it had come from the server. */
  function show(next: DocNode): void {
    doc = next;
    revision += 1;
  }

  function load(name: string): void {
    const sample = SAMPLES[name];
    if (sample) show(sample);
  }

  function importMarkdown(): void {
    const { content } = markdownToBlocks(markdown);
    show(content.length > 0 ? { type: 'doc', content } : emptyDoc());
  }

  /** Stands in for the server: a moment later the file has an identifier. */
  function uploadImage(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
      setTimeout(() => {
        if (file.name.includes('fail')) reject(new Error('made-up failure'));
        else resolve(`file-${(uploaded += 1)}`);
      }, uploadDelay);
    });
  }

  type Tool = {
    label: string;
    icon: Component;
    active: (tools: BodyEditorTools) => boolean;
    run: (tools: BodyEditorTools) => void;
  };
  // Reading `changes` makes the answer depend on the editor's state.
  const isActive =
    (name: string, attrs?: Record<string, unknown>) =>
    ({ editor, changes }: BodyEditorTools): boolean =>
      changes >= 0 && editor.isActive(name, attrs);
  const never = (): boolean => false;
  const tools: Tool[] = [
    {
      label: '굵게',
      icon: BoldIcon,
      active: isActive('bold'),
      run: ({ editor }) => editor.chain().focus().toggleBold().run(),
    },
    {
      label: '기울임',
      icon: ItalicIcon,
      active: isActive('italic'),
      run: ({ editor }) => editor.chain().focus().toggleItalic().run(),
    },
    {
      label: '취소선',
      icon: StrikethroughIcon,
      active: isActive('strike'),
      run: ({ editor }) => editor.chain().focus().toggleStrike().run(),
    },
    {
      label: '코드',
      icon: CodeIcon,
      active: isActive('code'),
      run: ({ editor }) => editor.chain().focus().toggleCode().run(),
    },
    {
      label: '제목 1',
      icon: Heading1Icon,
      active: isActive('heading', { level: 1 }),
      run: ({ editor }) => editor.chain().focus().toggleHeading({ level: 1 }).run(),
    },
    {
      label: '제목 2',
      icon: Heading2Icon,
      active: isActive('heading', { level: 2 }),
      run: ({ editor }) => editor.chain().focus().toggleHeading({ level: 2 }).run(),
    },
    {
      label: '목록',
      icon: ListIcon,
      active: isActive('bulletList'),
      run: ({ editor }) => editor.chain().focus().toggleBulletList().run(),
    },
    {
      label: '번호 목록',
      icon: ListOrderedIcon,
      active: isActive('orderedList'),
      run: ({ editor }) => editor.chain().focus().toggleOrderedList().run(),
    },
    {
      label: '할 일 목록',
      icon: ListChecksIcon,
      active: isActive('taskList'),
      run: ({ editor }) => editor.chain().focus().toggleTaskList().run(),
    },
    {
      label: '인용',
      icon: QuoteIcon,
      active: isActive('blockquote'),
      run: ({ editor }) => editor.chain().focus().toggleBlockquote().run(),
    },
    {
      label: '코드 블록',
      icon: SquareCodeIcon,
      active: isActive('codeBlock'),
      run: ({ editor }) => editor.chain().focus().toggleCodeBlock().run(),
    },
    {
      label: '구분선',
      icon: MinusIcon,
      active: never,
      run: ({ editor }) => editor.chain().focus().setHorizontalRule().run(),
    },
    {
      label: '이미지',
      icon: ImageIcon,
      active: never,
      run: ({ editor }) => {
        uploaded += 1;
        editor
          .chain()
          .focus()
          .setFileImage({ fileId: `file-${uploaded}` })
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
      <Button
        variant="outline"
        size="sm"
        data-testid="toggle-theme"
        onclick={() => {
          applyTheme(theme.resolved === 'dark' ? 'light' : 'dark');
        }}
      >
        밝기 바꾸기
      </Button>
    </div>
    <div class="rounded-lg border border-input-border" data-testid="editor">
      <BodyEditor
        {doc}
        {revision}
        users={USERS}
        resolveUser={nameOf}
        {uploadImage}
        placeholder="내용을 입력하세요. @ 로 사람을, : 로 이모지를, $ 로 수식을 넣습니다."
        class="min-h-64 p-4"
        onChange={(next: DocNode | null) => {
          stored = next;
        }}
        onLoad={(loaded: DocNode | null | undefined) => {
          // What the editor made of the document, not what it was given: the two must be equal.
          stored = loaded ?? null;
        }}
      >
        {#snippet toolbar(state: BodyEditorTools)}
          <div class="flex flex-wrap gap-1 border-b p-1" role="toolbar" aria-label="서식">
            {#each tools as tool (tool.label)}
              <Button
                variant={tool.active(state) ? 'secondary' : 'ghost'}
                size="icon-sm"
                aria-label={tool.label}
                aria-pressed={tool.active(state)}
                title={tool.label}
                onclick={() => {
                  tool.run(state);
                }}
              >
                <tool.icon />
              </Button>
            {/each}
          </div>
        {/snippet}
      </BodyEditor>
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

This codebase is yours: what it's for and how it's built are your call, and you'll keep building on it.

Storybook은 Cloudflare Workers Builds가 지어 올린다(Worker `fvoci-web-storybook`, 누구나 열고 색인은 막는다): master는 `storybook.fvoci.dev`, 다른 브랜치는 그 아래 미리 보기 주소. 설정은 `apps/web/wrangler.jsonc`, 명령은 `apps/web/package.json`의 `storybook:site`·`storybook:deploy`·`storybook:preview`, 머리글과 CSP는 `apps/web/tools/storybook/site.ts`, 고정한 Wrangler는 자기 잠금 파일과 함께 `apps/web/tools/storybook/wrangler/`.

## 스킬

- Rust를 쓰거나 고칠 때: `rust-best-practices` (`.agents/skills/rust-best-practices/SKILL.md`), 리뷰할 때: `rust-review` (`.agents/skills/rust-review/SKILL.md`)
- Svelte(`.svelte`, `.svelte.ts`)를 쓰거나 고칠 때: `svelte-code-writer` (`.agents/skills/svelte-code-writer/SKILL.md`), `svelte-core-bestpractices` (`.agents/skills/svelte-core-bestpractices/SKILL.md`)

## 코드 찾기

LSP 도구로 찾는다. 위치는 줄·글자 모두 1부터. 첫 질의가 "찾지 못함"이면 색인 중이니 한 번 더 묻는다.

- Rust(rust-analyzer): `incomingCalls`·`outgoingCalls`(부르는 쪽·불리는 쪽), `findReferences`, `goToImplementation`, `goToDefinition`, `workspaceSymbol`(이름으로).
- 웹: `.ts`의 함수·메서드를 누가 부르나는 `incomingCalls`(`.svelte`에서 부르는 곳까지 나온다). `.ts`에서의 `findReferences`는 정의 한 줄만 돌려주니 쓰지 않는다. `.svelte`에서는 `findReferences`와 `goToDefinition`.
- 기기마다 한 번: `bash .agents/setup-lsp.sh`.

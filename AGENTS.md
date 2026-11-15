This codebase is yours: what it's for and how it's built are your call, and you'll keep building on it.

## 스킬

- Rust를 쓰거나 고칠 때: `rust-best-practices` (`.agents/skills/rust-best-practices/SKILL.md`)
- Rust를 리뷰할 때: `rust-review` (`.agents/skills/rust-review/SKILL.md`)
- Svelte(`.svelte`, `.svelte.ts`)를 쓰거나 고칠 때: `svelte-code-writer` (`.agents/skills/svelte-code-writer/SKILL.md`), `svelte-core-bestpractices` (`.agents/skills/svelte-core-bestpractices/SKILL.md`)

## 코드 찾기

- Rust: LSP 도구(rust-analyzer, 플러그인 `rust-analyzer-lsp`). `incomingCalls`·`outgoingCalls`(부르는 쪽·불리는 쪽), `findReferences`, `goToImplementation`, `goToDefinition`, `workspaceSymbol`(이름으로). 위치는 줄·글자 모두 1부터. 세션의 첫 질의가 "찾지 못함"이면 색인 중이니 한 번 더 묻는다.
- 웹(`.ts`, `.svelte`)에는 LSP가 없다: `rg`로 찾는다.
- 기기마다 한 번: `rustup component add rust-analyzer`, `claude plugin install rust-analyzer-lsp@claude-plugins-official`.

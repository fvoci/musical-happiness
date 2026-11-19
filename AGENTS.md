This codebase is yours: what it's for and how it's built are your call, and you'll keep building on it.

## 스킬

- Rust를 쓰거나 고칠 때: `rust-best-practices` (`.agents/skills/rust-best-practices/SKILL.md`)
- Rust를 리뷰할 때: `rust-review` (`.agents/skills/rust-review/SKILL.md`)
- Svelte(`.svelte`, `.svelte.ts`)를 쓰거나 고칠 때: `svelte-code-writer` (`.agents/skills/svelte-code-writer/SKILL.md`), `svelte-core-bestpractices` (`.agents/skills/svelte-core-bestpractices/SKILL.md`)

## 코드 찾기

LSP 도구로 찾는다. 위치는 줄·글자 모두 1부터. 첫 질의가 "찾지 못함"이면 색인 중이니 한 번 더 묻는다.

- Rust(rust-analyzer): `incomingCalls`·`outgoingCalls`(부르는 쪽·불리는 쪽), `findReferences`, `goToImplementation`, `goToDefinition`, `workspaceSymbol`(이름으로).
- 웹: `.ts`의 함수·메서드를 누가 부르나는 `incomingCalls`(`.svelte`에서 부르는 곳까지 나온다). `.ts`에서의 `findReferences`는 정의 한 줄만 돌려주니 쓰지 않는다. `.svelte`에서는 `findReferences`와 `goToDefinition`.
- 기기마다 한 번: `bash .agents/setup-lsp.sh`.

## 구조

- `crates/`: 서버(Rust). `model`(도메인 타입) → `store`(DB, 스키마는 `store/schema/{sqlite,postgres}`) → `collab`(함께 고치기) → `server`(axum). 층 규칙은 `xtask/src/layers.rs`가 검사한다.
- `apps/web/`: 웹앱(Svelte 5, pnpm 워크스페이스).
- `contracts/`: 서버와 웹앱의 계약(`openapi.json` 등). `xtask/`: 개발 작업. `docker/`: 배포.
- 인수인계: `docs/handoff.md`(서버), `apps/web/docs/handoff.md`(웹앱).

## 검증 (저장소 루트에서)

- 서버: `cargo xtask check`(외부 서비스 없음), `cargo xtask test`(PostgreSQL·S3 컨테이너를 띄운다. Docker가 없으면 `--databases sqlite`).
- 계약: `cargo xtask openapi`(다르면 실패, `--write`로 다시 쓴다). 웹앱 타입은 `pnpm --filter web run api:types`.
- 웹앱: `pnpm install --frozen-lockfile` 뒤 `pnpm --filter web run check`, `test`, `build`, `e2e`.

## 하지 않는 것

- 강제 푸시, 올라간 커밋의 amend·rebase·reset. 고칠 것은 새 커밋으로.
- `xtask/exceptions/`의 수를 손으로 올리기(천장은 `cargo xtask measure`만 올린다), 검사를 끄거나 테스트를 건너뛰어 통과시키기.
- 설정 키를 더하면서 `docs/configuration.md`를 같은 커밋에서 고치지 않기.
- 비밀값(`docker/.env`)·개인 정보 커밋, `.github/workflows` 추가.

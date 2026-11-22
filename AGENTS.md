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

## CI와 스택

- PR마다 `.github/workflows/ci.yml`이 `cargo xtask check`·`test`와 웹앱의 검증 전부를 GitHub의 실행기에서 돈다. 스택의 PR도 자리와 관계없이 모두 전부를 돈다. 볼 검사는 `ci-done` 하나다(작업 하나라도 실패·취소·건너뜀이면 빨강). 로컬에서는 고친 곳의 시험만 돌린다.
- 스택(`gh stack`)으로 올릴 때는 tracer-bullet 하나가 브랜치 하나, PR 하나다. 처음 `gh stack init --base master fvoci/<묶음>-t1`, 다음 `gh stack add fvoci/<묶음>-t2`(브랜치 `fvoci/<묶음>`이 있으면 `fvoci/<묶음>/t1`은 만들 수 없다), 올리기는 `gh stack submit --auto --open`(새 PR)과 `gh stack push`. 아래 브랜치를 고치면 그 브랜치에 새 커밋을 얹고 `gh stack rebase --upstack`, `gh stack push`. master가 앞서면 `gh stack rebase`, `gh stack push`(합치기가 아니다). 스택의 상태와 잠금(`.git/gh-stack*`)은 모든 워크트리가 함께 쓰고, 트렁크는 로컬 `master`다: `git rev-list --count origin/master..master`가 0이 아니면 아직 올리지 않은 그 커밋이 PR에 드니 쌓지 않는다. 충돌로 멈춘 `gh stack rebase`는 그 자리에서 `--continue`나 `--abort`로 끝낸다(그동안 다른 워크트리의 `gh stack`이 거절된다). `rerere`는 켜지 않는다(설정이 모든 워크트리에 걸린다).

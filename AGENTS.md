This codebase is yours: what it's for and how it's built are your call, and you'll keep building on it.

## 스킬

- Rust를 쓰거나 고칠 때: `rust-best-practices` (`.claude/skills/rust-best-practices/SKILL.md`)
- Rust를 리뷰할 때: `rust-review` (`.claude/skills/rust-review/SKILL.md`)
- Svelte(`.svelte`, `.svelte.ts`)를 쓰거나 고칠 때: `svelte-code-writer` (`.claude/skills/svelte-code-writer/SKILL.md`), `svelte-core-bestpractices` (`.claude/skills/svelte-core-bestpractices/SKILL.md`)

## MCP (`.mcp.json`)

- `serena`: Rust 기호를 rust-analyzer로 찾는다. `find_referencing_symbols`(누가 쓰나), `find_implementations`, `find_declaration`, `find_symbol`, `get_symbols_overview`. `name_path`는 `impl Collab/forget`처럼 쓴다. 세션의 첫 질의는 색인하느라 1분쯤 걸린다. 웹 코드에는 쓰지 않는다.
- `codebase-memory`: 저장소의 그래프. 세션 처음에 `index_repository`(`repo_path`는 이 워크트리의 절대 경로), 그 뒤 `trace_path`(부르는 쪽·불리는 쪽), `detect_changes`(`base_branch`에 비교할 커밋: 바뀐 것이 닿는 곳), `search_graph`, `query_graph`(Cypher), `get_architecture`. 같은 이름이 여럿이면 답이 내미는 `qualified_name`으로 다시 묻는다.
- 둘 다 빠뜨릴 수 있다. 특히 `.svelte`에서 쓰는 곳과 인스턴스 메서드 호출(`wiki.movePage(...)`)이다. "쓰는 곳이 없다"는 고치기 전에 `rg`로 확인한다.
- 기기마다 한 번: `uv`, `rustup component add rust-analyzer`, PATH의 `codebase-memory-mcp`(github.com/DeusData/codebase-memory-mcp 릴리스, `checksums.txt`로 확인).

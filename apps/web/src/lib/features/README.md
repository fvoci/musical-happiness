# 기능 폴더

화면은 기능별로 `features/<기능>/` 아래에 둔다(예: `board`, `card`, `sidebar`, `views/kanban`).

규칙(ESLint `fvoci/feature-boundaries`, `tools/lint/layers.ts`):

- 기능 폴더는 다른 기능 폴더에서 가져오지 않는다. `$lib/features/다른기능/…`도, `../다른기능/…`도 안 된다.
- 두 기능이 같이 쓰는 것은 아래 층으로 내린다: 계산은 `lib/logic`, 상태는 `lib/state`, 서버와의 통신은 `lib/api`, 화면 부품은 `lib/ui`.
- 아래 층(`logic`, `state`, `api`, `ui` 등 `features`가 아닌 모든 폴더)은 기능 폴더에서 가져오지 않는다.
- 기능을 조립하는 곳은 `src/app`이다. 거기서는 어느 기능이든 가져올 수 있다.

`features/` 바로 아래의 첫 폴더 이름이 기능의 이름이다. `features/views/kanban`과 `features/views/table`은 같은 기능 `views`에 속한다.

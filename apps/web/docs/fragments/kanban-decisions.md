# 칸반 뷰와 갤러리 뷰: 스스로 정한 것

작성: 2026-11-10 23:40 KST. 기준 커밋 `ad44e4d4` 위에서 한 일이다. 부른 쪽이 문서에 합칠 조각이다. 경로는 `apps/web` 기준이다.

한 줄의 형식: **무엇을** — 왜 그 안을 / 다른 안 / 바꾸려면 어디를.

## 만든 것의 자리

| 무엇                                                                   | 어디                                                                                        |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| 카드의 얼굴(칸반과 갤러리가 함께 쓴다)                                 | `src/lib/components/card-face/CardFace.svelte`, `face.ts`                                   |
| 칸반: 조립, 두 `Sortable`, 놓기                                        | `src/lib/features/views/kanban/Kanban.svelte`                                               |
| 칸반: 열, 카드, 열 메뉴, 카드 메뉴(갤러리도 쓴다)                      | `KanbanColumn.svelte`, `KanbanCard.svelte`, `ColumnMenu.svelte`, `CardMenu.svelte`          |
| 칸반: 숨긴 열과 "+ 그룹 추가", 탭 줄, 끌리는 모습과 안내               | `HiddenColumns.svelte`, `ColumnTabs.svelte`, `DragLayer.svelte`                             |
| 칸반: 순수 함수, 고르기, 한 열씩 넘기기, 저장소에 쓰는 것, 드래그 문구 | `kanban.ts`, `selection.svelte.ts`, `paging.svelte.ts`, `board-data.ts`, `drag-messages.ts` |
| 갤러리                                                                 | `src/lib/features/views/gallery/Gallery.svelte`                                             |

## 카드

- **카드는 `role="link"`인 `div`이고 진짜 `<a href>`가 아니다** — 얼굴 안에 URL 속성의 링크와 복사 단추가 들어가서 `<a>`로 감싸면 링크 안의 링크가 되고, 제목만 `<a>`로 하면 `lib/drag`가 링크 위의 누름을 드래그로 치지 않아 제목을 잡고 끌 수 없다 / 다른 안: 카드 전체를 `<a>`로 하고 얼굴의 URL 속성을 글자로만 그리기(브라우저의 "새 탭에서 열기"를 얻는다) / 바꾸려면 `KanbanCard.svelte`와 `Gallery.svelte`의 `role="link"`인 요소. 카드의 주소(`cardHref`)는 지금 "링크 복사"에만 쓰인다.
- **카드의 "…" 단추는 카드(링크)의 형제로 두고 모서리에 겹쳐 그린다** — 링크 안의 단추는 화면 낭독기에 단추로 전해지지 않는다 / 다른 안: 원본처럼 카드 안에 / `KanbanCard.svelte`, `Gallery.svelte`의 `data-slot="card-menu"`. `CardFace`의 `menuRoom`이 제목 끝에 그 자리를 비운다.
- **값이 없는 속성은 줄을 차지하지 않는다. 체크박스는 늘 그리고 옆에 속성 이름을 적는다** — 빈 줄이 카드를 키우고, 이름 없는 체크박스는 뜻을 알 수 없다 / 다른 안: 원본처럼 모두 그리고 이름은 툴팁으로만 / `components/card-face/face.ts`의 `hasFaceValue`, `CardFace.svelte`. 다른 속성의 이름은 `title`(hover)과 화면 낭독기용 글자로만 있다.
- **본문 표시와 할 일 배지는 있으면 늘 그린다** — 지시가 그렇다. 원본은 뷰의 "배지" 가짜 속성이 켜져 있을 때만 그렸다 / 다른 안: 뷰의 설정을 따르기 / `CardFace.svelte`의 `card-badges`.
- **복제한 카드를 열지 않는다** — 좁은 화면에서 복제하자마자 화면이 카드로 덮이면 무엇이 생겼는지 보이지 않는다. 원본은 연다 / 다른 안: 연다 / `CardMenu.svelte`의 `duplicate`.
- **"링크 복사"의 확인은 단추 아래의 작은 글자("복사되었습니다!", 2초)다** — 앱에 알림(토스트)을 띄우는 자리가 아직 없다 / 다른 안: 틀이 알림을 갖게 되면 거기로 / `CardMenu.svelte`의 `copied`.
- **읽기만 하는 보드의 카드 메뉴에는 "링크 복사"만 있다** — 나머지는 모두 고치는 일이다 / `CardMenu.svelte`의 `actions`.

## 여러 장

- **Ctrl/⌘+누름으로 더하고 빼고, Shift+누름으로 범위, Enter도 같은 키와 함께면 같다. 고르는 동안 위에 "N장 선택됨 · 선택 해제" 줄이 나온다** — 몇 장을 골랐는지와 푸는 길이 보여야 한다 / 다른 안: 줄 없이 표시만 / `selection.svelte.ts`, `Kanban.svelte`의 `selection-bar`.
- **카드 메뉴의 "선택"은 손가락에서만이 아니라 어디서나 있다. 누르면 그 뒤로는 그냥 누름이 고르기가 되고, 고른 것이 0장이 되면 끝난다** — 포인터 종류로 메뉴를 가르지 않으려고(태블릿은 둘 다 쓴다). 키보드로 고르는 길도 된다 / 다른 안: hover가 없는 기기에서만 / `CardMenu.svelte`의 `select`, `Selection.begin`.
- **고른 카드의 메뉴에서 "다른 열로 이동"과 "삭제"는 고른 것 전부에 한다(줄 끝에 장수)** — 끌기 없이 여러 장을 옮기는 길이 있어야 한다 / 다른 안: 늘 그 카드 하나만 / `KanbanColumn.svelte`의 `targetsOf`.
- **놓거나 옮기면 선택이 풀린다. Escape는 초점이 보드 안에 있을 때만 선택을 푼다** — 스파이크의 동작을 따랐다 / `Kanban.svelte`의 `dropCards`, `attachKeys`.

## 열

- **그룹 속성이 없는 보드 뷰는 묶을 수 있는 첫 속성으로 묶는다** — 원본이 그렇게 그렸고, 씨앗의 보드 뷰 다섯(예: Sales Pipeline CRM의 Pipeline Tracker)이 `groupById` 없이 선택지 id만 갖고 있다. 저장소의 `groupsOf`와 `dropCards`가 같은 규칙을 써야 해서 저장소에 `groupingOf`를 더했다(폴더 밖 변경) / 다른 안: 변환기가 `groupById`를 채워 넣기 / `src/lib/state/board.svelte.ts`의 `groupingOf`.
- **열 이름 바꾸기는 머리글 자리에서 바로 한다(Enter·자리 뜨기는 저장, Escape는 취소). 색은 같은 자리에 뜨는 목록으로, 삭제는 묻고 한다** — 이름은 한 줄이라 대화상자가 과하다 / 다른 안: 선택 속성의 편집기처럼 한 겉에 이름과 색을 함께 / `KanbanColumn.svelte`의 `renaming`, `ColumnMenu.svelte`.
- **선택지를 지워도 카드의 값은 건드리지 않는다. 그 카드들은 값 없는 열에 보이고, 실행 취소하면 제 열로 돌아온다** — 원본도 정의에서만 지운다. 카드를 하나씩 고치면 실행 취소가 카드 수만큼 길어지고 돌이킬 수 없는 손실이 된다 / 다른 안: 카드의 값도 지운다 / `board-data.ts`의 `deleteOption`. 문구는 `kanban_delete_group_description`.
- **"+ 그룹 추가"는 "새 그룹"이라는 이름의 기본색 선택지를 맨 끝 열로 더하고 곧바로 이름을 고치게 한다. 속성 정의와 뷰의 변경은 실행 취소 한 번이다** / 다른 안: 이름을 먼저 묻기 / `board-data.ts`의 `addGroup`, `Kanban.svelte`의 `addNewGroup`.
- **다시 보이게 한 열은 맨 끝에 선다** — 숨기기 전의 자리를 뷰가 기억하지 않는다 / `kanban.ts`의 `withColumnShown`.
- **열 메뉴에 "왼쪽으로"·"오른쪽으로"를 두었다(지시에 없던 것)** — Q24의 권고(좁은 화면에서는 메뉴 항목을 먼저)이고, 한 열씩 넘기는 화면에서는 끌리는 열이 화면을 다 가린다 / 빼려면 `ColumnMenu.svelte`의 `left`·`right`.
- **사람 속성으로 묶인 뷰**: 열 이름은 그 사람의 표시 이름이고, 이름·색·삭제·순서 바꾸기·"그룹 추가"가 없다(열이 선택지가 아니다). 숨기기는 되고 뷰의 `hiddenOptionIds`만 쓴다. 만든 사람·고친 사람으로 묶이면 열의 "+"와 열 사이 이동도 없다(그 값은 정할 수 없다) / `Kanban.svelte`의 `isOption`, `settable`.
- **열마다 머리글의 "+" 말고 바닥에도 "+ 새로 만들기"가 있다** — 원본과 목업이 둘 다 가졌다. 긴 열에서는 바닥 쪽이 가깝다 / 빼려면 `KanbanColumn.svelte`의 `data-column-new`.
- **열의 계산은 카드 수만 보여 준다** — 지시대로. 원본의 `kanbanCalculations`(합계 등)는 읽지 않는다. 다른 계산은 테이블의 계산 행이 한다 / 더하려면 `KanbanColumn.svelte`의 `data-column-count` 자리에 `lib/logic/calculations.ts`.

## 드래그

- **놓은 모습을 저장소보다 먼저 그린다** — 저장소의 `dropCards`는 자리(뷰의 순서)를 먼저 쓰고 그 답이 온 뒤에 열(카드의 속성)을 쓴다. 그 사이(메모리 구현에서 40–80ms) 카드가 떠난 열에 도로 보였다가 옮겨 가고, 키보드로 놓은 카드는 초점을 잃었다. 그래서 놓는 순간의 배치를 저장소의 쓰기가 끝날 때까지 그대로 그린다 / 다른 안: `dropCards`가 두 변경을 한꺼번에 먼저 보여 주게 고치기(저장소의 일이다. 질문으로 남긴다) / `Kanban.svelte`의 `settling`, `kanban.ts`의 `listsAfterDrop`.
- **놓인 자리를 "그 열의 모든 카드 가운데의 자리"로 바꿔 넘긴다** — 저장소는 필터가 가린 카드까지 세고, 화면은 보이는 카드만 센다. 보이는 카드 가운데 놓인 자리의 바로 뒤 카드 앞, 끝에 놓았으면 마지막으로 보이는 카드 뒤가 된다 / `kanban.ts`의 `indexAmongAll`.
- **정렬된 보기에서 빈자리는 뷰의 순서에서 그 카드가 설 자리다** — 뷰가 준 카드의 순서(`cards`)에서 그 열의 카드 가운데 앞선 것의 수 / `kanban.ts`의 `sortedIndex`.
- **"다른 열로 이동"은 그 열의 끝에 놓는다(정렬된 보기에서는 정렬이 정한다)** / `Kanban.svelte`의 `moveCards`.
- **안내 글은 화면 맨 위 가운데에 띄운다(안전 영역 아래)** — 손가락에 가려지지 않는 곳 / `DragLayer.svelte`.
- **끌리는 열은 카드 여덟 장까지만 그린다** / `DragLayer.svelte`의 `COLUMN_CARDS`.
- **진동 설정을 끄면 `Sortable`을 다시 붙인다** — 포인터의 조정 값은 붙일 때 읽힌다 / `Kanban.svelte`의 `attachDrag`.
- **보드 밖**은 `Sortable`이 붙은 스크롤 상자의 밖이다: 뷰 머리글과 탭 줄 위에 놓으면 취소다. 손가락으로 취소할 길이 그것이다.

## 한 열씩 넘기기

- **"보던 열"의 뜻** — 한 열씩 넘기는 동안에는 멈춘 열이다. 여러 열이 보이는 동안에는 그 열이 화면에서 완전히 나갔을 때만 맨 왼쪽에 보이는 열로 바뀐다. 한 열 ↔ 여러 열로 바뀔 때 그 열을 가운데(한 열) 또는 화면 안(여러 열, 왼쪽에서 12px)으로 데려온다 / 다른 안: 여러 열일 때는 늘 맨 왼쪽 열을 "보던 열"로(그러면 좁혔다 넓혔다 할 때 다른 열로 돌아올 수 있다) / `paging.svelte.ts`.
- **열은 칸 너비에서 양쪽 28px를 뺀 만큼, 많아야 384px이고, 칸이 더 넓으면 가운데에 선다** — 양옆의 여백을 `max(1.75rem, (100% − 24rem) / 2)`로 주었다 / `Kanban.svelte`의 스크롤 상자 클래스, `KanbanColumn.svelte`의 `max-w-sm`.
- **마지막 열 뒤에 "+ 그룹 추가"와 숨긴 열이 한 장 더 있고, 탭 줄에도 그 탭이 있다("숨겨진 열 N", 숨긴 열이 없으면 "더 보기")** — 좁은 칸에서도 같은 부품이어야 창 크기가 바뀔 때 갈아 끼우지 않는다 / 다른 안: 좁은 칸에서는 뷰의 "…" 메뉴로 옮기기 / `HiddenColumns.svelte`, `Kanban.svelte`의 `tabs`.
- **탭 줄의 높이는 40px, 넉넉한 밀도에서 44px. 탭 이름은 12rem에서 줄인다** / `ColumnTabs.svelte`.
- **열 안의 스크롤은 "같은 픽셀"이 아니라 "같은 내용"이 남는다** — 열 너비가 바뀌면 제목이 다르게 접혀 카드 높이가 달라지고, 브라우저의 스크롤 앵커가 보던 카드를 제자리에 둔다. 테스트는 초점이 있던 카드가 여전히 보이는지를 본다.

## 갤러리

- **카드의 최소 너비는 14rem(224px)** — 휴대폰(412px 이하)에서 1열, 칸이 484px부터 2열, 720px부터 3열 / 다른 안: 10rem쯤으로 낮춰 휴대폰에서 2열(속성이 켜진 카드에는 좁다) / `Gallery.svelte`의 `grid-cols-[repeat(auto-fill,minmax(min(100%,14rem),1fr))]`.
- **한 줄의 카드는 높이가 같다** / `Gallery.svelte`의 `h-full`.
- **본문 미리보기와 대표 이미지는 그리지 않는다** — 카드 목록에 본문이 없다(`kanban-contract.md`의 질문).
- **끌어서 순서를 바꾸지 않는다** — 격자는 두 축이고 `Sortable`은 한 축만 다룬다. 순서는 뷰 머리글의 정렬에 맡긴다. 그래서 메뉴에 "선택"과 "다른 열로 이동"도 없다.
- **"+ 새로 만들기"는 격자의 마지막 칸이다(고칠 수 있을 때)**.

## `mobile-proposal.md`의 질문

- **Q3 (A)**: 칸이 544px 미만이면 한 열씩 멈추고(CSS scroll snap) 위에 탭 줄. 바꾸려면 `Kanban.svelte`의 스크롤 상자의 `snap-x snap-mandatory`와 `ColumnTabs.svelte`, 경계는 `app.css`의 `--container-columns`.
- **Q4 (가, 다)**: 끄는 동안에는 snap을 끄고(`dragging ? 'snap-none' : …`) 보드의 `data-drag-scroll="x"`가 가장자리에서 옆 열로 넘긴다. 브라우저 테스트로 확인했다(`e2e/kanban.spec.ts`의 "at the edge of the screen…": 끄는 동안 `scroll-snap-type`이 `none`이고, 놓은 뒤 `x mandatory`로 돌아오며 카드가 다음 열에 놓인다). 카드 메뉴의 "다른 열로 이동"은 `CardMenu.svelte`. (나) 탭 위에 올리면 넘어가기는 만들지 않았다.
- **Q16**: 열의 "+"는 그 열에 만든다. 뷰 머리글의 "새로 만들기"가 "지금 보이는 열에" 만들게 하려면 칸반이 보던 열을 머리글에 알려 줄 길이 필요한데 `ViewProps`에 그 길이 없다. 만들지 않았다(질문으로 남긴다).
- **Q22 (가)**: 카드의 "…"는 `can-hover:`에서만 숨고, 카드나 단추에 초점이 있으면 보인다. `CardMenu.svelte`의 단추 클래스.
- **Q24**: 카드의 터치 드래그(300ms 길게 누르기)와, 지시에 따라 열의 터치 드래그도 된다. 메뉴 항목("왼쪽으로"·"오른쪽으로", "다른 열로 이동")도 함께 있다.
- **Q28**: 갤러리는 열 수만 칸을 따른다.
- **Q31 (가)**: 칸 544px 이상은 256px 열의 자유 스크롤, 992px 이상은 272px. `KanbanColumn.svelte`의 `@columns/pane:w-64 @header/pane:w-68`.
- **Q33 (3)**: 위 "보던 열의 뜻". 열·카드·메뉴는 단계가 바뀌어도 같은 부품이다(클래스만 바뀐다). 진행 중인 드래그는 `Sortable`이 취소한다(테스트 "a window that changes width under a drag cancels it").

## 테스트가 보드의 너비를 정하는 방식

틀이 자리 표시인 동안에는 사이드바(256px)가 늘 떠 있어서 `mobile` 프로젝트의 칸이 156px다. 종단 테스트는 창의 너비를 바꿔 **보드의 너비**를 진짜 틀에서의 값(휴대폰 412, 펼친 기기 570, 데스크톱 1024)으로 맞춘 뒤 시작한다(`e2e/kanban.support.ts`의 `fitPane`, `PANE`). 틀이 바뀌어도 같은 보드를 본다. 그 대신 `mobile` 프로젝트의 창은 지금 668px라 메뉴가 시트가 아니라 드롭다운으로 뜬다(진짜 틀에서는 시트). 메뉴의 줄은 `data-action`으로 찾으므로 둘 다 통과한다.

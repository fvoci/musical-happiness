# 칸반 뷰와 갤러리 뷰: 서버의 계약에 관해 가정한 것과 묻는 것

작성: 2026-11-10 23:40 KST. 부른 쪽이 `docs/contract-assumptions.md`에 합칠 조각이다. 화면은 `lib/state`의 저장소만 썼고 서버의 API 모양을 새로 지어낸 것은 없다.

## 묻는 것

1. **갤러리의 미리보기에 쓸 것을 카드 목록이 실어 줄 수 있는가.** 원본의 갤러리는 카드마다 본문의 앞부분이나 첫 이미지를 보여 준다. 지금의 카드 목록(`Backend.cards`)에는 본문이 없고 `hasBody`, `tasksTotal`, `tasksDone`만 있다. 갤러리가 카드마다 `Backend.card`를 따로 부르지 않게 했으므로 미리보기와 대표 이미지는 그리지 않는다. 필요하면 목록의 카드에 "첫 이미지의 파일 id"와 "본문 앞부분의 글자(예: 200자)" 같은 요약을 실어 주는 길이 있어야 한다. 화면 쪽에서 그릴 자리는 `src/lib/components/card-face/CardFace.svelte`다.

## 가정한 것

1. **선택지의 id는 클라이언트가 만든다.** "+ 그룹 추가"는 `createId('block')`으로 만든 id의 선택지를 보드의 속성 정의에 더한다(`Backend.patchBoard`). 원본과 같고, 선택 속성의 편집기(`onCreateOption`)가 이미 같은 가정을 한다. 서버가 선택지의 id를 정한다면 `src/lib/features/views/kanban/board-data.ts`의 `addGroup`이 답에서 id를 읽게 바꿔야 한다(그 id를 뷰의 `visibleOptionIds`에 넣는다).
2. **선택지를 지워도 서버는 카드의 값을 건드리지 않는다.** 지운 선택지의 id를 값으로 가진 카드는 화면에서 값 없는 열에 보이고, 실행 취소(선택지를 정의에 되돌려 넣기)로 제 열에 돌아온다. 서버가 선택지를 지울 때 카드의 값을 함께 지운다면 실행 취소로 돌아오지 않으므로, 지우기 전에 묻는 글(`kanban_delete_group_description`)을 고쳐야 한다.
3. **뷰의 `visibleOptionIds`, `hiddenOptionIds`는 통째로 쓴다.** 열을 숨기고, 보이고, 순서를 바꾸는 일은 `BoardStore.setViewFields`로 두 목록을 새 값으로 바꾼다. 값 없는 열은 빈 문자열 id(`''`)로 목록에 든다(원본과 같다).
4. **그룹 속성이 없는 보드 뷰.** 뷰의 `groupById`가 비어 있으면 묶을 수 있는 첫 속성으로 묶어 그리고(원본의 동작), 카드를 놓으면 그 속성의 값을 바꾼다. 뷰의 `groupById`를 채워 쓰지는 않는다. 서버나 변환기가 보드 뷰에 `groupById`를 늘 채워 준다면 `BoardStore.groupingOf`의 대체 규칙은 쓰이지 않게 된다.

## 저장소(`lib/state`)에 남기는 것

- `BoardStore.dropCards`는 뷰의 순서를 쓰고 답을 기다린 뒤 카드의 속성을 쓴다. 그 사이에 화면은 카드가 떠난 열에 도로 선 것을 보게 되므로, 칸반이 놓은 모습을 그 호출이 끝날 때까지 스스로 그린다(`Kanban.svelte`의 `settling`). 두 변경을 먼저 한꺼번에 보여 주고 나서 보내도록 저장소를 고치면 그 우회는 지워도 된다.
- 놓은 직후(쓰기가 끝나기 전)의 실행 취소는 이미 끝난 쓰기까지만 되돌린다. 종단 테스트는 쓰기가 끝난 뒤에 실행 취소한다(`data-settling`이 사라질 때까지 기다린다).

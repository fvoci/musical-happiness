# 직접 쓴 공용 부품

여러 기능이 같이 쓰는 화면 부품 가운데 **손으로 쓴 것**을 둔다. `src/lib/ui`는 shadcn-svelte CLI가 만든 코드의 자리라(Prettier 대상에서 빠져 있고 다시 생성하면 덮어쓴다) 직접 쓴 부품을 거기에 두지 않는다.

- 이 폴더는 기능 폴더 바로 아래의 층이다(`src/lib/README.md`의 표). `lib/ui`, `lib/logic`, `lib/editor`, `lib/drag`, `lib/layout`, 표시 설정에서 가져올 수 있고, 기능 폴더(`lib/features/*`)와 `lib/state`에서는 가져오지 못한다(ESLint `fvoci/feature-boundaries`, `fvoci/layer-order`).
- 값과 콜백을 인자로 받는다. API·상태 모듈에 기대지 않는다.

| 부품              | 내용                                                                                                                                   |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `picker-surface/` | 선택 목록과 날짜 선택기를 띄우는 겉. 넓은 화면은 팝오버, 좁은 화면은 바닥 시트이고 어느 쪽인지는 인자 `presentation` 하나가 정한다     |
| `pick-list/`      | 검색 칸이 붙은 선택 목록(한 개·여러 개 고르기, 새 항목 만들기)                                                                         |
| `option-chip/`    | 선택지의 색 이름(`propColorBrown` 등)을 토큰으로 그리는 배지                                                                           |
| `sheet/`          | 바닥 시트. 좁은 단계에서 메뉴와 선택기가 뜨는 겉이다. 뒤로 가기로 닫히고, 가상 키보드 위에 올라앉고, 좌우·아래 안전 영역을 따로 비운다 |
| `menu/`           | 명령의 메뉴(`ActionMenu`). 좁은 단계에서는 바닥 시트, 그 밖에서는 드롭다운이다                                                         |
| `properties/`     | 속성 값의 표시와 편집 15종(`PropertyValue`). 칸반 카드, 테이블 칸, 카드 상세, 갤러리가 함께 쓴다                                       |

## 좁은 화면의 방식을 바꾸려면

메뉴와 선택기가 시트로 뜰지 팝오버로 뜰지는 **`src/lib/layout/presentation.ts`의 `overlayPresentation` 한 함수**가 정한다(지금: 좁은 단계면 시트, 아니면 팝오버). `menu/ActionMenu.svelte`와 `picker-surface/default-presentation.svelte.ts`가 그 함수를 부르고, 속성 편집기와 모든 "…" 메뉴가 이 둘을 거친다. 규칙을 바꾸려면(좁은 단계에서도 팝오버로, 포인터 종류로 가르기 등) 그 함수만 고친다. 한 곳에서만 다르게 하려면 그 호출부가 `presentation` 인자를 넘긴다.

시트의 생김새(높이, 제목 줄, "완료")는 `sheet/BottomSheet.svelte` 한 파일이다.

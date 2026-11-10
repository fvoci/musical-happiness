# 직접 쓴 공용 부품

여러 기능이 같이 쓰는 화면 부품 가운데 **손으로 쓴 것**을 둔다. `src/lib/ui`는 shadcn-svelte CLI가 만든 코드의 자리라(Prettier 대상에서 빠져 있고 다시 생성하면 덮어쓴다) 직접 쓴 부품을 거기에 두지 않는다.

- 이 폴더는 기능 폴더 바로 아래의 층이다(`src/lib/README.md`의 표). `lib/ui`, `lib/logic`, `lib/editor`, `lib/drag`, `lib/layout`, 표시 설정에서 가져올 수 있고, 기능 폴더(`lib/features/*`)와 `lib/state`에서는 가져오지 못한다(ESLint `fvoci/feature-boundaries`, `fvoci/layer-order`).
- 값과 콜백을 인자로 받는다. API·상태 모듈에 기대지 않는다.

| 부품              | 내용                                                                                                                               |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `picker-surface/` | 선택 목록과 날짜 선택기를 띄우는 겉. 넓은 화면은 팝오버, 좁은 화면은 바닥 시트이고 어느 쪽인지는 인자 `presentation` 하나가 정한다 |
| `pick-list/`      | 검색 칸이 붙은 선택 목록(한 개·여러 개 고르기, 새 항목 만들기)                                                                     |
| `option-chip/`    | 선택지의 색 이름(`propColorBrown` 등)을 토큰으로 그리는 배지                                                                       |
| `properties/`     | 속성 값의 표시와 편집 15종(`PropertyValue`). 칸반 카드, 테이블 칸, 카드 상세, 갤러리가 함께 쓴다                                   |

## 좁은 화면의 방식을 바꾸려면

`picker-surface/PickerSurface.svelte` 한 파일만 고친다. 팝오버와 시트 가운데 무엇을 쓸지는 이 부품의 `presentation` 인자(`'popover' | 'sheet'`)가 정하고, 속성 편집기는 모두 이 부품을 거쳐 목록과 달력을 띄운다. 좁은 화면에서 시트 대신 다른 겉(전체 화면 대화 상자 등)을 쓰려면 이 파일의 `sheet` 갈래를 바꾸면 된다.

좁은 화면인지의 판단은 이 부품이 하지 않는다. 호출부가 `presentation`을 넘긴다. 뷰포트 폭으로 기본값을 정하는 보조는 `picker-surface/default-presentation.svelte.ts`의 `PickerPresentation`이다(`$lib/hooks/is-mobile.svelte.ts`를 쓴다). 기준을 바꾸려면(예: 포인터 종류) 그 보조만 고친다.

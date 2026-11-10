# 뷰 머리글: 서버의 계약에 관해 가정한 것

작성: 2026-11-11 00:01 KST. 서버 설계 문서(`docs/s6-design.md`)에는 뷰의 `fields` 안에 무엇이 들어가는지가 없다. 머리글은 새 API를 지어내지 않았고, 저장소(`lib/state`)에 이미 있는 쓰기(`createView`, `duplicateView`, `deleteView`, `changeView`, `setViewFields`)만 쓴다. 아래는 그 쓰기에 **무엇을 담는지**에 관해 원본과 내장 템플릿을 따라 가정한 것이다. `docs/contract-assumptions.md`에 합칠 조각이다.

| 가정                                                                                                                                                                                         | 근거                                                    | 틀리면 고칠 곳                                                    |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- | ----------------------------------------------------------------- |
| 그룹이 없는 뷰는 `fields.groupById`가 빈 문자열 `""`이다(필드를 지우지 않는다)                                                                                                               | 내장 템플릿의 표 뷰, 원본의 "그룹 해제"                 | `features/view-header/view-fields.ts`의 `groupingFields`          |
| `fields.visibleOptionIds`의 빈 문자열 `""`은 값 없는 카드의 열이다                                                                                                                           | `lib/logic/group.ts`, 내장 템플릿                       | 같은 함수                                                         |
| `fields.visiblePropertyIds`에는 속성 id 말고 가짜 id 둘이 들어갈 수 있다: `__title`(갤러리의 제목), `__badges`(댓글·설명 표시). 서버는 보드에 없는 id를 거절하지 않는다                      | 원본의 상수와 내장 템플릿                               | `view-fields.ts`의 `propertyToggles`, `BADGES_COLUMN_ID`          |
| 새 표 뷰는 `fields.columnWidths`에 `{"__title": 280}`을 담아 만든다                                                                                                                          | 원본                                                    | `view-fields.ts`의 `newViewFields`                                |
| 새 뷰를 만들 때 `fields` 전체(빈 목록과 빈 객체 포함)를 보낸다. 새 보드 뷰에 선택 속성이 없으면 `groupById`는 없다(`undefined`라 JSON에 실리지 않는다)                                       | `logic/wire.ts`의 `createBoardView`                     | `newViewFields`                                                   |
| 필터의 조건은 `{propertyId, condition, values}`이고 제목의 조건은 `propertyId`가 `"title"`이다. 사람 조건의 값은 사용자 id, 선택 조건의 값은 선택지 id, 날짜 조건의 값은 `YYYY-MM-DD` 하나다 | `logic/filter.ts`, `logic/wire.ts`의 `formatFilterDate` | `features/view-header/filter-edit.ts`, `FilterDate.svelte`        |
| 필터 안의 중첩된 묶음은 클라이언트가 만들지 않지만 서버가 준 것은 그대로 되돌려 보낸다(필터를 고칠 때 `filter` 전체를 보낸다)                                                                | 원본도 전체를 보낸다                                    | `filter-edit.ts`(모든 함수가 손대지 않은 부분을 같은 객체로 둔다) |
| 뷰의 이름은 빈 문자열일 수 있다                                                                                                                                                              | 원본(빈 이름은 "(제목 없는 뷰)"로 보인다)               | `ViewMenu.svelte`의 `setRenameOpen`                               |
| 마지막 뷰를 지우지 못하게 하는 것은 화면의 일이다. 서버가 따로 막는지는 모른다                                                                                                               | 원본은 화면에서만 막는다                                | `ViewMenu.svelte`의 `disabled`                                    |

## 서버 세션에 물을 것

- 뷰의 `fields`를 서버가 검증하는가. 한다면 위의 가짜 id(`__title`, `__badges`), 빈 `groupById`, 빈 제목을 받아야 한다.
- 뷰의 수정 권한이 카드의 수정 권한과 따로인가. 원본은 따로였다(뷰·속성을 고치는 권한과 카드를 만드는 권한). 지금 화면은 `board.canEdit` 하나로 가른다.

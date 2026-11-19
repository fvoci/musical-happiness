# 원본 번역의 paraglide 메시지 변환

원본 웹앱(`webapp/`, React, react-intl)의 번역 파일 `webapp/i18n/*.json`을 새 웹앱(`apps/web`, paraglide)의 메시지 파일 `apps/web/messages/*.json`으로 옮긴 기록이다. 숫자와 목록은 변환 도구의 보고에서 가져왔다(처음 변환은 2026-11-10 18:49 KST, 아래 "결정을 반영한 것"은 2026-11-10 20:23 KST 실행). 원본 참조는 저장소 기준 상대 경로다. 옛 제품 이름과 옛 회사 이름은 이 문서에 적지 않고 "옛 제품 이름", "옛 회사 이름"이라고 쓴다.

## 요약

- 원본에는 번역 파일이 38개 있지만 원본이 실제로 제공하는 로케일은 17개다. 이 17개와 한국어를 변환했다. 결과 로케일은 18개다. 한국어는 원본의 17개에 들어 있지 않다.
- 영어 메시지 461개 가운데 376개를 남겼다. 34개는 포팅하지 않는 기능의 것이라 뺐고(처음 28개, 결정으로 6개 더), 51개는 원본 코드 어디에서도 쓰지 않아 뺐다. 여기에 이 앱에만 있는 키(`app_*`, `ui_*`. 이 문서를 쓸 때 8개)가 더해져 `en`은 384개다. 처음 변환 때는 390개였다.
- 메시지 id는 그대로다(`BoardComponent.add-a-group`). 코드에서는 `m['BoardComponent.add-a-group']()`로 부른다. 트리 셰이킹은 유지된다.
- 18개 파일 전체에서 쓰인 ICU 구문은 단순 인자 `{name}`과 `<b>` 태그뿐이다. plural, select, selectordinal, number, date, time, `''`, 아포스트로피 인용은 한 번도 나오지 않는다. 구문 오류도 없다.
- 변환하지 않은 메시지는 3개다(`id` 2개, `it` 1개). 영어 메시지에 없는 인자를 쓴다.
- 원본의 문구를 손으로 쓴 문구로 덮어쓴 것이 160개다: `en` 6개(제품 이름 4개, 다시 쓴 것 2개), `ko` 130개, 다른 11개 로케일 24개(제품 이름 23개, `nl`의 합성어 1개). 영어를 다시 쓴 두 메시지는 `ko`를 뺀 로케일에서 번역을 내지 않아 영어로 나온다.
- `ko`는 `en`과 키 집합이 같다. 원본 `ko.json`에 없던 18개를 새로 번역해 채웠다. 고친 한국어와 채운 한국어는 모두 `docs/ko-review.md`에 있다.
- 빈 화면 빌드의 JS는 30,921 → 31,768바이트(br 11,161 → 11,396)다(처음 변환 때의 측정).
- 메시지를 다 쓰는 화면이면 18개 로케일의 문구가 한 번들에 들어간다. 로케일별로 나눠 받는 방법을 조사해 "로케일별로 나눠 받기"에 적었다. 제품의 빌드는 바꾸지 않았다.

## 결정을 반영한 것

처음 변환이 남긴 미결 사항을 제품 책임자가 정했고, 그것을 반영했다(2026-11-10).

| 결정                                                                                       | 한 일                                                                                                                        | 적은 곳                                    |
| ------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| 1. "Boards"를 제품 이름으로 쓰는 메시지는 `fvoci`로. 카테고리 이름으로서의 Boards는 그대로 | 영어 4개를 고쳤다. 후보 5개 가운데 1개는 제품 이름이 아니라서 그대로 뒀다. 다른 로케일은 이름이 라틴 문자로 남은 곳만 고쳤다 | "이름 치환"의 "제품 이름으로 쓰인 Boards"  |
| 2. 네덜란드어 `medefvocigebruikers`                                                        | `andere fvoci-gebruikers`로 고쳤다                                                                                           | "이름 치환"                                |
| 3. 이상해 보이는 기존 한국어를 고치고 용어를 통일                                          | 한국어 130개를 덮어썼다                                                                                                      | `docs/ko-review.md`                        |
| 4. 검수 문서                                                                               | `docs/ko-review.md`                                                                                                          | 그 문서                                    |
| 5. 판단을 미뤘던 메시지: 8개는 남기고, 6개는 빼고, 2개는 다시 쓴다                         | 6개를 뺐고, 2개는 영어와 한국어를 다시 썼다                                                                                  | "제거한 메시지", "판단을 미뤘다가 정한 것" |
| 6. 로케일별로 나눠 받기는 조사만                                                           | 버리는 프로젝트로 네 가지를 해 봤다                                                                                          | "로케일별로 나눠 받기"                     |

이 결정들을 담으려고 도구에 "원본에 있는 문구를 덮어쓰는" 길(`messages-override/`)을 더했다. 도구와 함께 지금은 지웠다.

## 번들 로케일과 태그

원본이 제공하는 로케일은 `webapp/src/i18n.tsx`가 정한다. 이 파일이 `import`하는 번역 파일이 17개이고(4–20행), `supportedLanguages`(24행)와 `getMessages`의 `switch`도 같은 17개다. 언어 메뉴의 목록인 `webapp/src/constants.ts`의 `Constants.languages`도 같은 17개다. 과제에 적힌 "17개"와 맞는다.

`webapp/i18n`의 나머지 21개 파일(`ar`, `ars`, `en_AU`, `et`, `fa`, `he`, `hr`, `hu`, `ka`, `kab`, `kk`, `ko`, `lt`, `ml`, `nb_NO`, `pl`, `pt`, `sk`, `sl`, `uk`, `vi`)은 원본이 번들하지 않는다. 이 가운데 `ko`만 변환했다. 이 앱의 로케일이기 때문이다. 나머지 20개는 건드리지 않았다.

| 태그 (이 앱, BCP 47) | 원본 파일      | 원본이 저장하던 언어 코드   |
| -------------------- | -------------- | --------------------------- |
| `en` (기준)          | `en.json`      | `en`                        |
| `ko`                 | `ko.json`      | 없음 (원본이 제공하지 않음) |
| `ca`                 | `ca.json`      | `ca`                        |
| `de`                 | `de.json`      | `de`                        |
| `el`                 | `el.json`      | `el`                        |
| `es`                 | `es.json`      | `es`                        |
| `fr`                 | `fr.json`      | `fr`                        |
| `id`                 | `id.json`      | `id`                        |
| `it`                 | `it.json`      | `it`                        |
| `ja`                 | `ja.json`      | `ja`                        |
| `nl`                 | `nl.json`      | `nl`                        |
| `oc`                 | `oc.json`      | `oc`                        |
| `pt-BR`              | `pt_BR.json`   | `pt-br`                     |
| `ru`                 | `ru.json`      | `ru`                        |
| `sv`                 | `sv.json`      | `sv`                        |
| `tr`                 | `tr.json`      | `tr`                        |
| `zh-Hans`            | `zh_Hans.json` | `zh-cn`                     |
| `zh-Hant`            | `zh_Hant.json` | `zh-tw`                     |

변환 도구는 실행할 때마다 `webapp/src/i18n.tsx`의 `import` 목록을 읽어 이 표와 다르면 멈췄다. `project.inlang/settings.json`의 `locales`는 이 순서의 18개이고 `baseLocale`은 `en` 그대로다. 원본이 사용자 설정에 저장하던 코드(`pt-br`, `zh-cn`, `zh-tw`)는 새 태그와 다르다. 사용자 설정을 옮긴다면 셋째 열을 써야 한다.

## 지금의 구성

- 변환 도구(`tools/` 아래의 변환기, 원천 폴더 `messages-own/`·`messages-fill/`·`messages-override/`, 뺀 id의 목록)는 2026-11-11에 지웠다(사용자 결정). 그 결과인 `messages/<태그>.json`이 원천이고 손으로 고친다.
- `pnpm --filter web run i18n:keys`(`check`에도 있다)가 로케일별 키 집합을 `en`과 견준다. `en`에 없는 키가 있거나 `ko`에 빠진 키가 있으면 실패한다.
- 아래의 표와 수는 변환했을 때의 기록이다. 덮어쓴 문구의 "이전"(원본 문구)은 이 문서와 `docs/ko-review.md`의 표에만 남아 있다.

## 코드에서 메시지를 부르는 법

paraglide 3.2.1은 점과 하이픈이 있는 키를 그대로 받는다. 컴파일러 소스(`node_modules/@inlang/paraglide-js/dist/compiler/safe-module-id.js`)와 실제 컴파일 결과로 확인했다(2026-11-10 18:26 KST, 버리는 프로젝트를 컴파일해 호출).

- 메시지마다 모듈이 하나 생긴다. 파일 이름은 id를 소문자로 바꾸고 `[a-z0-9_]`가 아닌 글자를 `_`로 바꾼 뒤 대문자 개수를 붙인 것이다: `BoardComponent.add-a-group` → `messages/boardcomponent_add_a_group2.js`.
- 그 모듈은 함수를 **원래 id를 이름으로** 내보낸다. 문자열 이름 export다(ES2022).
  ```js
  export { boardcomponent_add_a_group2 as 'BoardComponent.add-a-group' };
  ```
- 서로 다른 id가 같은 파일 이름이 되면 한쪽이 `$a$2e$b` 꼴의 이름을 받는다. 처음 변환의 390개에서 그런 충돌이 없었고, 그 뒤로는 6개를 뺐을 뿐이다.

부르는 법은 둘이다. 둘 다 `svelte-check`, ESLint, `vite build`를 통과하는 것을 `App.svelte`에 임시로 넣어 확인했다(2026-11-10 18:44 KST, 확인 뒤 되돌림).

```svelte
<script lang="ts">
  import { m } from '$lib/i18n/paraglide/messages.js';
  import { 'BoardComponent.hidden-columns' as hiddenColumns } from '$lib/i18n/paraglide/messages.js';
</script>

{m['BoardComponent.add-a-group']()}
{m['GroupBy.hideEmptyGroups']({ count: 3 })}
{hiddenColumns()}
{m.ui_close()}
```

- 자체 키는 전처럼 `m.ui_close()`다. 기존 호출 코드는 고치지 않았다.
- 타입 검사가 된다. 없는 id(`m['No.such-id']`)와 빠진 인자(`m['GroupBy.hideEmptyGroups']()`)는 `svelte-check`가 오류로 잡는다(2026-11-10 18:50 KST 확인).
- **트리 셰이킹은 유지된다.** 위 세 메시지를 쓴 빌드에는 그 세 메시지의 문구만 들어가고, 쓰지 않은 메시지(`CardDetail.add-property`, `ui_toggle_sidebar`)의 문구는 없었다. `m['…']` 꼴과 문자열 이름 `import` 꼴 모두 그렇다.
- 태그가 있는 메시지는 문자열로 부르면 태그 없이 글자만 나온다. 굵게 그리려면 `.parts()`를 쓴다: `m['FindBoardsDialog.SubTitle'].parts()`는 `{ type: 'markup-start', name: 'b' }`, `{ type: 'text', value: … }`, `{ type: 'markup-end', name: 'b' }`의 배열을 준다.

## ICU 조사

조사 대상은 번들 17개와 `ko`의 원본 파일 전체다. 제거한 메시지도 센다. 원본의 파서(react-intl 5가 쓰는 것)와 같은 규칙으로 파싱했다(변환 도구의 ICU 파서).

| 로케일    | 메시지 | 글자만 | 인자: 메시지/개수 | 태그: 메시지/개수 | plural·select·selectordinal·`#` | number·date·time | `''` | 아포스트로피 인용 | 글자 아포스트로피 | 구문 오류 | en과 인자·태그가 다름 |
| --------- | -----: | -----: | ----------------: | ----------------: | ------------------------------: | ---------------: | ---: | ----------------: | ----------------: | --------: | --------------------: |
| `en`      |    461 |    423 |             37/42 |               3/5 |                               0 |                0 |    0 |                 0 |                22 |         0 |                     0 |
| `ko`      |    437 |    400 |             36/41 |               3/5 |                               0 |                0 |    0 |                 0 |                 0 |         0 |                     0 |
| `ca`      |    195 |    188 |               7/7 |                 0 |                               0 |                0 |    0 |                 0 |                 7 |         0 |                     0 |
| `de`      |    460 |    422 |             37/42 |               3/5 |                               0 |                0 |    0 |                 0 |                 0 |         0 |                     0 |
| `el`      |     47 |     47 |                 0 |                 0 |                               0 |                0 |    0 |                 0 |                 0 |         0 |                     0 |
| `es`      |    226 |    215 |             11/13 |                 0 |                               0 |                0 |    0 |                 0 |                 0 |         0 |                     0 |
| `fr`      |    413 |    378 |             34/39 |               3/5 |                               0 |                0 |    0 |                 0 |                40 |         0 |                     0 |
| `id`      |    149 |    142 |               7/8 |                 0 |                               0 |                0 |    0 |                 0 |                 0 |         0 |                     2 |
| `it`      |    309 |    289 |             19/23 |               2/4 |                               0 |                0 |    0 |                 0 |                 8 |         0 |                     1 |
| `ja`      |    449 |    411 |             37/42 |               3/5 |                               0 |                0 |    0 |                 0 |                 0 |         0 |                     0 |
| `nl`      |    449 |    411 |             37/42 |               3/5 |                               0 |                0 |    0 |                 0 |                 0 |         0 |                     0 |
| `oc`      |    148 |    139 |               9/9 |                 0 |                               0 |                0 |    0 |                 0 |                 1 |         0 |                     0 |
| `pt-BR`   |    460 |    422 |             37/42 |               3/5 |                               0 |                0 |    0 |                 0 |                 0 |         0 |                     0 |
| `ru`      |    428 |    393 |             34/38 |               2/4 |                               0 |                0 |    0 |                 0 |                 0 |         0 |                     0 |
| `sv`      |    449 |    411 |             37/42 |               3/5 |                               0 |                0 |    0 |                 0 |                 0 |         0 |                     0 |
| `tr`      |    460 |    422 |             37/42 |               3/5 |                               0 |                0 |    0 |                 0 |                 0 |         0 |                     0 |
| `zh-Hans` |    449 |    411 |             37/42 |               3/5 |                               0 |                0 |    0 |                 0 |                 0 |         0 |                     0 |
| `zh-Hant` |    449 |    411 |             37/42 |               3/5 |                               0 |                0 |    0 |                 0 |                 0 |         0 |                     0 |

- "글자만"은 구문이 하나도 없는 메시지다. "아포스트로피 인용"은 `'{`, `'}`, `'<`, `'>`로 시작하는 인용이다. "글자 아포스트로피"는 `don't`, `l'année`처럼 글자로 쓰인 아포스트로피가 있는 메시지 수다. 원본 파서에서 아포스트로피는 `{`, `}`, `<`, `>` 앞에서만 인용을 시작하므로 이들은 모두 글자다.
- 인자와 태그 말고는 어떤 구문도 없다. plural·select·selectordinal·`#`, number·date·time, `''`, 아포스트로피 인용, 구문 오류의 열은 모든 로케일에서 0이다.
- 태그는 모두 `<b>`다. `<br/>` 같은 자체 닫힘 태그와 글자로 쓰인 `<`는 없다.
- 영어와 복수 범주가 다른 로케일(`ru`, `ja`, `ko` 등)도 plural을 쓰지 않는다. 수가 들어가는 메시지는 `{count}`를 그대로 넣는다(예: `GroupBy.hideEmptyGroups`).
- 중괄호가 깨졌거나 닫히지 않은 메시지는 없다.
- 영어와 인자가 다른 메시지가 3개 있다(`id` 2, `it` 1). "변환하지 않은 메시지"에 적었다. 영어에 있는 인자를 번역이 빠뜨린 경우는 없다.

`en.json`의 구문별 메시지 id. †는 제거했거나 죽은 id다.

- **인자 `{name}`: 37개 메시지, 42군데.** `AttachmentBlock.addElement`, `AttachmentElement.upload-percentage`, `BoardComponent.no-property`, `BoardComponent.no-property-title`, `BoardsUnfurl.Remainder`†, `BoardsUnfurl.Updated`†, `CardDetailProperty.confirm-delete-subtext`, `CardDetailProperty.confirm-property-name-change-subtext`, `CardDetailProperty.property-deleted`, `CardDetailProperty.property-name-change-subtext`, `ColorOption.selectColor`, `ContentBlock.addElement`, `DeleteBoardDialog.confirm-info`, `DeleteBoardDialog.confirm-info-template`, `FindBoardsDialog.NoResultsFor`, `GroupBy.hideEmptyGroups`, `GroupBy.showHiddenGroups`, `ShareBoard.workspacePermissionsText`, `SidebarCategories.CategoryMenu.DeleteModal.Body`, `UndoRedoHotKeys.canRedo-with-description`, `UndoRedoHotKeys.canUndo-with-description`, `ViewHeader.display-by`, `ViewHeader.group-by`, `boardSelector.confirm-link-board-subtext`†, `boardSelector.confirm-link-board-subtext-with-other-channel`†, `centerPanel.undefined`, `notification-box-card-limit-reached.title`†, `notification-box.card-limit-reached.not-admin.text`†, `notification-box.card-limit-reached.text`†, `person.add-user-to-board`, `person.add-user-to-board-question`, `person.add-user-to-board-warning`, `rhs-boards.last-update-at`†, `rhs-boards.link-boards-to-channel`†, `rhs-boards.no-boards-linked-to-channel`†, `shareBoard.confirm-change-workspace-role.body`, `shareBoard.confirm-link-channel-subtext-with-other-channel`†
- **태그 `<b>`: 3개 메시지, 5군데.** `FindBoardsDialog.SubTitle`, `SidebarCategories.CategoryMenu.DeleteModal.Body`, `shareBoard.confirm-change-workspace-role.body`
- **글자 아포스트로피: 22개 메시지.** `AttachmentBlock.failed`, `BoardComponent.no-property-title`, `CardDialog.editing-template`, `CardDialog.nocard`, `ContentBlock.errorText`, `Filter.not-contains`, `Filter.not-ends-with`, `Filter.not-includes`, `Filter.not-starts-with`, `SidebarCategories.CategoryMenu.DeleteModal.Body`, `VersionMessage.help`†, `WelcomePage.NoThanks.Text`†, `Workspace.editing-board-template`, `createImageBlock.failed`, `error.invalid-read-only-board`, `error.not-logged-in`, `guest-no-board.subtitle`†, `imagePaste.upload-failed`, `login.register-button`, `person.add-user-to-board-warning`, `rhs-board-non-admin-msg`†, `shareBoard.confirm-unlink.body`†
- plural, select, selectordinal, number, date, time, `#`, `''`, 아포스트로피 인용, 중첩 구문: 0개.

## 변환 규칙

설치된 `@inlang/plugin-message-format` 4.6.0의 형식을 그 패키지의 README와 `dist/index.js`(패턴·선언·매치 파서)로 확인했다. 메시지 값은 문자열(단순 메시지)이거나, 객체 하나를 담은 배열(복합 메시지: `declarations`, `selectors`, `match`)이다. 패턴 안에서 `{…}`는 자리표시자이고 글자 `{`, `}`, `\`는 `\{`, `\}`, `\\`로 쓴다. `{#b}…{/b}`는 마크업, `{n: number}`는 함수가 붙은 자리표시자다. 선언은 `input x`, `local y = x: plural`이고, 매치 키는 `선택자=값, 선택자=값`이며 `*`가 나머지 전부다.

실제 자료에 나오는 구문은 처음 넷이다. 나머지는 자료에 없지만 지원하고, 변환 도구의 단위 테스트가 변환 결과와 paraglide로 컴파일한 뒤의 실행 결과를 함께 확인했다.

| 구문              | 변환 전 (ICU)                                                 | 변환 후 (inlang)                                                                                                                      |
| ----------------- | ------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| 글자              | `Delete board`                                                | `Delete board`                                                                                                                        |
| 글자 아포스트로피 | `You're editing a template.`                                  | `You're editing a template.`                                                                                                          |
| 인자              | `No {property}`                                               | `No {property}`                                                                                                                       |
| 태그              | `Boards in <b>{categoryName}</b> will move back`              | `Boards in {#b}{categoryName}{/b} will move back`                                                                                     |
| `''`              | `a '' b`                                                      | `a ' b`                                                                                                                               |
| 아포스트로피 인용 | `'{'x'}'`                                                     | `\{x\}`                                                                                                                               |
| number            | `{n, number}`, `{n, number, integer}`, `{n, number, percent}` | `{n: number}`, `{n: number style=integer}`, `{n: number style=percent}`                                                               |
| plural            | `{count, plural, one {# card} other {# cards}}`               | `local countPlural = count: plural`, 매치 `countPlural=one` → `{count: icu:pound} card`, `countPlural=*` → `{count: icu:pound} cards` |
| plural의 `=0`     | `{count, plural, =0 {none} one {one} other {many}}`           | 선택자 둘: `local countPluralExact = count`가 먼저, 매치 `countPluralExact=0, countPlural=*` → `none`                                 |
| plural의 `offset` | `{n, plural, offset:1 … other {# more}}`                      | `local nPlural = n: plural offset=1`, `#`는 `{n: icu:pound offset=1}`                                                                 |
| selectordinal     | `{place, selectordinal, one {#st} … other {#th}}`             | `local placeOrdinal = place: plural type=ordinal`                                                                                     |
| select            | `{role, select, admin {All} other {None}}`                    | 선택자 `role`, 매치 `role=admin` → `All`, `role=*` → `None`                                                                           |
| 중첩·나란한 선택  | select 절마다 plural                                          | 변형을 곱해 편다: `g=own, nPlural=one`, `g=own, nPlural=*`, `g=*, nPlural=one`, `g=*, nPlural=*`                                      |

- `other`는 `*`로 옮긴다. ICU에서 `other`는 "다른 절이 가져가지 않은 모든 값"이고 그것이 `*`의 뜻이다. 플러그인 README의 예처럼 `countPlural=other`라고 쓰면 그 로케일에 없는 범주에서 메시지 id가 출력된다.
- 컴파일된 메시지는 변형을 "선택자 순서대로, 글자 키가 `*`보다 먼저"의 순으로 시도해 처음 맞는 것을 돌려준다(`compile-message.js`의 `sortVariantsBySelectorPreference`). 변환기는 곱해 편 변형마다 ICU가 그 변형을 고르는 조건을 함께 들고 있다가, 키의 모든 조합에 대해 "처음 맞는 변형 = ICU가 고르는 변형"인지 전부 대조한다(`inlang.ts`의 `verify`). 하나라도 어긋나면 변환하지 않는다.
- **변환하지 않는 것.** 근사치로 옮기지 않고 "변환하지 않은 메시지"에 이유와 함께 넣는다.
  - `date`, `time` 형식. 원본의 이름 붙은 스타일(`short` 등)은 필드 목록이고 paraglide의 것은 `Intl`의 `dateStyle`·`timeStyle`이라 로케일에 따라 출력이 다를 수 있다.
  - `number`의 `integer`, `percent`가 아닌 스타일(스켈레톤 포함).
  - 평범한 식별자(`[A-Za-z_][A-Za-z0-9_]*`)가 아닌 인자 이름, 글자와 숫자가 아닌 태그 이름.
  - 한 인자가 한 경로에서 두 번 선택하는 메시지(나란히 또는 자기 절 안에서).
  - 원본 파서가 구문 오류로 보는 문구.
  - 영어 메시지에 없는 인자나 태그를 쓰는 번역. paraglide는 로케일들의 인자를 합쳐 입력 타입을 만들기 때문에, 번역에만 있는 인자는 모든 호출에 그 인자를 요구하게 만든다.
  - 바깥 사이트 주소가 든 문구.
- 변환하지 않은 메시지는 그 로케일에서 빠지고, 컴파일된 메시지는 영어 문구를 보여 준다.

`@inlang/plugin-icu1`이라는 플러그인이 ICU 파일을 직접 읽는다는 것이 README에 언급돼 있다. 설치돼 있지 않고 새 의존성을 넣지 않기로 했으므로 쓰지 않았다. 제거·이름 치환·채움도 그것만으로는 되지 않는다.

## 로케일별 통계

이 표는 변환 도구가 썼다.

| 로케일    | 원본 파일      | 원본 메시지 | 변환 | 그 가운데 덮어씀 | 변환 안 함 | 영어가 바뀌어 내지 않음 | 제거: 미포팅 | 제거: 죽은 id | 채움 | 남긴 id 가운데 빠짐 |
| --------- | -------------- | ----------- | ---- | ---------------- | ---------- | ----------------------- | ------------ | ------------- | ---- | ------------------- |
| `en`      | `en.json`      | 461         | 376  | 6                | 0          | 0                       | 34           | 51            | 0    | 0                   |
| `ko`      | `ko.json`      | 437         | 358  | 130              | 0          | 0                       | 33           | 46            | 18   | 0                   |
| `ca`      | `ca.json`      | 195         | 191  | 1                | 0          | 0                       | 1            | 3             | 0    | 185                 |
| `de`      | `de.json`      | 460         | 373  | 2                | 0          | 2                       | 34           | 51            | 0    | 3                   |
| `el`      | `el.json`      | 47          | 47   | 0                | 0          | 0                       | 0            | 0             | 0    | 329                 |
| `es`      | `es.json`      | 226         | 213  | 1                | 0          | 0                       | 6            | 7             | 0    | 163                 |
| `fr`      | `fr.json`      | 413         | 334  | 1                | 0          | 2                       | 33           | 44            | 0    | 42                  |
| `id`      | `id.json`      | 149         | 142  | 1                | 2          | 0                       | 5            | 0             | 0    | 234                 |
| `it`      | `it.json`      | 309         | 291  | 1                | 1          | 1                       | 10           | 6             | 0    | 85                  |
| `ja`      | `ja.json`      | 449         | 363  | 3                | 0          | 2                       | 33           | 51            | 0    | 13                  |
| `nl`      | `nl.json`      | 449         | 363  | 5                | 0          | 2                       | 33           | 51            | 0    | 13                  |
| `oc`      | `oc.json`      | 148         | 142  | 0                | 0          | 0                       | 4            | 2             | 0    | 234                 |
| `pt-BR`   | `pt_BR.json`   | 460         | 373  | 4                | 0          | 2                       | 34           | 51            | 0    | 3                   |
| `ru`      | `ru.json`      | 428         | 358  | 0                | 0          | 2                       | 26           | 42            | 0    | 18                  |
| `sv`      | `sv.json`      | 449         | 363  | 4                | 0          | 2                       | 33           | 51            | 0    | 13                  |
| `tr`      | `tr.json`      | 460         | 373  | 0                | 0          | 2                       | 34           | 51            | 0    | 3                   |
| `zh-Hans` | `zh_Hans.json` | 449         | 363  | 1                | 0          | 2                       | 33           | 51            | 0    | 13                  |
| `zh-Hant` | `zh_Hant.json` | 449         | 363  | 0                | 0          | 2                       | 33           | 51            | 0    | 13                  |

- "원본 메시지"는 원본 파일의 키 수다. "변환 + 변환 안 함 + 영어가 바뀌어 내지 않음 + 제거: 미포팅 + 제거: 죽은 id"가 그 수와 같다.
- "그 가운데 덮어씀"은 "변환"에 들어 있는 수다. 원본의 문구 대신 `messages-override`의 문구가 나간 id다.
- "영어가 바뀌어 내지 않음"은 영어 문구를 다시 써서 옛 번역을 내지 않은 id다(`ShareBoard.searchPlaceholder`, `OnboardingTour.CopyLink.Body` 가운데 그 로케일의 원본에 있는 것).
- "남긴 id 가운데 빠짐"은 남긴 원본 id 376개 가운데 그 로케일의 결과에 없는 수다. 빠진 메시지는 영어로 나온다.
- 이 표는 자체 키(`messages-own`)를 세지 않는다. 자체 키는 `en`과 `ko`에만 있다. `messages/<태그>.json`의 키 수는 "변환 + 채움"에, `en`과 `ko`에서는 자체 키의 수를 더한 것이다. 이 문서를 쓸 때 자체 키는 8개여서 `en`과 `ko`가 384개였다. `i18n:keys`가 말하는 "missing"은 다른 로케일에서 "남긴 id 가운데 빠짐"에 자체 키의 수를 더한 것이다.
- 원본 파일에 `en.json`에 없는 키가 있는 로케일은 없다.

## 변환하지 않은 메시지

| 로케일 | id                                                    | 이유 (도구의 출력)                                       |
| ------ | ----------------------------------------------------- | -------------------------------------------------------- |
| `id`   | `BoardTemplateSelector.plugin.no-content-description` | not in the English message: {lineBreak}, {workspaceName} |
| `id`   | `BoardTemplateSelector.plugin.no-content-title`       | not in the English message: {workspaceName}              |
| `it`   | `BoardTemplateSelector.plugin.no-content-description` | not in the English message: {lineBreak}, {teamName}      |

셋 다 옛 영어 문구의 번역이 남은 것이다. 지금의 영어 문구(`Create a board`, `Add a board to the sidebar using any of the templates defined below or start from scratch.`)에는 인자가 없다. 원본에서도 이 호출은 값을 넘기지 않으므로 react-intl이 형식화에 실패하고 코드의 영어 기본 문구를 보여 줬을 것이다(실행해 확인하지는 않았다). 결과는 같다: 이 세 메시지는 `id`, `it`에서 영어로 나온다.

## 제거한 메시지

새 앱이 포팅하지 않는 기능에서만 쓰이는 메시지 34개다. 처음 변환이 28개를 뺐고, 판단을 미뤘던 것 가운데 6개를 결정에 따라 더 뺐다(마지막 소절). 목록과 이유는 아래 표에 있다. id마다 `webapp/src`(테스트와 스냅숏 제외)에서 문자열 리터럴로 찾아 쓰이는 파일을 확인했다. "쓰던 파일" 열은 변환 도구가 한 같은 검색의 출력이다.

### 클라우드 한도와 업그레이드 안내 (13개)

| id                                                   | 영어 문구                                                                  | 쓰던 파일 (`webapp/src/` 아래)                   |
| ---------------------------------------------------- | -------------------------------------------------------------------------- | ------------------------------------------------ |
| `CardDetail.limited-body`                            | Upgrade to our Professional or Enterprise plan.                            | `components/cardDetail/cardDetail.tsx`           |
| `CardDetail.limited-button`                          | Upgrade                                                                    | `components/cardDetail/cardDetail.tsx`           |
| `CardDetail.limited-title`                           | This card is hidden                                                        | `components/cardDetail/cardDetail.tsx`           |
| `CardDetial.limited-link`                            | Learn more about our plans.                                                | `components/cardDetail/cardDetail.tsx`           |
| `ViewLimitDialog.notifyAdmin.Success`                | Your admin has been notified                                               | `components/cardLimitNotification.tsx`           |
| `limitedCard.title`                                  | Cards hidden                                                               | `components/hiddenCardCount/hiddenCardCount.tsx` |
| `notification-box-card-limit-reached.close-tooltip`  | Snooze for 10 days                                                         | `components/cardLimitNotification.tsx`           |
| `notification-box-card-limit-reached.contact-link`   | notify your admin                                                          | `components/cardLimitNotification.tsx`           |
| `notification-box-card-limit-reached.link`           | Upgrade to a paid plan                                                     | `components/cardLimitNotification.tsx`           |
| `notification-box-card-limit-reached.title`          | {cards} cards hidden from board                                            | `components/cardLimitNotification.tsx`           |
| `notification-box-cards-hidden.title`                | This action has hidden another card                                        | `components/cardLimitNotification.tsx`           |
| `notification-box.card-limit-reached.not-admin.text` | To access archived cards, you can {contactLink} to upgrade to a paid plan. | `components/cardLimitNotification.tsx`           |
| `notification-box.card-limit-reached.text`           | Card limit reached, to view older cards, {link}                            | `components/cardLimitNotification.tsx`           |

### 보드를 채팅 채널에 연결하는 UI (5개)

`shareBoard.tsx`의 채널 연결 확인 대화상자다. 같은 파일의 다른 메시지는 남겼다.

| id                                                           | 영어 문구                                                                                                                                                                                                                                                        | 쓰던 파일 (`webapp/src/` 아래)         |
| ------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------- |
| `shareBoard.confirm-link-channel`                            | Link board to channel                                                                                                                                                                                                                                            | `components/shareBoard/shareBoard.tsx` |
| `shareBoard.confirm-link-channel-button`                     | Link channel                                                                                                                                                                                                                                                     | `components/shareBoard/shareBoard.tsx` |
| `shareBoard.confirm-link-channel-button-with-other-channel`  | Unlink and link here                                                                                                                                                                                                                                             | `components/shareBoard/shareBoard.tsx` |
| `shareBoard.confirm-link-channel-subtext`                    | When you link a channel to a board, all members of the channel (existing and new) will be able to edit it. This excludes members who are guests.                                                                                                                 | `components/shareBoard/shareBoard.tsx` |
| `shareBoard.confirm-link-channel-subtext-with-other-channel` | When you link a channel to a board, all members of the channel (existing and new) will be able to edit it. This excludes members who are guests.{lineBreak}This board is currently linked to another channel. It will be unlinked if you choose to link it here. | `components/shareBoard/shareBoard.tsx` |

### 라우트되지 않는 welcomePage (5개)

`pages/welcome/welcomePage.tsx`는 `webapp/src`의 어디에서도 `import`하지 않는다.

| id                              | 영어 문구                                                                                                                                | 쓰던 파일 (`webapp/src/` 아래)  |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| `WelcomePage.Description`       | Boards is a project management tool that helps define, organize, track, and manage work across teams using a familiar Kanban board view. | `pages/welcome/welcomePage.tsx` |
| `WelcomePage.Explore.Button`    | Take a tour                                                                                                                              | `pages/welcome/welcomePage.tsx` |
| `WelcomePage.Heading`           | Welcome To Boards                                                                                                                        | `pages/welcome/welcomePage.tsx` |
| `WelcomePage.NoThanks.Text`     | No thanks, I'll figure it out myself                                                                                                     | `pages/welcome/welcomePage.tsx` |
| `WelcomePage.StartUsingIt.Text` | Start using it                                                                                                                           | `pages/welcome/welcomePage.tsx` |

### 바깥 사이트로만 이어지는 메뉴와 문구 (5개)

| id                                   | 영어 문구                             | 쓰던 파일 (`webapp/src/` 아래)                                      |
| ------------------------------------ | ------------------------------------- | ------------------------------------------------------------------- |
| `Sidebar.about`                      | About (옛 제품 이름)                  | `components/sidebar/sidebarUserMenu.tsx`                            |
| `SidebarTour.SidebarCategories.Link` | Learn more                            | `components/onboardingTour/sidebarCategories/sidebarCategories.tsx` |
| `TopBar.give-feedback`               | Give feedback                         | `components/topBar.tsx`                                             |
| `VersionMessage.help`                | Check out what's new in this version. | `components/messages/versionMessage.tsx`                            |
| `VersionMessage.learn-more`          | Learn more                            | `components/messages/versionMessage.tsx`                            |

- `Sidebar.about`: 누르면 옛 제품의 사이트를 새 창으로 연다. 옛 제품 이름이 든 유일한 메시지였다.
- `TopBar.give-feedback`: 옛 제품의 피드백 페이지로 가는 링크의 글자다. 같은 막대의 도움말 아이콘(사용자 안내서 링크)에는 메시지가 없다.
- `VersionMessage.*`: 새 판 안내 띠다. 내용이 "새 기능" 페이지로 가는 링크뿐이다.
- `SidebarTour.SidebarCategories.Link`: 둘러보기의 한 단계에 붙은 "Learn more" 링크로, 옛 회사의 문서 사이트로 간다. 그 단계의 제목과 본문은 처음에는 남겼다가 아래 결정으로 뺐다.

과제의 미포팅 목록 가운데 나머지에는 해당 메시지가 없다. `channels` 스토어 조각, `initPlugin`, 텔레메트리, `nativeApp.ts`는 메시지를 쓰지 않는다. 통계(`SiteStats.*`)와 채널 쪽 화면(`rhs-*`, `boardSelector.*` 등)의 메시지는 `webapp/src`에 쓰는 곳이 없어 아래 "죽은 id"에 들어갔다. 가져오기 도움말 페이지 링크의 글자는 메시지가 아니라 코드에 박힌 이름이다.

### 결정으로 뺀 것 (6개)

처음 변환이 판단을 미루고 남겼던 메시지다. 제품 책임자의 결정(2026-11-10)으로 뺐다. 묶음으로는 `guest-accounts`(3개), `old-notice`(1개), `upgrade-tour-step`(2개)이다.

| id                                    | 영어 문구                                                                                                                                                                                                                                                                              | 쓰던 파일 (`webapp/src/` 아래)                                      |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `badge.guest`                         | Guest                                                                                                                                                                                                                                                                                  | `widgets/guestBadge.tsx`                                            |
| `guest-no-board.subtitle`             | You don't have access to any board in this team yet, please wait until somebody adds you to any board.                                                                                                                                                                                 | `components/guestNoBoards.tsx`                                      |
| `guest-no-board.title`                | No boards yet                                                                                                                                                                                                                                                                          | `components/guestNoBoards.tsx`                                      |
| `Error.mobileweb`                     | Mobile web support is currently in early beta. Not all functionality may be present.                                                                                                                                                                                                   | `pages/boardPage/boardPage.tsx`                                     |
| `SidebarTour.SidebarCategories.Body`  | All your boards are now organized under your new sidebar. No more switching between workspaces. One-time custom categories based on your prior workspaces may have automatically been created for you as part of your v7.2 upgrade. These can be removed or edited to your preference. | `components/onboardingTour/sidebarCategories/sidebarCategories.tsx` |
| `SidebarTour.SidebarCategories.Title` | Sidebar categories                                                                                                                                                                                                                                                                     | `components/onboardingTour/sidebarCategories/sidebarCategories.tsx` |

- `badge.guest`, `guest-no-board.*`: 게스트 계정의 배지와, 게스트가 볼 보드가 없을 때의 화면이다. 서버 설계 문서에는 게스트라는 말이 없다(저장소 루트의 `docs/s6-design.md`를 찾아봤다).
- `Error.mobileweb`: 모바일 웹 지원이 초기 베타라는 옛 제품의 안내다.
- `SidebarTour.SidebarCategories.*`: 둘러보기의 한 단계로, 옛 제품의 v7.2 업그레이드가 사이드바와 워크스페이스에 한 일을 설명한다.

## 죽은 id

`en.json`에 있지만 `webapp/src`의 테스트가 아닌 `.ts`/`.tsx` 어디에도 문자열 리터럴로 나오지 않는 id 51개다. 원본이 id를 실행 중에 조립하는 곳은 `Sidebar.${theme.id}` 하나이고, 거기서 만들어지는 id는 어느 번역 파일에도 없다. 대부분 이 저장소에 없는 플러그인 쪽 코드가 쓰던 것으로 보인다.

| id                                                            | 영어 문구                                                                                                                                                                                                                                                                 |
| ------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `AppBar.Tooltip`                                              | Toggle Linked Boards                                                                                                                                                                                                                                                      |
| `BoardMember.unlinkChannel`                                   | Unlink                                                                                                                                                                                                                                                                    |
| `BoardsUnfurl.Limited`                                        | Additional details are hidden due to the card being archived                                                                                                                                                                                                              |
| `BoardsUnfurl.Remainder`                                      | +{remainder} more                                                                                                                                                                                                                                                         |
| `BoardsUnfurl.Updated`                                        | Updated {time}                                                                                                                                                                                                                                                            |
| `CardDetail.Follow`                                           | Follow                                                                                                                                                                                                                                                                    |
| `CardDetail.Following`                                        | Following                                                                                                                                                                                                                                                                 |
| `ChannelIntro.CreateBoard`                                    | Create a board                                                                                                                                                                                                                                                            |
| `SiteStats.total_boards`                                      | Total boards                                                                                                                                                                                                                                                              |
| `SiteStats.total_cards`                                       | Total cards                                                                                                                                                                                                                                                               |
| `ViewLimitDialog.Heading`                                     | Views per board limit reached                                                                                                                                                                                                                                             |
| `ViewLimitDialog.PrimaryButton.Title.Admin`                   | Upgrade                                                                                                                                                                                                                                                                   |
| `ViewLimitDialog.PrimaryButton.Title.RegularUser`             | Notify Admin                                                                                                                                                                                                                                                              |
| `ViewLimitDialog.Subtext.Admin`                               | Upgrade to our Professional or Enterprise plan.                                                                                                                                                                                                                           |
| `ViewLimitDialog.Subtext.Admin.PricingPageLink`               | Learn more about our plans.                                                                                                                                                                                                                                               |
| `ViewLimitDialog.Subtext.RegularUser`                         | Notify your Admin to upgrade to our Professional or Enterprise plan.                                                                                                                                                                                                      |
| `ViewLimitDialog.UpgradeImg.AltText`                          | upgrade image                                                                                                                                                                                                                                                             |
| `boardSelector.confirm-link-board`                            | Link board to channel                                                                                                                                                                                                                                                     |
| `boardSelector.confirm-link-board-button`                     | Yes, link board                                                                                                                                                                                                                                                           |
| `boardSelector.confirm-link-board-subtext`                    | When you link "{boardName}" to the channel, all members of the channel (existing and new) will be able to edit it. This excludes members who are guests. You can unlink a board from a channel at any time.                                                               |
| `boardSelector.confirm-link-board-subtext-with-other-channel` | When you link "{boardName}" to the channel, all members of the channel (existing and new) will be able to edit it. This excludes members who are guests.{lineBreak} This board is currently linked to another channel. It will be unlinked if you choose to link it here. |
| `boardSelector.create-a-board`                                | Create a board                                                                                                                                                                                                                                                            |
| `boardSelector.link`                                          | Link                                                                                                                                                                                                                                                                      |
| `boardSelector.search-for-boards`                             | Search for boards                                                                                                                                                                                                                                                         |
| `boardSelector.title`                                         | Link boards                                                                                                                                                                                                                                                               |
| `boardSelector.unlink`                                        | Unlink                                                                                                                                                                                                                                                                    |
| `cloudMessage.learn-more`                                     | Learn more                                                                                                                                                                                                                                                                |
| `new_channel_modal.create_board.empty_board_description`      | Create a new empty board                                                                                                                                                                                                                                                  |
| `new_channel_modal.create_board.empty_board_title`            | Empty board                                                                                                                                                                                                                                                               |
| `new_channel_modal.create_board.select_template_placeholder`  | Select a template                                                                                                                                                                                                                                                         |
| `new_channel_modal.create_board.title`                        | Create a board for this channel                                                                                                                                                                                                                                           |
| `rhs-board-non-admin-msg`                                     | You're not an admin of the board                                                                                                                                                                                                                                          |
| `rhs-boards.add`                                              | Add                                                                                                                                                                                                                                                                       |
| `rhs-boards.dm`                                               | DM                                                                                                                                                                                                                                                                        |
| `rhs-boards.gm`                                               | GM                                                                                                                                                                                                                                                                        |
| `rhs-boards.header.dm`                                        | this direct message                                                                                                                                                                                                                                                       |
| `rhs-boards.header.gm`                                        | this group message                                                                                                                                                                                                                                                        |
| `rhs-boards.last-update-at`                                   | Last update at: {datetime}                                                                                                                                                                                                                                                |
| `rhs-boards.link-boards-to-channel`                           | Link boards to {channelName}                                                                                                                                                                                                                                              |
| `rhs-boards.linked-boards`                                    | Linked boards                                                                                                                                                                                                                                                             |
| `rhs-boards.no-boards-linked-to-channel`                      | No boards are linked to {channelName} yet                                                                                                                                                                                                                                 |
| `rhs-boards.no-boards-linked-to-channel-description`          | Boards is a project management tool that helps define, organize, track and manage work across teams, using a familiar kanban board view.                                                                                                                                  |
| `rhs-boards.unlink-board`                                     | Unlink board                                                                                                                                                                                                                                                              |
| `rhs-boards.unlink-board1`                                    | Unlink board                                                                                                                                                                                                                                                              |
| `rhs-channel-boards-header.title`                             | Boards                                                                                                                                                                                                                                                                    |
| `shareBoard.channels-select-group`                            | Channels                                                                                                                                                                                                                                                                  |
| `shareBoard.confirm-unlink.body`                              | When you unlink a channel from a board, all members of the channel (existing and new) will lose access to it unless they're given permission separately.                                                                                                                  |
| `shareBoard.confirm-unlink.confirmBtnText`                    | Unlink channel                                                                                                                                                                                                                                                            |
| `shareBoard.confirm-unlink.title`                             | Unlink channel from board                                                                                                                                                                                                                                                 |
| `shareBoard.members-select-group`                             | Members                                                                                                                                                                                                                                                                   |
| `shareBoard.unknown-channel-display-name`                     | Unknown channel                                                                                                                                                                                                                                                           |

## 판단을 미뤘다가 정한 것

처음 변환은 제거 기준에 걸칠 수 있지만 확신이 없는 메시지를 남겨 두고 이 자리에 적었다. 제품 책임자가 다음과 같이 정했다(2026-11-10).

| id                                                                                                                                              | 걸리던 점                                                                                                                                                                                   | 결정                                                                                                                                                                                                            |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `AdminBadge.SystemAdmin`, `AdminBadge.WorkspaceAdmin`                                                                                           | 시스템 관리자·팀 관리자는 옛 회사 제품의 계정 종류다                                                                                                                                        | 남긴다. 팀을 유지하고 인스턴스 관리자가 생긴다                                                                                                                                                                  |
| `ShareBoard.workspacePermissionsText`, `shareBoard.confirm-change-workspace-role.*` 3개, `error.back-to-workspace`, `error.workspace-undefined` | 팀 개념을 전제한다                                                                                                                                                                          | 남긴다. 같은 이유다                                                                                                                                                                                             |
| `badge.guest`, `guest-no-board.title`, `guest-no-board.subtitle`                                                                                | 게스트는 옛 회사 제품의 계정 종류다                                                                                                                                                         | 뺀다(묶음 `guest-accounts`)                                                                                                                                                                                     |
| `Error.mobileweb`                                                                                                                               | "Mobile web support is currently in early beta"라는 옛 제품의 안내다                                                                                                                        | 뺀다(`old-notice`)                                                                                                                                                                                              |
| `SidebarTour.SidebarCategories.Title`, `.Body`                                                                                                  | 둘러보기 단계지만 본문이 옛 제품의 v7.2 업그레이드와 워크스페이스 폐지를 설명한다                                                                                                           | 뺀다(`upgrade-tour-step`). 이 단계의 링크 글자(`.Link`)는 처음 변환 때 이미 뺐다                                                                                                                                |
| `ShareBoard.searchPlaceholder`                                                                                                                  | 문구가 "Search for people and channels"다. 채널 연결은 포팅하지 않는다                                                                                                                      | 사람만 찾는 문구로 다시 쓴다. 영어 "Search for people", 한국어 "사용자 검색". 다른 로케일은 번역을 내지 않는다                                                                                                  |
| `OnboardingTour.CopyLink.Body`                                                                                                                  | "channel, direct message, or group message"에 붙여 넣으라고 한다                                                                                                                            | 채널 언급 없이 다시 쓴다. 영어 "You can share your cards with teammates by copying the link and sending it to them.", 한국어 "링크를 복사해 팀원에게 보내면 카드를 공유할 수 있습니다." 다른 로케일은 위와 같다 |
| `BoardPage.newVersion`, `OnboardingTour.OpenACard.Body`, `error.invalid-read-only-board`, `error.not-logged-in`                                 | 영어 문구가 "Boards"를 제품 이름으로 쓴다                                                                                                                                                   | `fvoci`로 바꾼다. "이름 치환"에 적었다                                                                                                                                                                          |
| `SidebarCategories.CategoryMenu.DeleteModal.Body`                                                                                               | 처음 변환은 이것도 "Boards"를 제품 이름처럼 쓴다고 봤다                                                                                                                                     | 그대로 둔다. 읽어 보니 제품 이름이 아니다. "이름 치환"에 적었다                                                                                                                                                 |
| `Sidebar.product-tour`                                                                                                                          | 쓰는 곳이 `globalHeaderSettingsMenu.tsx`뿐이고, 그 위의 `GlobalHeader`는 `webapp/src` 안에서 `import`되지 않는다(플러그인 진입점용). 둘러보기를 다시 시작하는 메뉴라 둘러보기와 함께 남겼다 | 결정이 없다. 남겨 둔 그대로다                                                                                                                                                                                   |
| `BoardTemplateSelector.plugin.no-content-title`, `.plugin.no-content-description`                                                               | id에 `plugin`이 있다. `workspace.tsx`가 채널 경로일 때뿐 아니라 고른 보드가 없을 때도 쓴다                                                                                                  | 결정이 없다. 남겨 둔 그대로다                                                                                                                                                                                   |

- 다시 쓴 두 메시지의 영어 문구는 내가 쓴 것이다. `ShareBoard.searchPlaceholder`의 새 문구는 템플릿을 공유할 때 같은 자리에 나오던 `ShareTemplate.searchPlaceholder`와 같다.
- "다른 로케일은 번역을 내지 않는다"는 영어 항목의 `"stale": true`로 적었다. 어느 로케일에서 빠졌는지는 아래 표다. `ru`의 `ShareBoard.searchPlaceholder`는 원본 번역이 이미 "사람 검색"이라는 뜻이었지만(`Поиск людей`) 결정대로 함께 뺐다. 각 로케일이 `ShareTemplate.searchPlaceholder`의 번역을 이 id에 다시 쓰는 길도 있다. 하지 않았다.

| 로케일    | 번역을 내지 않은 id                                            |
| --------- | -------------------------------------------------------------- |
| `de`      | `OnboardingTour.CopyLink.Body`, `ShareBoard.searchPlaceholder` |
| `fr`      | `OnboardingTour.CopyLink.Body`, `ShareBoard.searchPlaceholder` |
| `it`      | `OnboardingTour.CopyLink.Body`                                 |
| `ja`      | `OnboardingTour.CopyLink.Body`, `ShareBoard.searchPlaceholder` |
| `nl`      | `OnboardingTour.CopyLink.Body`, `ShareBoard.searchPlaceholder` |
| `pt-BR`   | `OnboardingTour.CopyLink.Body`, `ShareBoard.searchPlaceholder` |
| `ru`      | `OnboardingTour.CopyLink.Body`, `ShareBoard.searchPlaceholder` |
| `sv`      | `OnboardingTour.CopyLink.Body`, `ShareBoard.searchPlaceholder` |
| `tr`      | `OnboardingTour.CopyLink.Body`, `ShareBoard.searchPlaceholder` |
| `zh-Hans` | `OnboardingTour.CopyLink.Body`, `ShareBoard.searchPlaceholder` |
| `zh-Hant` | `OnboardingTour.CopyLink.Body`, `ShareBoard.searchPlaceholder` |

## 이름 치환

- **옛 제품 이름.** 18개 파일에서 `Sidebar.about`에만 있었고(`el`을 뺀 17개 로케일), 그 메시지는 제거했다. 남은 메시지에는 없다. 변환 도구는 남는 문구에 이 이름이 있으면 대소문자와 상관없이 `fvoci`로 바꿨다. 지금은 `tests/tools/i18n.test.ts`가 메시지 파일에 이 이름이 없는지 본다.
- **옛 회사 이름.** `OnboardingTour.AddComments.Body` 하나에 있다(13개 로케일). 댓글에서 다른 사용자를 @멘션하라는 문구에서 "이 제품의 사용자"를 가리키므로 `fvoci`로 바꿨다. 한국어는 한글로 음역해 적혀 있어서 그 표기도 함께 바꾼다. 치환은 기계적이다. `nl`은 이름이 합성어 안에 들어가 `medefvocigebruikers`가 됐었다. 결정 2에 따라 `messages-override/nl.json`에서 `andere fvoci-gebruikers`("다른 fvoci 사용자들". `sv`의 `andra fvoci-användare`와 같은 꼴)로 고쳤다. 한국어는 빠진 말이 있어 문장을 다시 옮겼다(`docs/ko-review.md`).
- 변환 도구는 이 메시지가 아닌 곳에서 옛 회사 이름을 만나면 멈췄다. 지금은 같은 테스트가 메시지 파일에 그 이름(한글 표기 포함)이 없는지 본다.
- 라틴 문자와 한글 말고 다른 문자로 음역한 표기(키릴, 가나 등)가 있는지 몇 가지 철자로 찾아봤고 없었다. 모든 철자를 다 찾았다고는 할 수 없다.

`OnboardingTour.AddComments.Body`의 치환 결과다. `ko`와 `nl`은 치환한 문구를 다시 덮어썼으므로 화면에 나오는 것은 아래 "덮어쓴 문구" 표와 `docs/ko-review.md`의 문구다.

| 로케일    | 치환한 뒤의 문구                                                                                                      |
| --------- | --------------------------------------------------------------------------------------------------------------------- |
| `en`      | You can comment on issues, and even @mention your fellow fvoci users to get their attention.                          |
| `ko`      | @mention 을 이용하여 이슈에 대해 fvoci 사용자에게 알릴 수 있습니다.                                                   |
| `de`      | Du kannst Themen kommentieren und sogar deine fvoci-Kollegen @erwähnen, um deren Aufmerksamkeit zu erhalten.          |
| `fr`      | Vous pouvez commenter les bugs et même @mentionner les utilisateurs de fvoci pour attirer leur attention.             |
| `it`      | Puoi commentare sui issues e anche @menzionare il tuo compagno su fvoci per attirare la loro attenzione.              |
| `ja`      | 問題にコメントしたり、仲間のfvociユーザーの注意を引くために@メンションすることもできます。                            |
| `nl`      | Je kunt commentaar geven op onderwerpen, en zelfs je medefvocigebruikers @vermelden om hun aandacht te trekken.       |
| `pt-BR`   | Você pode comentar questões, e até mesmo @mencionar seus usuários companheiros de fvoci para conseguir suas atenções. |
| `ru`      | Вы можете комментировать проблемы и даже @упоминать своих коллег-пользователей fvoci, чтобы привлечь их внимание.     |
| `sv`      | Du kan kommentera ämnen och till och med @omnämna andra fvoci-användare för att få deras uppmärksamhet.               |
| `tr`      | Sorunlar hakkında yorum yapabilir ve fvoci kullanıcılarının dikkatini çekmek için @anabilirsiniz.                     |
| `zh-Hans` | 你可以对问题进行评论，甚至可以@提及你的fvoci同伴，以引起他们的注意。                                                  |
| `zh-Hant` | 你可以對問題進行評論,甚至標記提到你的fvoci夥伴,引起他們的注意。                                                       |

### 제품 이름으로 쓰인 Boards

원본의 영어 문구 몇 곳은 제품을 "Boards"라고 부른다(제거한 `WelcomePage.Heading`의 "Welcome To Boards"도 그렇다). 금지된 두 이름이 아니어서 처음 변환은 손대지 않았다. 결정 1: 제품을 가리키는 Boards는 `fvoci`로 바꾸고, 사이드바의 기본 카테고리 이름인 Boards는 그대로 둔다. 기계적 치환이 아니라 메시지마다 읽고 `messages-override`에 적었다.

남긴 영어 문구에서 대문자로 시작하는 "Boards"가 나오는 메시지는 7개였다. 4개를 바꿨다.

| id                                                | 영어 문구 (고치기 전)                                                                                          | 판단                                                                                                                                                                                                                                    |
| ------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `BoardPage.newVersion`                            | A new version of Boards is available, click here to reload.                                                    | 제품. 바꿨다                                                                                                                                                                                                                            |
| `OnboardingTour.OpenACard.Body`                   | Open a card to explore the powerful ways that Boards can help you organize your work.                          | 제품. 바꿨다                                                                                                                                                                                                                            |
| `error.invalid-read-only-board`                   | You don't have access to this board. Log in to access Boards.                                                  | 제품. 바꿨다                                                                                                                                                                                                                            |
| `error.not-logged-in`                             | Your session may have expired or you're not logged in. Log in again to access Boards.                          | 제품. 바꿨다                                                                                                                                                                                                                            |
| `SidebarCategories.CategoryMenu.DeleteModal.Body` | Boards in \<b>{categoryName}\</b> will move back to the Boards categories. You're not removed from any boards. | 그대로. 앞의 Boards는 문장 첫머리의 보통명사("그 카테고리의 보드들")이고, 뒤의 "the Boards categories"는 기본 카테고리의 이름이다. 그 이름은 서버가 정한 글자 "Boards"이고(원본 `server/app/category_boards.go`, 서버 트랙도 같은 이름) |
| `FindBoardsDialog.Title`                          | Find Boards                                                                                                    | 그대로. 제목의 대문자 표기일 뿐 보통명사다(`BoardsSwitcher.Title`은 "Find boards")                                                                                                                                                      |
| `shareBoard.lastAdmin`                            | Boards must have at least one Administrator                                                                    | 그대로. 문장 첫머리의 보통명사다                                                                                                                                                                                                        |

사용자가 센 "5건"과의 차이: 처음 변환이 든 후보는 5개였고 그 가운데 `SidebarCategories.CategoryMenu.DeleteModal.Body`는 읽어 보니 제품 이름이 아니었다. 바꾼 것은 4건이다. 후보 밖에서 더 찾은 것은 없다.

다른 로케일에서는 그 네 메시지의 번역에 제품 이름이 라틴 문자로 남아 있는 곳만 `fvoci`로 바꿨다. 아래 "덮어쓴 문구" 표의 `name` 행들이다(23개. `nl`의 `OnboardingTour.AddComments.Body`를 빼고).

- 글자 그대로 "Boards"인 곳이 21개다.
- `id`와 `it`의 `BoardPage.newVersion`은 "Boards"가 아니라 단수 "Board"로 적혀 있다. 두 번역은 다른 메시지에서 보드를 `papan`·`bacheca`나 소문자 `board`로 적는데 여기서는 문장 가운데에 대문자로 썼다. 그래서 제품 이름으로 보고 바꿨다. 결정의 글자("Boards")에서 벗어난 판단이다.
- `nl`과 `pt-BR`의 `OnboardingTour.OpenACard.Body`는 이름을 복수로 받고 있어서 동사와 관사를 단수로 함께 고쳤다(`Boards je kunnen helpen` → `fvoci je kan helpen`, `os Boards podem ajudar` → `o fvoci pode ajudar`).

제품 이름을 그 언어의 보통명사로 옮겼거나 아예 적지 않은 번역은 그대로 뒀다. 그 언어에서는 "보드의 새 버전", "보드에 접근하려면"처럼 읽힌다. 한국어도 이 경우였고 손으로 고쳤다(`docs/ko-review.md`).

| id                              | 그대로 둔 로케일과 그 낱말                                                                                                                                           |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `BoardPage.newVersion`          | `ru`(Доски), `tr`(pano), `zh-Hant`(版面)                                                                                                                             |
| `OnboardingTour.OpenACard.Body` | `fr`(les tableaux), `it`(una Bacheca), `ja`(Board. 이 번역은 보통명사 보드도 "このBoard"처럼 적는다), `ru`(Доски), `tr`(Panoların), `zh-Hans`(板块), `zh-Hant`(看板) |
| `error.invalid-read-only-board` | `de`(das Board), `fr`(이름 없이 "y accéder"), `ru`(к Доскам), `tr`(Panolara), `zh-Hans`(板块), `zh-Hant`(이름 없음)                                                  |
| `error.not-logged-in`           | `de`(das Board), `fr`(aux tableaux d'administration), `ru`(к Доскам), `tr`(Panolara), `zh-Hans`(板块), `zh-Hant`(看板)                                               |

### 덮어쓴 문구 (한국어 밖)

`en`과 다른 로케일에서 덮어쓴 문구 전부다. 한국어 130개는 `docs/ko-review.md`에 있다.

| 로케일    | id                                | 구분                       | 이전                                                                                                                       | 이후                                                                                                                      |
| --------- | --------------------------------- | -------------------------- | -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `en`      | `BoardPage.newVersion`            | 제품 이름                  | A new version of Boards is available, click here to reload.                                                                | A new version of fvoci is available, click here to reload.                                                                |
| `en`      | `OnboardingTour.CopyLink.Body`    | 다시 씀 (번역을 내지 않음) | You can share your cards with teammates by copying the link and pasting it in a channel, direct message, or group message. | You can share your cards with teammates by copying the link and sending it to them.                                       |
| `en`      | `OnboardingTour.OpenACard.Body`   | 제품 이름                  | Open a card to explore the powerful ways that Boards can help you organize your work.                                      | Open a card to explore the powerful ways that fvoci can help you organize your work.                                      |
| `en`      | `ShareBoard.searchPlaceholder`    | 다시 씀 (번역을 내지 않음) | Search for people and channels                                                                                             | Search for people                                                                                                         |
| `en`      | `error.invalid-read-only-board`   | 제품 이름                  | You don't have access to this board. Log in to access Boards.                                                              | You don't have access to this board. Log in to access fvoci.                                                              |
| `en`      | `error.not-logged-in`             | 제품 이름                  | Your session may have expired or you're not logged in. Log in again to access Boards.                                      | Your session may have expired or you're not logged in. Log in again to access fvoci.                                      |
| `ca`      | `BoardPage.newVersion`            | 제품 이름                  | Una nova versió de Boards és disponible, clica aquí per recarregar.                                                        | Una nova versió de fvoci és disponible, clica aquí per recarregar.                                                        |
| `de`      | `BoardPage.newVersion`            | 제품 이름                  | Eine neue Version von Boards ist verfügbar, klicke hier, um neu zu laden.                                                  | Eine neue Version von fvoci ist verfügbar, klicke hier, um neu zu laden.                                                  |
| `de`      | `OnboardingTour.OpenACard.Body`   | 제품 이름                  | Öffne eine Karte und entdecke die Möglichkeiten, die Boards bei der Organisation deiner Arbeit bietet.                     | Öffne eine Karte und entdecke die Möglichkeiten, die fvoci bei der Organisation deiner Arbeit bietet.                     |
| `es`      | `BoardPage.newVersion`            | 제품 이름                  | Una nueva versión de Boards está disponible, haz clic aquí para recargar.                                                  | Una nueva versión de fvoci está disponible, haz clic aquí para recargar.                                                  |
| `fr`      | `BoardPage.newVersion`            | 제품 이름                  | Une nouvelle version de Boards est disponible, cliquez ici pour recharger.                                                 | Une nouvelle version de fvoci est disponible, cliquez ici pour recharger.                                                 |
| `id`      | `BoardPage.newVersion`            | 제품 이름                  | Versi baru Board tersedia, klik di sini untuk memuat ulang.                                                                | Versi baru fvoci tersedia, klik di sini untuk memuat ulang.                                                               |
| `it`      | `BoardPage.newVersion`            | 제품 이름                  | Una nuova versione di Board è disponibile, clicca qui per ricaricare.                                                      | Una nuova versione di fvoci è disponibile, clicca qui per ricaricare.                                                     |
| `ja`      | `BoardPage.newVersion`            | 제품 이름                  | Boardsの新しいバージョンが利用可能です。ここをクリックして再読み込みしてください。                                         | fvociの新しいバージョンが利用可能です。ここをクリックして再読み込みしてください。                                         |
| `ja`      | `error.invalid-read-only-board`   | 제품 이름                  | このBoardにアクセスできません。アクセスするにはBoardsにログインしてください。                                              | このBoardにアクセスできません。アクセスするにはfvociにログインしてください。                                              |
| `ja`      | `error.not-logged-in`             | 제품 이름                  | セッションの有効期限が切れているか、ログインしていない可能性があります。Boardsにアクセスするには再度ログインしてください。 | セッションの有効期限が切れているか、ログインしていない可能性があります。fvociにアクセスするには再度ログインしてください。 |
| `nl`      | `BoardPage.newVersion`            | 제품 이름                  | Er is een nieuwe versie van Boards, klik hier om te herladen.                                                              | Er is een nieuwe versie van fvoci, klik hier om te herladen.                                                              |
| `nl`      | `OnboardingTour.AddComments.Body` | 제품 이름                  | Je kunt commentaar geven op onderwerpen, en zelfs je medefvocigebruikers @vermelden om hun aandacht te trekken.            | Je kunt commentaar geven op onderwerpen, en zelfs andere fvoci-gebruikers @vermelden om hun aandacht te trekken.          |
| `nl`      | `OnboardingTour.OpenACard.Body`   | 제품 이름                  | Open een kaart om de krachtige manieren te ontdekken waarop Boards je kunnen helpen je werk te organiseren.                | Open een kaart om de krachtige manieren te ontdekken waarop fvoci je kan helpen je werk te organiseren.                   |
| `nl`      | `error.invalid-read-only-board`   | 제품 이름                  | Je hebt geen toegang tot dit board. Meld je aan om toegang te krijgen tot Boards.                                          | Je hebt geen toegang tot dit board. Meld je aan om toegang te krijgen tot fvoci.                                          |
| `nl`      | `error.not-logged-in`             | 제품 이름                  | Jouw sessie is misschien verlopen of je bent niet ingelogd. Meldt je opnieuw aan om toegang te krijgen tot Boards.         | Jouw sessie is misschien verlopen of je bent niet ingelogd. Meldt je opnieuw aan om toegang te krijgen tot fvoci.         |
| `pt-BR`   | `BoardPage.newVersion`            | 제품 이름                  | Uma nova versão do Boards está disponível, clique aqui para recarregar.                                                    | Uma nova versão do fvoci está disponível, clique aqui para recarregar.                                                    |
| `pt-BR`   | `OnboardingTour.OpenACard.Body`   | 제품 이름                  | Abra um cartão para explorar formas poderosas que os Boards podem ajudar a organizar seu trabalho.                         | Abra um cartão para explorar formas poderosas que o fvoci pode ajudar a organizar seu trabalho.                           |
| `pt-BR`   | `error.invalid-read-only-board`   | 제품 이름                  | Você não possui acesso a este board. Faça log in para acessar Boards.                                                      | Você não possui acesso a este board. Faça log in para acessar fvoci.                                                      |
| `pt-BR`   | `error.not-logged-in`             | 제품 이름                  | Sua sessão pode ter expirado ou você não está logado. Faça Log in para obter acesso ao Boards.                             | Sua sessão pode ter expirado ou você não está logado. Faça Log in para obter acesso ao fvoci.                             |
| `sv`      | `BoardPage.newVersion`            | 제품 이름                  | En ny version av Boards finns tillgänglig. Klicka här för att uppdatera.                                                   | En ny version av fvoci finns tillgänglig. Klicka här för att uppdatera.                                                   |
| `sv`      | `OnboardingTour.OpenACard.Body`   | 제품 이름                  | Öppna ett kort för att utforska produktiva sätt som Boards kan hjälpa dig att organisera ditt arbete.                      | Öppna ett kort för att utforska produktiva sätt som fvoci kan hjälpa dig att organisera ditt arbete.                      |
| `sv`      | `error.invalid-read-only-board`   | 제품 이름                  | Du har inte tillgång till denna board. Logga in för att få tillgång till Boards.                                           | Du har inte tillgång till denna board. Logga in för att få tillgång till fvoci.                                           |
| `sv`      | `error.not-logged-in`             | 제품 이름                  | Din session kan ha löpt ut eller så är du inte inloggad. Logga in igen för att få tillgång till Boards.                    | Din session kan ha löpt ut eller så är du inte inloggad. Logga in igen för att få tillgång till fvoci.                    |
| `zh-Hans` | `BoardPage.newVersion`            | 제품 이름                  | Boards 的新版本已可用，点击这里重新加载。                                                                                  | fvoci 的新版本已可用，点击这里重新加载。                                                                                  |

`en`과 `ko`가 아닌 로케일의 문구는 내가 고친 것이고 그 언어를 쓰는 사람이 보지 않았다.

## 바깥 URL이 든 메시지

없다. 18개 원본 파일의 모든 문구에서 `http://`, `https://`, `www.`, `이름.com|org|net|io|html` 꼴을 찾았고 한 건도 없었다. 원본의 바깥 링크는 문구가 아니라 코드에 있다(`sidebarUserMenu.tsx`, `topBar.tsx`, `messages/versionMessage.tsx`, `onboardingTour/sidebarCategories/sidebarCategories.tsx`, `globalHeader/globalHeader.tsx`, `constants.ts`의 `archiveHelpPage`). 그 링크의 글자였던 메시지 5개는 위에서 제거했다. 도구는 남는 문구에서 주소를 발견하면 그 메시지를 변환하지 않고 이유를 적는다. 지금은 `tests/tools/i18n.test.ts`가 메시지 파일에서 같은 꼴을 찾는다.

## 한국어

고친 문구와 채운 문구의 이전·이후는 모두 `docs/ko-review.md`에 있다. 검수는 그 문서로 한다. 여기에는 경위만 적는다.

### 원본 번역에서 읽은 것

처음 변환 때 원본 `ko.json`에서 읽은 것이다.

- 용어: board → 보드, card → 카드, property → 속성, template → 템플릿, category → 카테고리, comment → 댓글, filter → 필터, group → 그룹, sidebar → 사이드바, admin → 관리자, member → 멤버. view는 대부분 "뷰"(`뷰 추가`, `뷰 삭제`)이고 일부가 "보기"였다.
- 어조: 문장은 합니다체(`~습니다`, `~하시겠습니까?`)이고 `~하십시오`와 `~하세요`·`~주세요`가 섞여 있었다. 단추와 메뉴는 명사구이거나 `~하기`였다.
- 문장부호: 말줄임은 점 셋(`...`)이다. 문장에는 마침표를 찍고 단추·제목에는 찍지 않는다.
- 필터 조건은 `~`를 앞에 붙인다(`~로 시작함`, `~를 포함하지 않음`).
- "비어 있음"과 "비어있음"이 함께 쓰였다.

### 채운 번역 (18개)

원본 `ko.json`에 없던 id다. 지금은 `messages/ko.json`에 있다. 각 id가 쓰이는 원본 코드를 읽고 옮겼다. 문구는 `docs/ko-review.md`의 표에서 묶음이 "채움"인 행이다.

- `Filter.is-after`와 `Filter.isafter`, `Filter.is-before`와 `Filter.isbefore`는 원본에 둘 다 있고 쓰는 곳이 다르다(`octoUtils.tsx`, `filterEntry.tsx`). 같은 문구를 넣었다.
- `centerPanel.undefined`는 같은 영어 꼴(`No {property}`)의 기존 번역 `BoardComponent.no-property`("{property} 속성 없음")를 따랐다.
- `TableRow.DuplicateCard`는 실행 취소 설명에 들어간다("카드 복제 실행 취소").
- "비어 있음"은 띄어 쓴 쪽(`DateRange.empty`)을 따랐다.
- 18개 가운데 `boardPage.confirm-join-button`과 `boardPage.confirm-join-title`은 처음에 "참여하기", "비공개 보드 참여하기"로 채웠다가, 단추와 제목을 명사구로 통일하면서 "참여", "비공개 보드 참여"로 고쳤다. 채운 문구는 덮어쓰기의 대상이 아니므로 채운 파일을 바로 고쳤고, 처음 채운 값은 검수 문서의 표에 함께 적었다.

### 고친 번역 (130개)

처음 변환은 원본의 한국어에서 이상해 보이는 문자열 44행을 표로 적어 두고 고치지 않았다. 결정 3에 따라 고쳤다. 그 표는 `docs/ko-review.md`로 옮겼고(행마다 어떻게 됐는지, 이전과 이후), 여기에는 남기지 않았다.

- 오타는 바로잡고 오역은 영어의 뜻에 맞게 다시 옮겼다.
- 같은 영어를 두 가지로 옮긴 곳은 하나로 통일했다. 그 때문에 원 표에 없던 메시지도 함께 고쳤다. 고른 말과 이유는 `docs/ko-review.md`의 "고른 용어와 이유"에 있다.
- 값 뒤에 붙어 이름에 따라 틀리던 조사는 문장을 바꿔 없앴다.
- 제품 이름 "Boards"를 "보드"로 옮겼던 네 문구는 `fvoci`로 고쳤다.
- 영어를 다시 쓴 두 메시지는 한국어도 다시 옮겼다.
- 고친 문구는 지금 `messages/ko.json`에 있고, 고치기 전의 원본 문구는 `docs/ko-review.md`의 표에 있다.

## 측정

같은 기기에서 `cd apps/web && pnpm run i18n && pnpm exec vite build`로 쟀다. "전"은 커밋 `1faf53c0`(로케일 2개, 메시지 8개), "후"는 처음 변환(로케일 18개, `en` 390개)이다. 결정을 반영한 뒤에는 이 표를 다시 재지 않았다. 그 사이에 앱의 다른 코드가 늘어 같은 줄로 견줄 수 없기 때문이다(2026-11-10 20:17 KST의 빌드에서 `index-*.js`는 34,235바이트, br 12,160바이트다. 이 작업이 그 크기를 바꿨는지는 재지 않았다. 빈 화면이 쓰는 메시지는 여전히 둘이다).

| 항목                               | 전 (2026-11-10 18:26 KST) |    후 (2026-11-10 18:49 KST) |
| ---------------------------------- | ------------------------: | ---------------------------: |
| `pnpm run i18n` (paraglide 컴파일) |              0.23초 (1회) |            0.38–0.39초 (3회) |
| 컴파일 결과의 메시지 모듈          |                       8개 | 390개 (출력 폴더 전체 3.9MB) |
| `vite build`가 변환한 모듈         |                       124 |                          506 |
| `vite build`                       |                    0.43초 |                       0.53초 |
| `dist/static/index-*.js`           |              30,921바이트 |          31,768바이트 (+847) |
| `index-*.js.br`                    |              11,161바이트 |          11,396바이트 (+235) |
| `index-*.js.gz`                    |              12,300바이트 |          12,620바이트 (+320) |
| `dist/static/index-*.css`          |              95,162바이트 |          95,342바이트 (+180) |
| `index-*.css.br`                   |              12,484바이트 |           12,434바이트 (−50) |

- **JS가 847바이트 늘어난 까닭.** 빈 화면이 쓰는 메시지는 `app_name`과 `app_loading` 둘이고 다른 메시지의 문구는 번들에 없다. 늘어난 것은 로케일 분기다. paraglide는 메시지 함수마다 로케일별 분기를 만들고, 번역이 없는 로케일도 영어 함수의 별칭과 분기를 하나씩 갖는다(`n==='ca'?Mr(e):…`). 로케일이 2개에서 18개가 되면서 쓰는 메시지마다 약 420바이트가 붙었다.
- **메시지를 다 쓰게 되면.** 메시지 모듈 하나에 그 메시지의 모든 로케일 문구가 들어간다. 메시지 셋을 더 쓴 빌드는 33,849바이트였다(+2,081, br 12,200). `messages/*.json`의 합은 334,589바이트이고 `en`만은 21,950바이트였다(결정을 반영한 뒤에는 321,433바이트와 21,196바이트). 화면이 메시지를 다 쓰면 18개 로케일의 문구가 한 번들에 들어간다. 메시지 384개를 다 쓰는 진입 파일로 재면 287,431바이트(br 62,308)다. 로케일별로 나누는 방법과 그 크기는 아래 "로케일별로 나눠 받기"에 있다.
- **CSS가 180바이트 늘어난 까닭.** 메시지와는 상관없다. Tailwind가 `tools/`도 클래스 후보로 훑는데, 번역 변환 도구(지금은 지웠다)에 있던 낱말 `ordinal`이 Tailwind의 `ordinal` 유틸리티로 잡혀 규칙 하나가 생겼다. 테스트 파일을 쓰기 전에 `src/styles/app.css`에 그 폴더를 빼는 `@source not` 한 줄을 넣어 빌드하니 95,162바이트로 돌아왔다(2026-11-10 18:44 KST). 그때는 그 도구의 테스트에도 같은 낱말이 있어 그 한 줄로는 95,342바이트 그대로였다. `tools`와 `tests`를 통째로 빼면 94,852바이트다(2026-11-10 18:54 KST). 기준 커밋보다도 310바이트 작으니, 기준에서도 이 두 폴더의 낱말이 규칙을 만들고 있었다는 뜻이다. 확인한 뒤 되돌렸고 `app.css`는 고치지 않았다.
- `svelte-check`가 보는 파일은 2,736개다(컴파일 결과 포함).

## 로케일별로 나눠 받기

조사만 했다. 제품의 빌드 설정(`package.json`의 스크립트, `vite.config.ts`, `project.inlang`)은 바꾸지 않았다. 실험은 커밋하지 않는 버리는 프로젝트(저장소 루트의 `target/locale-split/`)에서 했다(2026-11-10 20:05–20:10 KST). W2에서 정할 일이다.

### 지금의 모습

`pnpm run i18n`은 CLI로 컴파일한다(`paraglide-js compile … --strategy globalVariable baseLocale`). 출력 구조는 CLI의 기본값인 `message-modules`다. 메시지마다 모듈이 하나이고, 그 모듈 안에 18개 로케일의 문구와 로케일 분기가 모두 들어 있다. 번들러는 쓰지 않는 메시지를 통째로 떨어뜨리지만, 쓰는 메시지는 18개 로케일의 문구를 함께 끌고 온다. 설치된 패키지의 `README.md`도 그렇게 적는다("Default output retains configured locales for every used message; unused-message tree-shaking alone does not remove inactive locales").

### 설치된 paraglide 3.2.1이 가진 것

기억이 아니라 설치된 패키지에서 확인했다: `node_modules/@inlang/paraglide-js`의 `README.md`, `dist/compiler/compiler-options.d.ts`, `dist/compiler/runtime/create-runtime.js`, `dist/compiler/compile-bundle.js`, `dist/compiler/output-structure/`, `dist/bundler-plugins/vite.d.ts`, `dist/bundler-plugins/per-locale-build/`, 그리고 `paraglide-js compile --help`.

| 옵션                                                        | 어디에 있나       | 하는 일                                                                                                                                                                                                                                                    |
| ----------------------------------------------------------- | ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `outputStructure` (`--output-structure`)                    | 컴파일러, CLI     | `message-modules`(CLI의 기본값. 메시지마다 모듈), `locale-modules`(로케일마다 모듈 하나), `virtual-modules`(Vite 플러그인의 기본값. 플러그인이 있어야 한다). CLI의 도움말은 앞의 둘만 받는다고 적는다                                                      |
| `messageModuleSize` (`--message-module-size`)               | 컴파일러, CLI     | `message-modules`에서 모듈 하나에 메시지를 여러 개 묶는다. 로케일과는 상관없다                                                                                                                                                                             |
| `experimentalStaticLocale` (`--experimental-static-locale`) | 컴파일러, CLI     | 컴파일 시점의 로케일 상수가 될 JavaScript 식. 주석이 "experimental and opt-in"이라고 적는다                                                                                                                                                                |
| `experimentalPerLocaleBuild`                                | Vite 플러그인에만 | 로케일마다 클라이언트 자산 트리를 하나씩 빌드한다. 타입 선언의 주석: Vite 8 이상이 필요하고, `paraglide-vite-locales.json`을 내며, 로케일을 바꿀 때는 문서 전체를 새로 받아야 한다("Locale changes need to use full-document navigation"). `@experimental` |
| `isDevelopment` (`--is-development`)                        | 컴파일러, CLI     | 개발용 진단을 넣을지 정하는 식. 기본값이 `true`다. 로케일별 빌드에서는 메시지마다 경고 문구가 들어가므로 `false`로 줘야 한다(아래 3번)                                                                                                                     |

- 로케일의 문구를 실행 중에 따로 불러오는 옵션은 없다. 출력 구조를 만드는 코드(`dist/compiler/output-structure/*.js`, `compile-bundle.js`)가 내는 `import`는 모두 정적이다. `locale-modules`의 `messages/_index.js`도 18개 로케일 모듈을 정적으로 가져온다.
- `README.md`는 번역을 늦게 불러오는 것을 권하지 않는다(요청이 앱 코드 뒤로 밀리는 폭포. "Don't Lazy-Load Translations"라는 글을 가리킨다). 그 글과 문서 사이트는 읽지 않았다. 바깥 사이트다.

### 해 본 것

버리는 프로젝트는 `messages/*.json`(이 변환의 결과, `en` 384개)과 `project.inlang/settings.json`을 복사하고, **메시지 384개를 모두 부르는 진입 파일 하나**만 둔 것이다. 메시지 말고는 코드가 없으므로 아래 크기는 "화면이 메시지를 다 쓸 때 메시지가 차지하는 양"이다. Vite 8.3.6(Rolldown 1.2.16)의 기본 축소로 빌드했고, gzip은 9단계, brotli는 11단계로 쟀다.

| #   | 방법                                                                                                                                                 | 받는 JS (바이트 / gzip / br)                                                                                                                                                  |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | 지금대로: `message-modules`, 18개 로케일                                                                                                             | 287,431 / 82,101 / 62,308                                                                                                                                                     |
| 2   | 1에 CLI의 `--experimental-static-locale '"ko"'`만 더함                                                                                               | 288,902 / 82,317 / 62,479. **줄지 않는다**                                                                                                                                    |
| 3   | 로케일별 빌드를 손으로: 플러그인이 쓰는 식을 CLI에 주고 한 번 컴파일(`--is-development false`), 로케일마다 `vite build`에 `define`으로 로케일을 넣음 | `en` 31,375 / 8,059 / 6,970, `ko` 33,321 / 8,667 / 7,443. 가장 큰 `ru` 39,297 / 9,947 / 8,433. 18개의 합 606,788. 18번 빌드에 8.5초                                           |
| 4   | Vite 플러그인의 `experimentalPerLocaleBuild: true`                                                                                                   | `en` 36,016 / 9,550 / 7,620, `ko` 38,250 / 10,379 / 8,238. 18개의 합 676,152. `vite build` 한 번에 29초                                                                       |
| 5   | `--output-structure locale-modules`로 컴파일하고, 진입 파일이 `import.meta.glob`으로 로케일 모듈 하나를 동적으로 가져옴                              | 진입 3,441 / 1,214 / 1,039에 로케일 청크 하나: `ko` 27,783 / 8,245 / 7,042. 번역이 빠진 로케일은 영어 문구 청크(15,916 / 5,883 / 4,513)도 받는다(`de` 27,190 / 8,117 / 7,374) |
| 6   | `locale-modules`를 그대로(정적으로) 씀                                                                                                               | 339,986 / 87,447 / 68,912. 1보다 크다                                                                                                                                         |
| 7   | 1에 `--message-module-size 64`                                                                                                                       | 286,793 / 77,631 / 58,472. 로케일은 그대로 다 들어 있다                                                                                                                       |
| 8   | 참고: 로케일을 `en`과 `ko` 둘로 줄인 프로젝트를 1처럼                                                                                                | 54,707 / 13,220 / 11,156                                                                                                                                                      |

- **2가 줄지 않는 까닭.** 컴파일러는 받은 식을 `runtime.js`에 `export const experimentalStaticLocale = assertIsLocale("ko");`로 적는다(`create-runtime.js`). 함수 호출의 결과라 번들러가 상수로 접지 못하고, 메시지 모듈의 다른 로케일 분기가 그대로 남는다. 빌드 결과에 영어와 한국어 문구가 모두 있는 것을 확인했다.
- **3이 줄어드는 까닭.** 식이 플러그인이 쓰는 것(`/** @type {any} */ (globalThis).__PARAGLIDE_STATIC_LOCALE__`, `dist/compiler/per-locale-build.js`)과 글자까지 같을 때만 컴파일러가 그 식을 감싸지 않고 메시지 함수 안에 그대로 넣는다. 그러면 번들러의 `define`(`globalThis.__PARAGLIDE_STATIC_LOCALE__` → `"ko"`)이 그것을 글자 상수로 바꾸고 나머지 로케일의 분기와 문구가 떨어진다. `ko` 빌드에는 영어 문구가, `en` 빌드에는 한국어 문구가 없는 것을 확인했다. 그 파일의 주석은 이 식을 "Internal compiler/bundler contract"라고 부른다. 공개된 약속이 아니다.
- **3에서 `--is-development false`를 빼면** 메시지 함수마다 "options.locale cannot override a locale-specialized client bundle…"이라는 경고 문구가 들어가 `ko`가 103,755바이트(br 8,140)가 된다. 384번 되풀이되는 문구라 압축하면 차이가 작다.
- **3에서 `define` 없이 빌드하면** 18개 로케일이 다 들어간 303,251바이트(br 62,680)가 나온다. 한 번 컴파일한 결과를 로케일별 빌드와 "전부 든 빌드"에 함께 쓸 수 있다는 뜻이다.
- **4의 산출물.** `dist/__paraglide/<로케일>-<16진수>/` 아래에 로케일마다 트리가 하나씩 생기고(`ko-6b6f`, `pt-BR-70742d4252` 등) `dist/paraglide-vite-locales.json`이 로케일별 진입 파일과 파일 목록을 적는다. 플러그인이 컴파일도 맡고(`locale-modules`로) `builder.buildApp`을 차지한다. 다른 플러그인이 `buildApp`을 정했으면 멈추게 되어 있다(`vite-environments.js`). 그 JSON에는 진입 파일의 절대 경로가 들어간다. 배포물에 넣으면 빌드한 기기의 경로가 새어 나간다.
- **5의 청크.** 로케일 청크의 절반 가까이는 문구가 아니라 내보내는 이름이다(384개의 모듈 이름으로 13KB쯤). 번들러는 영어를 둘로 나눈다: 영어 문구가 든 청크(15,916바이트)와, 그것을 영어 로케일의 이름들로 다시 내보내기만 하는 청크(13,011바이트). 번역이 빠진 로케일은 앞의 것을 함께 받고, `en`은 둘 다 받는다. `ko`는 빠진 것이 없어 청크 하나만 받는다. 로케일 모듈은 함수를 원래 id가 아니라 모듈 이름(`boardpage_newversion3`)으로 내보낸다. 원래 id로 다시 내보내는 것은 `_index.js`뿐이고 그 파일은 모든 로케일을 정적으로 가져오므로 이 방법에서는 쓸 수 없다.

### 방법들의 장단점

**가. 로케일별 빌드(3, 4).** 컴파일할 때 로케일을 못 박아, 로케일마다 그 로케일의 문구만 든 번들을 만든다.

- 받는 양이 가장 작다. 메시지를 다 써도 로케일당 31–39KB(br 7–8.4KB)이고, 쓰지 않는 메시지가 떨어지는 것도 그대로다.
- 메시지를 부르는 코드(`m['…']()`, `.parts()`)와 타입이 바뀌지 않는다.
- 산출물이 로케일 수만큼 생긴다. 문구가 든 JS 청크는 로케일마다 다르다. 빌드 시간도 로케일 수만큼 든다(실험의 작은 프로젝트에서 4는 29초였다. 실제 앱에서는 재지 않았다).
- 서버가 사용자의 로케일에 맞는 `index.html`을 골라 줘야 한다. 지금 서버는 `index.html` 하나를 모든 페이지에 내려준다(`vite.config.ts`의 주석). 로그인 전에는 쿠키나 `Accept-Language`로, 로그인 뒤에는 사용자 설정으로 골라야 하므로 서버 트랙과 약속이 필요하다.
- 언어를 바꾸면 문서를 새로 받아야 한다. 런타임의 `setLocale`도 못 박힌 로케일에서 다른 로케일로 갈 때 새로 고침을 강제한다(`runtime/set-locale.js`).
- 4는 `@experimental`이고 `dist`의 배치(`__paraglide/…`)를 스스로 정한다. 지금의 빌드(자리표시자가 든 `index.html`, `static/`의 해시 이름 검사, 미리 압축, 고지문)와 맞추려면 빌드 뒤에 트리들을 하나로 합치는 단계가 필요할 것이다. 3은 그런 제약이 없지만 공개되지 않은 식에 기댄다. 판을 올리면 조용히 2처럼 될 수 있으므로, 쓴다면 "`ko` 빌드에 영어 문구가 없다"를 빌드 검사로 넣어야 한다.

**나. 로케일 모듈의 동적 import(5).** `locale-modules`로 컴파일하고 로케일 모듈을 실행 중에 가져온다.

- 빌드가 한 번이고 산출물이 한 벌이다. 서버는 바뀌지 않는다. 새로 고침 없이 언어를 바꿀 수 있다(다른 청크를 받으면 된다).
- 로케일당 받는 양은 가보다 조금 작거나 비슷하다(`ko` 27.8KB, br 7.0KB에 진입 쪽 3.4KB). 다만 그 로케일의 메시지를 쓰든 안 쓰든 전부 받는다.
- paraglide가 만들어 주는 진입점(`messages.js`, `m`)을 쓸 수 없다. 원래 id와 모듈 이름의 대응, 인자의 타입, `.parts()`, 빠진 번역의 영어 대체를 잇는 층을 이 앱이 직접 만들고 지켜야 한다. 지금 코드의 `m.ui_close()` 같은 호출도 모두 그 층을 거치게 바꿔야 한다.
- 로케일 청크는 진입 코드가 실행된 뒤에야 요청된다. 첫 화면이 그만큼 늦는다. 미리 받게 하려면 서버가 로케일을 알고 `index.html`에 적어 줘야 하는데, 그러면 서버가 바뀌지 않는다는 장점이 줄어든다.

**다. 출력 구조 옵션만 바꾸기(6, 7).** 효과가 없다. `locale-modules`는 정적으로 쓰면 오히려 크고, `messageModuleSize`는 로케일과 상관없다.

**라. 로케일 수를 줄이기(8).** 방법이 아니라 제품의 결정이다. 참고로 쟀다. `en`과 `ko`만이면 메시지를 다 써도 54.7KB(br 11.2KB)다.

### 권고

W2에서는 먼저 실제 화면이 쓰는 메시지로 1을 다시 재고, 나누기로 하면 **가(로케일별 빌드)를, 서버가 로케일별 `index.html`을 고르는 약속을 서버 트랙과 맺은 뒤에** 하는 것을 권한다. 지금 메시지가 차지하는 최대치는 압축 전 287KB, br 62KB이고 로케일별 빌드는 그것을 br 7–8KB로 줄인다. 아끼는 것은 첫 방문의 br 55KB쯤과 그만큼의 파싱이다(해시 이름이라 그 뒤로는 캐시된다). 가는 메시지를 부르는 코드와 타입을 그대로 두고 paraglide가 스스로 내놓은 길(`README.md`의 "Ship only the active locale")을 따르므로, 호출 층을 새로 만들어야 하는 나보다 이 앱이 떠안는 코드가 적다. 가 안에서는 공개된 옵션인 4를 먼저 실제 빌드에 붙여 보고(개발 서버도 플러그인으로 바꿔야 한다), 지금의 빌드 플러그인들과 맞지 않으면 3을 "영어 문구가 없다"는 검사와 함께 쓴다. 서버 쪽 약속을 맺을 수 없으면 나가 남는데, 그때는 호출 층을 만드는 비용과 55KB를 견주어 그냥 지금대로 두는 것도 선택지다. 둘 다 `experimental`이라 판을 올릴 때마다 다시 봐야 한다.

### 확인하지 못한 것

- **실제 앱에 붙여 보지 않았다.** 버리는 프로젝트에는 Svelte 컴포넌트도, 지금의 빌드 플러그인(해시 이름 검사, 미리 압축, 고지문, `renderBuiltUrl`)도 없다. 4가 그것들과 함께 도는지, 3이 코드 분할된 실제 번들에서도 다른 로케일을 다 떨어뜨리는지는 모른다.
- **빌드한 결과를 브라우저에서 실행해 보지 않았다.** 크기와 번들에 든 문구만 봤다. 못 박힌 로케일에서 `getLocale`·`setLocale`이 어떻게 구는지는 소스로만 읽었다.
- **개발 서버.** 4는 Vite 플러그인을 쓰므로 `pnpm run i18n`(CLI)으로 컴파일하는 지금의 `dev`·`test`·`check`·`storybook` 흐름이 어떻게 바뀌어야 하는지 보지 않았다. `virtual-modules` 출력 구조도 해 보지 않았다.
- **로케일 사이에 같은 파일.** 로케일별 트리에서 CSS와 글꼴처럼 내용이 같은 파일이 같은 해시 이름을 받는지(트리들을 하나로 합칠 수 있는지) 확인하지 않았다. 실험에는 JS 하나뿐이었다.
- **빌드 시간.** 8.5초와 29초는 메시지뿐인 프로젝트의 것이다.
- **압축 수치.** `node:zlib`으로 쟀다. 제품 빌드의 미리 압축과 같은 설정인지 맞춰 보지 않았다.
- **paraglide의 문서 사이트.** 패키지 안의 `README.md`와 소스만 읽었다. `README.md`가 가리키는 로케일별 빌드 설명과 벤치마크 페이지는 읽지 않았다.
- **서버.** 로케일별 `index.html`을 고르는 일이 서버 설계에 들어갈 수 있는지 서버 트랙에 묻지 않았다.

## 포팅할 때 알아둘 것

- **요소를 인자로 넘기던 메시지.** 원본은 `ViewHeader.group-by`와 `ViewHeader.display-by`의 `{property}`에 글자가 아니라 `<span>` 요소를 넘겨 색을 입힌다. paraglide의 인자는 문자열로 들어간다. 그 부분만 달리 그리려면 그 컴포넌트를 옮길 때 문구를 마크업(`{#name}…{/name}`)으로 바꿔야 한다. 변환은 문구를 그대로 뒀다. 한국어는 두 문구를 "그룹화 기준: {property}", "표시 기준: {property}"로 고쳐 값이 끝에 온다.
- **문구 안에 다른 문구를 넣는 곳.** `CardDetailProperty.confirm-property-name-change-subtext`의 `{customText}`에는 `CardDetailProperty.property-name-change-subtext`의 결과가 들어간다(원본 `cardDetailProperties.tsx`). 한국어는 두 문구를 이 조립을 전제로 함께 고쳤다(`…다음과 같이 바꾸시겠습니까? 유형: "A" → "B". …`). 한쪽만 쓰거나 다르게 조립하면 문장이 어긋난다.
- **기본 카테고리의 이름.** `SidebarCategories.CategoryMenu.DeleteModal.Body`의 한국어는 기본 카테고리를 서버가 주는 이름 그대로 "Boards 카테고리"라고 부른다. 화면에서 그 이름을 번역해 보여 주기로 하면 이 문구도 함께 바꿔야 한다.
- **값이 없는 인자.** react-intl은 `undefined`인 인자를 빈 문자열로 넣고 paraglide는 `undefined`라는 글자를 넣는다. `centerPanel.undefined`는 원본이 `groupByProperty?.name`을 넘긴다.
- **select의 입력.** 컴파일된 메시지는 값을 `===`로 비교한다. ICU는 문자열로 바꿔 비교한다. 불리언을 넘기던 호출은 문자열로 넘겨야 한다. 지금 자료에는 select가 없다.
- **번역 파일에 없는 id.** 원본 코드에는 `en.json`에 없는 id가 있다. 테마 이름 4개(`Sidebar.default-theme`, `dark-theme`, `light-theme`, `system-theme`: 코드의 `displayName`이 문구다)와 시험용 메뉴 4개(`ViewHeader.test-*`)다. 변환 대상이 아니다. 테마 메뉴를 옮길 때 문구를 `messages/`에 새로 넣어야 한다.
- **`en.json`과 코드의 기본 문구가 다른 곳이 있다.** 예: `CardDetail.limited-body`. 원본은 영어에서도 `en.json`을 쓰므로 `en.json`을 따랐다. 남긴 메시지 전체를 코드의 기본 문구와 대조하지는 않았다.

## 확인하지 못한 것

- **원본 파서와의 일치.** 변환 도구의 ICU 파서는 원본이 쓰는 파서의 규칙(아포스트로피, 태그, `#`, 오류 조건)을 그 소스에 대한 기억으로 옮긴 것이었다. 그 라이브러리는 설치돼 있지 않아 같은 입력으로 나란히 돌려 보지 못했다. 실제 자료에는 글자, 글자 아포스트로피, 단순 인자, `<b>` 태그만 있으므로 차이가 날 수 있는 곳은 이 넷의 처리뿐이다.
- **plural·select 계열의 변환.** 실제 자료가 없어 테스트의 문구로만 확인했다. paraglide로 컴파일해 `en`과 `ru`에서 결과를 맞춰 봤다(one·few·many·other, `=0`, `offset`, 서수, 중첩).
- **변환하지 않은 3개가 원본에서 영어로 나왔는지.** react-intl의 동작에 대한 이해이고 원본을 실행해 보지 않았다.
- **죽은 id의 판정.** `webapp/src`에서 문자열 리터럴로 찾은 결과다. 이 저장소에 없는 코드가 쓰던 id는 알 수 없다.
- **쓰이지만 도달할 수 없는 코드.** 컴포넌트가 `import`는 되지만 실제로는 그려지지 않는 경우까지 따지지는 않았다. `Sidebar.product-tour`는 눈에 띄어 위에 적었다.
- **한국어가 원본에서 빠진 까닭.** `ko.json`은 461개 가운데 437개가 있는데 번들 목록에 없다. 이유는 코드에서 알 수 없었다.
- **채운 한국어와 고친 한국어.** 내가 옮긴 것이고 다른 사람이 검토하지 않았다. `docs/ko-review.md`가 그 검토를 위한 문서다. 새 앱이 이 메시지들을 아직 쓰지 않아 화면에서 본 적도 없다.
- **다른 로케일의 번역 품질.** 구조(인자, 태그, 구문)만 봤고 문구는 읽지 않았다. 이름 치환 결과 13개와, 제품 이름을 찾느라 네 메시지의 번역만 눈으로 봤다.
- **다른 로케일에서 손으로 고친 24개.** 이름을 바꿔 넣은 것이고(`nl`과 `pt-BR`의 한 문장씩은 동사와 관사도 고쳤다) 그 언어를 쓰는 사람이 보지 않았다. 제품 이름을 보통명사로 옮긴 번역을 그대로 둔 것이 그 언어에서 어떻게 읽히는지도 판단하지 않았다.
- **다른 메시지에 남은 제품 이름.** 영어 문구에 "Boards"가 있는 메시지만 봤다. 영어에는 없는데 번역이 제품 이름을 넣은 경우는 찾지 않았다.
- **다시 쓴 두 영어 문구.** 내가 쓴 것이다.
- **메시지를 많이 쓸 때의 트리 셰이킹.** 메시지 2개와 5개를 쓴 빌드에서만 확인했다.
- **`check`의 소요 시간 변화.** 전후를 재지 않았다.

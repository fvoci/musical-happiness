# 원본 번역의 paraglide 메시지 변환

원본 웹앱(`webapp/`, React, react-intl)의 번역 파일 `webapp/i18n/*.json`을 새 웹앱(`apps/web`, paraglide)의 메시지 파일 `apps/web/messages/*.json`으로 옮긴 기록이다. 숫자와 목록은 `tools/i18n/convert.ts --report`의 출력에서 가져왔다(2026-11-10 18:49 KST 실행). 원본 참조는 저장소 기준 상대 경로다. 옛 제품 이름과 옛 회사 이름은 이 문서에 적지 않고 "옛 제품 이름", "옛 회사 이름"이라고 쓴다.

## 요약

- 원본에는 번역 파일이 38개 있지만 원본이 실제로 제공하는 로케일은 17개다. 이 17개와 한국어를 변환했다. 결과 로케일은 18개다. 한국어는 원본의 17개에 들어 있지 않다.
- 영어 메시지 461개 가운데 382개를 남겼다. 28개는 포팅하지 않는 기능의 것이라 뺐고, 51개는 원본 코드 어디에서도 쓰지 않아 뺐다. 여기에 이 앱에만 있는 키 8개(`app_*`, `ui_*`)가 더해져 `en`은 390개다.
- 메시지 id는 그대로다(`BoardComponent.add-a-group`). 코드에서는 `m['BoardComponent.add-a-group']()`로 부른다. 트리 셰이킹은 유지된다.
- 18개 파일 전체에서 쓰인 ICU 구문은 단순 인자 `{name}`과 `<b>` 태그뿐이다. plural, select, selectordinal, number, date, time, `''`, 아포스트로피 인용은 한 번도 나오지 않는다. 구문 오류도 없다.
- 변환하지 않은 메시지는 3개다(`id` 2개, `it` 1개). 영어 메시지에 없는 인자를 쓴다.
- `ko`는 `en`과 키 집합이 같다. 원본 `ko.json`에 없던 18개를 새로 번역해 채웠다.
- 빈 화면 빌드의 JS는 30,921 → 31,768바이트(br 11,161 → 11,396)다.

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

이 표는 `tools/i18n/locales.ts`에 있다. 변환 도구는 실행할 때마다 `webapp/src/i18n.tsx`의 `import` 목록을 읽어 이 표와 다르면 멈춘다. `project.inlang/settings.json`의 `locales`는 이 순서의 18개이고 `baseLocale`은 `en` 그대로다. 원본이 사용자 설정에 저장하던 코드(`pt-br`, `zh-cn`, `zh-tw`)는 새 태그와 다르다. 사용자 설정을 옮긴다면 셋째 열을 써야 한다.

## 구성과 다시 돌리는 법

```
webapp/i18n/<파일>.json            원본. ICU MessageFormat. 읽기만 한다
apps/web/messages-own/<태그>.json   이 앱에만 있는 메시지(app_*, ui_*). inlang 형식. 손으로 고친다
apps/web/messages-fill/<태그>.json  원본의 그 로케일 파일에 없는 id의 번역. ICU 형식. 손으로 고친다
apps/web/tools/i18n/removed.json    포팅하지 않는 기능의 id와 이유. 손으로 고친다
apps/web/messages/<태그>.json       생성물. 손으로 고치지 않는다
```

```bash
pnpm --filter web run i18n:convert   # messages/*.json 을 다시 쓴다
pnpm --filter web run i18n:keys      # 로케일별 키 집합을 en 과 비교한다 (--verbose 로 빠진 키 전부)
pnpm --filter web run i18n           # paraglide 컴파일
cd apps/web && node tools/i18n/convert.ts --check    # 생성물이 원천과 같은지만 본다
cd apps/web && node tools/i18n/convert.ts --report   # 이 문서의 숫자와 목록(JSON)
```

- **생성물과 원천을 나눴다.** `messages/*.json`은 도구가 매번 통째로 다시 쓴다. 거기에 직접 넣은 키는 다음 실행에서 사라진다. 이 앱에만 있는 키는 `messages-own/en.json`과 `messages-own/ko.json`에 넣고 `i18n:convert`를 돌린다. 지금 있는 8개(`app_name`, `app_loading`, `ui_*` 6개)를 그리로 옮겼다. 자체 키는 소문자 스네이크 표기여야 하고 원본 id와 겹치면 도구가 멈춘다.
- **`check`에 두 가지를 넣었다.** `node tools/i18n/convert.ts --check`는 `messages/*.json`이 도구가 쓸 내용과 한 글자라도 다르면 실패한다. `messages/`를 직접 고친 것을 잡기 위한 것이고, 요청에는 없던 추가다. `node tools/i18n/check-keys.ts`는 요청대로다. 어떤 로케일에든 `en`에 없는 키가 있거나 `ko`에 빠진 키가 있으면 실패한다. 다른 로케일의 빠진 키는 개수만 출력한다.
- **출력 순서.** 자체 키가 먼저(`messages-own/en.json`의 순서), 그다음 원본 id가 `en.json`의 순서로 온다. 모든 로케일이 같은 순서다. 같은 입력이면 같은 바이트가 나온다(테스트가 두 번 돌려 비교한다).
- **`messages-fill`은 원본 형식이다.** 원본 파일에 빠진 항목을 대신하는 것이라 ICU로 적고, 원본과 같은 변환을 거친다. 원본에 이미 변환되는 문구가 있는 id를 여기에 적으면 도구가 멈춘다. 덮어쓰기는 없다.
- `messages/`를 `.prettierignore`에 넣었다. 형식은 도구가 정하고 `--check`가 지킨다.
- `project.inlang/settings.json`은 도구가 쓰지 않는다. `locales`가 `locales.ts`와 다르면 도구가 멈춘다.

과제가 정한 출력 위치(`messages/<태그>.json`)를 따랐다. 플러그인은 `pathPattern`에 배열을 받아 여러 파일을 합치므로, 자체 키를 합치지 않고 따로 둔 채 플러그인이 합치게 하는 구성도 가능하다. 쓰지 않았다.

## 코드에서 메시지를 부르는 법

paraglide 3.2.1은 점과 하이픈이 있는 키를 그대로 받는다. 컴파일러 소스(`node_modules/@inlang/paraglide-js/dist/compiler/safe-module-id.js`)와 실제 컴파일 결과로 확인했다(2026-11-10 18:26 KST, 버리는 프로젝트를 컴파일해 호출).

- 메시지마다 모듈이 하나 생긴다. 파일 이름은 id를 소문자로 바꾸고 `[a-z0-9_]`가 아닌 글자를 `_`로 바꾼 뒤 대문자 개수를 붙인 것이다: `BoardComponent.add-a-group` → `messages/boardcomponent_add_a_group2.js`.
- 그 모듈은 함수를 **원래 id를 이름으로** 내보낸다. 문자열 이름 export다(ES2022).
  ```js
  export { boardcomponent_add_a_group2 as 'BoardComponent.add-a-group' };
  ```
- 서로 다른 id가 같은 파일 이름이 되면 한쪽이 `$a$2e$b` 꼴의 이름을 받는다. 지금의 390개에서는 그런 충돌이 없다.

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

조사 대상은 번들 17개와 `ko`의 원본 파일 전체다. 제거한 메시지도 센다. 원본의 파서(react-intl 5가 쓰는 것)와 같은 규칙으로 파싱했다(`tools/i18n/icu.ts`).

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

- **인자 `{name}`: 37개 메시지, 42군데.** `AttachmentBlock.addElement`, `AttachmentElement.upload-percentage`, `BoardComponent.no-property`, `BoardComponent.no-property-title`, `BoardsUnfurl.Remainder`†, `BoardsUnfurl.Updated`†, `CardDetailProperty.confirm-delete-subtext`, `CardDetailProperty.confirm-property-name-change-subtext`, `CardDetailProperty.property-deleted`, `CardDetailProperty.property-name-change-subtext`, `ColorOption.selectColor`, `ContentBlock.addElement`, `DeleteBoardDialog.confirm-info`, `DeleteBoardDialog.confirm-info-template`, `FindBoardsDialog.NoResultsFor`, `GroupBy.hideEmptyGroups`, `GroupBy.showHiddenGroups`, `ShareBoard.teamPermissionsText`, `SidebarCategories.CategoryMenu.DeleteModal.Body`, `UndoRedoHotKeys.canRedo-with-description`, `UndoRedoHotKeys.canUndo-with-description`, `ViewHeader.display-by`, `ViewHeader.group-by`, `boardSelector.confirm-link-board-subtext`†, `boardSelector.confirm-link-board-subtext-with-other-channel`†, `centerPanel.undefined`, `notification-box-card-limit-reached.title`†, `notification-box.card-limit-reached.not-admin.text`†, `notification-box.card-limit-reached.text`†, `person.add-user-to-board`, `person.add-user-to-board-question`, `person.add-user-to-board-warning`, `rhs-boards.last-update-at`†, `rhs-boards.link-boards-to-channel`†, `rhs-boards.no-boards-linked-to-channel`†, `shareBoard.confirm-change-team-role.body`, `shareBoard.confirm-link-channel-subtext-with-other-channel`†
- **태그 `<b>`: 3개 메시지, 5군데.** `FindBoardsDialog.SubTitle`, `SidebarCategories.CategoryMenu.DeleteModal.Body`, `shareBoard.confirm-change-team-role.body`
- **글자 아포스트로피: 22개 메시지.** `AttachmentBlock.failed`, `BoardComponent.no-property-title`, `CardDialog.editing-template`, `CardDialog.nocard`, `ContentBlock.errorText`, `Filter.not-contains`, `Filter.not-ends-with`, `Filter.not-includes`, `Filter.not-starts-with`, `SidebarCategories.CategoryMenu.DeleteModal.Body`, `VersionMessage.help`†, `WelcomePage.NoThanks.Text`†, `Workspace.editing-board-template`, `createImageBlock.failed`, `error.invalid-read-only-board`, `error.not-logged-in`, `guest-no-board.subtitle`, `imagePaste.upload-failed`, `login.register-button`, `person.add-user-to-board-warning`, `rhs-board-non-admin-msg`†, `shareBoard.confirm-unlink.body`†
- plural, select, selectordinal, number, date, time, `#`, `''`, 아포스트로피 인용, 중첩 구문: 0개.

## 변환 규칙

설치된 `@inlang/plugin-message-format` 4.6.0의 형식을 그 패키지의 README와 `dist/index.js`(패턴·선언·매치 파서)로 확인했다. 메시지 값은 문자열(단순 메시지)이거나, 객체 하나를 담은 배열(복합 메시지: `declarations`, `selectors`, `match`)이다. 패턴 안에서 `{…}`는 자리표시자이고 글자 `{`, `}`, `\`는 `\{`, `\}`, `\\`로 쓴다. `{#b}…{/b}`는 마크업, `{n: number}`는 함수가 붙은 자리표시자다. 선언은 `input x`, `local y = x: plural`이고, 매치 키는 `선택자=값, 선택자=값`이며 `*`가 나머지 전부다.

실제 자료에 나오는 구문은 처음 넷이다. 나머지는 자료에 없지만 지원하고, 단위 테스트가 변환 결과와 paraglide로 컴파일한 뒤의 실행 결과를 함께 확인한다(`tests/tools/i18n.test.ts`).

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

| 로케일    | 원본 파일      | 원본 메시지 | 변환 | 변환 안 함 | 제거: 미포팅 | 제거: 죽은 id | 채움 | 자체 키 | 결과 키 | en 대비 빠짐 |
| --------- | -------------- | ----------: | ---: | ---------: | -----------: | ------------: | ---: | ------: | ------: | -----------: |
| `en`      | `en.json`      |         461 |  382 |          0 |           28 |            51 |    0 |       8 |     390 |            0 |
| `ko`      | `ko.json`      |         437 |  364 |          0 |           27 |            46 |   18 |       8 |     390 |            0 |
| `ca`      | `ca.json`      |         195 |  191 |          0 |            1 |             3 |    0 |       0 |     191 |          199 |
| `de`      | `de.json`      |         460 |  381 |          0 |           28 |            51 |    0 |       0 |     381 |            9 |
| `el`      | `el.json`      |          47 |   47 |          0 |            0 |             0 |    0 |       0 |      47 |          343 |
| `es`      | `es.json`      |         226 |  213 |          0 |            6 |             7 |    0 |       0 |     213 |          177 |
| `fr`      | `fr.json`      |         413 |  342 |          0 |           27 |            44 |    0 |       0 |     342 |           48 |
| `id`      | `id.json`      |         149 |  142 |          2 |            5 |             0 |    0 |       0 |     142 |          248 |
| `it`      | `it.json`      |         309 |  293 |          1 |            9 |             6 |    0 |       0 |     293 |           97 |
| `ja`      | `ja.json`      |         449 |  371 |          0 |           27 |            51 |    0 |       0 |     371 |           19 |
| `nl`      | `nl.json`      |         449 |  371 |          0 |           27 |            51 |    0 |       0 |     371 |           19 |
| `oc`      | `oc.json`      |         148 |  142 |          0 |            4 |             2 |    0 |       0 |     142 |          248 |
| `pt-BR`   | `pt_BR.json`   |         460 |  381 |          0 |           28 |            51 |    0 |       0 |     381 |            9 |
| `ru`      | `ru.json`      |         428 |  364 |          0 |           22 |            42 |    0 |       0 |     364 |           26 |
| `sv`      | `sv.json`      |         449 |  371 |          0 |           27 |            51 |    0 |       0 |     371 |           19 |
| `tr`      | `tr.json`      |         460 |  381 |          0 |           28 |            51 |    0 |       0 |     381 |            9 |
| `zh-Hans` | `zh_Hans.json` |         449 |  371 |          0 |           27 |            51 |    0 |       0 |     371 |           19 |
| `zh-Hant` | `zh_Hant.json` |         449 |  371 |          0 |           27 |            51 |    0 |       0 |     371 |           19 |

- "원본 메시지"는 원본 파일의 키 수다. "변환 + 변환 안 함 + 제거: 미포팅 + 제거: 죽은 id"가 그 수와 같다.
- "결과 키"는 `messages/<태그>.json`의 키 수(변환 + 채움 + 자체 키)다.
- "en 대비 빠짐"은 `en`의 390개 가운데 그 로케일에 없는 키 수다. 자체 키 8개는 `en`과 `ko`에만 있으므로 다른 로케일은 모두 8 이상이다. 빠진 키는 영어로 나온다.
- 원본 파일에 `en.json`에 없는 키가 있는 로케일은 없다.

## 변환하지 않은 메시지

| 로케일 | id                                                    | 이유 (도구의 출력)                                       |
| ------ | ----------------------------------------------------- | -------------------------------------------------------- |
| `id`   | `BoardTemplateSelector.plugin.no-content-description` | not in the English message: {lineBreak}, {workspaceName} |
| `id`   | `BoardTemplateSelector.plugin.no-content-title`       | not in the English message: {workspaceName}              |
| `it`   | `BoardTemplateSelector.plugin.no-content-description` | not in the English message: {lineBreak}, {teamName}      |

셋 다 옛 영어 문구의 번역이 남은 것이다. 지금의 영어 문구(`Create a board`, `Add a board to the sidebar using any of the templates defined below or start from scratch.`)에는 인자가 없다. 원본에서도 이 호출은 값을 넘기지 않으므로 react-intl이 형식화에 실패하고 코드의 영어 기본 문구를 보여 줬을 것이다(실행해 확인하지는 않았다). 결과는 같다: 이 세 메시지는 `id`, `it`에서 영어로 나온다.

## 제거한 메시지

새 앱이 포팅하지 않는 기능에서만 쓰이는 메시지 28개다. 목록과 이유는 `tools/i18n/removed.json`에 있다. id마다 `webapp/src`(테스트와 스냅숏 제외)에서 문자열 리터럴로 찾아 쓰이는 파일을 확인했다. 도구가 실행할 때마다 같은 검색을 하므로 "쓰던 파일" 열은 그 출력이다.

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
- `SidebarTour.SidebarCategories.Link`: 둘러보기의 한 단계에 붙은 "Learn more" 링크로, 옛 회사의 문서 사이트로 간다. 그 단계의 제목과 본문은 남겼다.

과제의 미포팅 목록 가운데 나머지에는 해당 메시지가 없다. `channels` 스토어 조각, `initPlugin`, 텔레메트리, `nativeApp.ts`는 메시지를 쓰지 않는다. 통계(`SiteStats.*`)와 채널 쪽 화면(`rhs-*`, `boardSelector.*` 등)의 메시지는 `webapp/src`에 쓰는 곳이 없어 아래 "죽은 id"에 들어갔다. 가져오기 도움말 페이지 링크의 글자는 메시지가 아니라 코드에 박힌 이름이다.

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

## 판단이 서지 않아 남긴 것

제거 기준에 걸칠 수 있지만 확신이 없어 남긴 메시지다. 지금은 모두 `messages/`에 들어 있다. 빼려면 `removed.json`에 넣고 `i18n:convert`를 돌린다.

| id                                                                                                                                                                 | 걸리는 점                                                                                                                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Sidebar.product-tour`                                                                                                                                             | 쓰는 곳이 `globalHeaderSettingsMenu.tsx`뿐이고, 그 위의 `GlobalHeader`는 `webapp/src` 안에서 `import`되지 않는다(플러그인 진입점용). 둘러보기를 다시 시작하는 메뉴라 둘러보기와 함께 남겼다 |
| `SidebarTour.SidebarCategories.Title`, `.Body`                                                                                                                     | 둘러보기 단계지만 본문이 옛 제품의 v7.2 업그레이드와 워크스페이스 폐지를 설명한다. 링크만 제거했다                                                                                          |
| `BoardTemplateSelector.plugin.no-content-title`, `.plugin.no-content-description`                                                                                  | id에 `plugin`이 있다. `workspace.tsx`가 채널 경로일 때뿐 아니라 고른 보드가 없을 때도 쓴다                                                                                                  |
| `ShareBoard.searchPlaceholder`                                                                                                                                     | 문구가 "Search for people and channels"다. 공유 대화상자의 검색창 자리표시자로 쓰인다. 채널이 없으면 `ShareTemplate.searchPlaceholder`("Search for people")가 맞다                          |
| `OnboardingTour.CopyLink.Body`                                                                                                                                     | "channel, direct message, or group message"에 붙여 넣으라고 한다                                                                                                                            |
| `AdminBadge.SystemAdmin`, `AdminBadge.TeamAdmin`, `badge.guest`, `guest-no-board.title`, `guest-no-board.subtitle`                                                 | 시스템 관리자·팀 관리자·게스트는 옛 회사 제품의 계정 종류다. 새 서버에 같은 구분이 있는지 모른다                                                                                            |
| `ShareBoard.teamPermissionsText`, `shareBoard.confirm-change-team-role.*` 3개, `error.back-to-team`, `error.team-undefined`                                        | 팀 개념을 전제한다                                                                                                                                                                          |
| `BoardPage.newVersion`, `OnboardingTour.OpenACard.Body`, `error.invalid-read-only-board`, `error.not-logged-in`, `SidebarCategories.CategoryMenu.DeleteModal.Body` | 영어 문구가 "Boards"를 제품 이름처럼 쓴다("A new version of Boards is available", "Log in to access Boards"). 금지된 두 이름은 아니어서 바꾸지 않았다. `fvoci`로 바꿀지는 정해야 한다       |
| `Error.mobileweb`                                                                                                                                                  | "Mobile web support is currently in early beta"라는 옛 제품의 안내다                                                                                                                        |

## 이름 치환

- **옛 제품 이름.** 18개 파일에서 `Sidebar.about`에만 있었고(`el`을 뺀 17개 로케일), 그 메시지는 제거했다. 남은 메시지에는 없다. 도구는 남는 문구에 이 이름이 있으면 대소문자와 상관없이 `fvoci`로 바꾼다.
- **옛 회사 이름.** `OnboardingTour.AddComments.Body` 하나에 있다(13개 로케일). 댓글에서 다른 사용자를 @멘션하라는 문구에서 "이 제품의 사용자"를 가리키므로 `fvoci`로 바꿨다. 한국어는 한글로 음역해 적혀 있어서 그 표기도 함께 바꾼다. 치환은 기계적이다. 아래에서 `nl`은 합성어 안에 들어가 어색하다(`medefvocigebruikers`).
- 도구는 이 메시지가 아닌 곳에서 옛 회사 이름을 만나면 멈춘다. 회사나 그 회사의 다른 제품에 관한 문구는 치환이 아니라 판단이 필요하기 때문이다. 지금은 그런 문구가 없다.
- 라틴 문자와 한글 말고 다른 문자로 음역한 표기(키릴, 가나 등)가 있는지 몇 가지 철자로 찾아봤고 없었다. 모든 철자를 다 찾았다고는 할 수 없다.

`OnboardingTour.AddComments.Body`의 치환 결과:

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

`apps/web` 아래의 메시지, 도구, 테스트, 이 문서에 두 이름은 없다(`node tools/names/check.ts`). 도구와 테스트는 `names/check.ts`처럼 이름을 반쪽 둘로 이어 만든다.

## 바깥 URL이 든 메시지

없다. 18개 원본 파일의 모든 문구에서 `http://`, `https://`, `www.`, `이름.com|org|net|io|html` 꼴을 찾았고 한 건도 없었다. 원본의 바깥 링크는 문구가 아니라 코드에 있다(`sidebarUserMenu.tsx`, `topBar.tsx`, `messages/versionMessage.tsx`, `onboardingTour/sidebarCategories/sidebarCategories.tsx`, `globalHeader/globalHeader.tsx`, `constants.ts`의 `archiveHelpPage`). 그 링크의 글자였던 메시지 5개는 위에서 제거했다. 도구는 남는 문구에서 주소를 발견하면 그 메시지를 변환하지 않고 이유를 적는다.

## 한국어

### 기존 번역에서 읽은 것

- 용어: board → 보드, card → 카드, property → 속성, template → 템플릿, category → 카테고리, comment → 댓글, filter → 필터, group → 그룹, sidebar → 사이드바, admin → 관리자, member → 멤버. view는 대부분 "뷰"(`뷰 추가`, `뷰 삭제`)이고 일부가 "보기"다.
- 어조: 문장은 합니다체(`~습니다`, `~하시겠습니까?`)이고 `~하십시오`와 `~하세요`·`~주세요`가 섞여 있다. 단추와 메뉴는 명사구이거나 `~하기`다.
- 문장부호: 말줄임은 점 셋(`...`)이다. 문장에는 마침표를 찍고 단추·제목에는 찍지 않는다.
- 필터 조건은 `~`를 앞에 붙인다(`~로 시작함`, `~를 포함하지 않음`).
- "비어 있음"과 "비어있음"이 함께 쓰인다.

### 채운 번역 (18개)

원본 `ko.json`에 없던 id다. `messages-fill/ko.json`에 있고 고치면 다음 `i18n:convert`에 반영된다. 각 id가 쓰이는 원본 코드를 읽고 옮겼다.

| 키                              | 영어                                                                                                                                       | 채운 한국어                                                                                          |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------- |
| `AdminBadge.SystemAdmin`        | Admin                                                                                                                                      | 관리자                                                                                               |
| `AdminBadge.TeamAdmin`          | Team Admin                                                                                                                                 | 팀 관리자                                                                                            |
| `ConfirmPerson.empty`           | Empty                                                                                                                                      | 비어 있음                                                                                            |
| `ConfirmPerson.search`          | Search...                                                                                                                                  | 검색...                                                                                              |
| `ContentBlock.errorText`        | You've exceeded the size limit for this content. Please shorten it to avoid losing data.                                                   | 이 콘텐츠의 크기 제한을 초과했습니다. 데이터가 손실되지 않도록 내용을 줄여 주세요.                   |
| `DateFilter.empty`              | Empty                                                                                                                                      | 비어 있음                                                                                            |
| `Filter.is-after`               | is after                                                                                                                                   | ~이후                                                                                                |
| `Filter.is-before`              | is before                                                                                                                                  | ~이전                                                                                                |
| `Filter.isafter`                | is after                                                                                                                                   | ~이후                                                                                                |
| `Filter.isbefore`               | is before                                                                                                                                  | ~이전                                                                                                |
| `FilterValue.empty`             | (empty)                                                                                                                                    | (비어 있음)                                                                                          |
| `PersonProperty.me`             | Me                                                                                                                                         | 나                                                                                                   |
| `TableRow.DuplicateCard`        | duplicate card                                                                                                                             | 카드 복제                                                                                            |
| `boardPage.confirm-join-button` | Join                                                                                                                                       | 참여하기                                                                                             |
| `boardPage.confirm-join-text`   | You are about to join a private board without explicitly being added by the board admin. Are you sure you wish to join this private board? | 보드 관리자가 직접 추가하지 않은 비공개 보드에 참여하려고 합니다. 이 비공개 보드에 참여하시겠습니까? |
| `boardPage.confirm-join-title`  | Join private board                                                                                                                         | 비공개 보드 참여하기                                                                                 |
| `centerPanel.undefined`         | No {propertyName}                                                                                                                          | {propertyName} 속성 없음                                                                             |
| `centerPanel.unknown-user`      | Unknown user                                                                                                                               | 알 수 없는 사용자                                                                                    |

- `Filter.is-after`와 `Filter.isafter`, `Filter.is-before`와 `Filter.isbefore`는 원본에 둘 다 있고 쓰는 곳이 다르다(`octoUtils.tsx`, `filterEntry.tsx`). 같은 문구를 넣었다.
- `centerPanel.undefined`는 같은 영어 꼴(`No {property}`)의 기존 번역 `BoardComponent.no-property`("{property} 속성 없음")를 따랐다.
- `TableRow.DuplicateCard`는 실행 취소 설명에 들어간다("카드 복제 실행 취소하기").
- "비어 있음"은 띄어 쓴 쪽(`DateRange.empty`)을 따랐다.

### 이상해 보이는 기존 문자열

고치지 않았다. 남긴 메시지만 봤다. 원본 `ko.json`의 문구 그대로다.

| id                                                             | 지금의 한국어                                                                 | 영어                                                                                     | 이상한 점                                                                                                      |
| -------------------------------------------------------------- | ----------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `Sidebar.import`                                               | 사이드바 가져요기                                                             | Import                                                                                   | 오타, "사이드바"는 영어에 없다                                                                                 |
| `RegistrationLink.regenerateToken`                             | 토큰 재성성                                                                   | Regenerate token                                                                         | 오타(재생성)                                                                                                   |
| `ShareBoard.tokenRegenrated`                                   | 토큰이 재성생되었음                                                           | Token regenerated                                                                        | 오타(재생성)                                                                                                   |
| `View.NewGalleryTitle`                                         | 갤리리 형태로 보기                                                            | Gallery view                                                                             | 오타(갤러리)                                                                                                   |
| `SidebarTour.ManageCategories.Body`                            | …사용자의 카레고리로 이동해도…                                                | …moving a board to your category…                                                        | 오타(카테고리)                                                                                                 |
| `person.add-user-to-board-warning`                             | {username}은 보드에 참여중이지 않으며, 관련될 알림을 수신하지 않습니다.       | {username} isn't a member of the board, and won't receive any notifications about it.    | "관련될"(관련된)                                                                                               |
| `tutorial_tip.seen`                                            | 전에 본적 있나요?                                                             | Seen this before?                                                                        | 띄어쓰기(본 적), 어조가 다른 문구와 다르다                                                                     |
| `shareBoard.lastAdmin`                                         | 보드에는 최소한 한명 이상의 관리자 있어야 합니다                              | Boards must have at least one Administrator                                              | 조사 빠짐(관리자가)                                                                                            |
| `Filter.contains`                                              | 필터를 포함하다                                                               | contains                                                                                 | 오역. 다른 조건처럼 "~를 포함함"이어야 한다                                                                    |
| `UndoRedoHotKeys.cannotRedo`                                   | 다시 실행하지 않기                                                            | Nothing to Redo                                                                          | 오역. "다시 실행할 것이 없음"이라는 뜻이다                                                                     |
| `UndoRedoHotKeys.cannotUndo`                                   | 실행취소 하지 않기                                                            | Nothing to Undo                                                                          | 위와 같다                                                                                                      |
| `ViewHeader.view-header-menu`                                  | 머리글 메뉴 보기                                                              | View header menu                                                                         | 오역. View를 동사로 읽었다(뷰 머리글 메뉴)                                                                     |
| `FindBoardsDialog.IntroText`                                   | 보드에서 검색                                                                 | Search for boards                                                                        | 오역(보드 검색)                                                                                                |
| `Sidebar.product-tour`                                         | 상품 둘러보기                                                                 | Product tour                                                                             | product를 "상품"으로 옮겼다(제품)                                                                              |
| `TableRow.MoreOption`                                          | 더 많은 행동                                                                  | More actions                                                                             | 직역                                                                                                           |
| `ConfirmationDialog.confirm-action`                            | 결정하기                                                                      | Confirm                                                                                  | 다른 곳은 "확인"이다                                                                                           |
| `Calculations.Options.sum.label`, `.displayName`               | 더하기                                                                        | Sum                                                                                      | "합계"가 맞다                                                                                                  |
| `Calculations.Options.earliest.*`, `latest.*`                  | 이른 순으로, 늦은 순으로                                                      | Earliest, Latest                                                                         | 정렬이 아니라 가장 이른·늦은 값이다                                                                            |
| `Calculations.Options.countValue.label`                        | 계산 값                                                                       | Count value                                                                              | Count를 "계산"으로 옮겼다(값 개수)                                                                             |
| `Calculations.Options.countUniqueValue.label`                  | 고유 값 계산                                                                  | Count unique values                                                                      | 위와 같다                                                                                                      |
| `Calculations.Options.countChecked.*`, `percentChecked.*`      | 확인됨, 확인된 수, 선택된 비율                                                | Checked, Count checked, Percent checked                                                  | checked가 "확인"과 "선택"으로 갈린다                                                                           |
| `GroupBy.hideEmptyGroups`                                      | {count} 그룹 숨기기                                                           | Hide {count} empty groups                                                                | "빈"이 빠졌다                                                                                                  |
| `SidebarCategories.CategoryMenu.DeleteModal.Body`              | \<b>{categoryName}\</b>의 다시 보드 카테고리로 이동합니다. …                  | Boards in \<b>{categoryName}\</b> will move back to the Boards categories. …             | 주어("보드가")가 빠져 문장이 안 된다                                                                           |
| `SidebarTour.SearchForBoards.Body`                             | (Cmd/Ctrl + K)를 열어 보드를 빠르게 검색하고…                                 | Open the board switcher (Cmd/Ctrl + K) to…                                               | "보드 전환기"가 빠졌다                                                                                         |
| `SidebarTour.SidebarCategories.Body`                           | …워크스페이스 간 전환은… 이전 작업 공간을 기반으로…                           | …workspaces…                                                                             | 같은 말을 두 가지로 옮겼다                                                                                     |
| `OnboardingTour.AddComments.Body`                              | @mention 을 이용하여 이슈에 대해 … 사용자에게 알릴 수 있습니다.               | You can comment on issues, and even @mention your fellow … users to get their attention. | "댓글을 달 수 있다"가 빠졌고 영어 `@mention`이 남았다                                                          |
| `OnboardingTour.AddView.Body`                                  | …보드를 구성할 새 보기를 작성하려면…                                          | …create a new view…                                                                      | 제목은 "뷰 추가"인데 본문은 "보기"다                                                                           |
| `CardDetailProperty.confirm-property-name-change-subtext`      | 정말로 "{propertyName}"속성을 {customText}로 바꾸시겠습니까? …                | Are you sure you want to change property "{propertyName}" {customText}? …                | `{customText}`에 들어가는 문구(`CardDetailProperty.property-name-change-subtext`)가 "…로"로 끝나 "로로"가 된다 |
| `CardDialog.delete-confirmation-dialog-heading`, `-attachment` | 카드 삭제 확인!, 첨부 파일 삭제 확인!                                         | Confirm card delete, Confirm attachment delete                                           | 영어에 없는 느낌표                                                                                             |
| `DeleteBoardDialog.confirm-info-template`                      | {boardTitle} 보드 템플릿을 삭제 하시겠습니까?                                 | Are you sure you want to delete the board template “{boardTitle}”?                       | 따옴표가 빠졌다. 옆의 `confirm-info`에는 있다                                                                  |
| `FindBoardsDialog.SubTitle`                                    | …선택은 \<b>ENTER\</b> , 해제는 \<b>ESC\</b>                                  | …\<b>ENTER\</b> to select, \<b>ESC\</b> to dismiss                                       | dismiss를 "해제"로 옮겼다(닫기), 쉼표 앞 공백                                                                  |
| `ShareBoard.PublishTitle`                                      | 웹에 게시하다                                                                 | Publish to the web                                                                       | 제목인데 기본형이다. `share-board.publish`는 "게재하기"다                                                      |
| `ShareBoard.userPermissionsYouText`                            | 당신                                                                          | (You)                                                                                    | 괄호가 빠졌다                                                                                                  |
| `shareBoard.confirm-change-team-role.body`                     | …\<b>{role}권한으로 승급합니다.\</b> …                                        | …will \<b>now be promoted to {role}\</b>. …                                              | 마침표가 굵은 글씨 안에 있고 첫 `{role}`의 따옴표가 빠졌다                                                     |
| `ViewHeader.group-by`                                          | {property}로 그룹화                                                           | Group by: {property}                                                                     | 받침 있는 이름 뒤에서 조사가 틀린다(으로)                                                                      |
| `person.add-user-to-board`, `-question`                        | {username}을 보드에 추가                                                      | Add {username} to board                                                                  | 조사가 이름에 따라 틀린다(을/를)                                                                               |
| `error.page.title`                                             | 죄송해요, 뭔가 잘못되었어요                                                   | Sorry, something went wrong                                                              | 혼자 해요체다                                                                                                  |
| `guest-no-board.title`, `.subtitle`                            | 보드에 참여중이 아님 / 당신은 이 팀의 어느 보드에도 접속하고 있지 않습니다. … | No boards yet / You don't have access to any board in this team yet, …                   | access를 "접속"으로 옮겼다(접근 권한)                                                                          |
| `BoardComponent.new`, `TableComponent.plus-new`                | + 추가하기, + 생성                                                            | + New (둘 다)                                                                            | 같은 영어를 두 가지로 옮겼다                                                                                   |
| `Sidebar.export-archive`, `Sidebar.import-archive`             | 아카이브 내보내기, 아카이브 들여오기                                          | Export archive, Import archive                                                           | import가 "들여오기"와 "가져오기"로 갈린다                                                                      |
| `PropertyType.MultiSelect`, `PropertyType.Select`              | 다중 선택하기, 선택                                                           | Multi select, Select                                                                     | 한쪽만 "~하기"다                                                                                               |
| `CardActionsMenu.copiedLink`                                   | 복사!                                                                         | Copied!                                                                                  | 다른 세 곳은 "복사되었습니다!"다                                                                               |
| `PropertyValueElement.empty`, `Filter.is-empty`                | 비어있음                                                                      | Empty, is empty                                                                          | `DateRange.empty`는 "비어 있음"이다                                                                            |
| `BoardPage.newVersion`                                         | 새 버전의 보드가 존재합니다, 여기를 눌러 다시 불러오세요.                     | A new version of Boards is available, click here to reload.                              | 제품 이름 "Boards"를 "보드"로 옮겼다                                                                           |

## 측정

같은 기기에서 `cd apps/web && pnpm run i18n && pnpm exec vite build`로 쟀다. "전"은 커밋 `1faf53c0`(로케일 2개, 메시지 8개), "후"는 이 변환(로케일 18개, `en` 390개)이다.

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
- **메시지를 다 쓰게 되면.** 메시지 모듈 하나에 그 메시지의 모든 로케일 문구가 들어간다. 메시지 셋을 더 쓴 빌드는 33,849바이트였다(+2,081, br 12,200). `messages/*.json`의 합은 334,589바이트이고 `en`만은 21,950바이트다. 화면이 메시지를 다 쓰면 18개 로케일의 문구 대부분이 한 번들에 들어간다. paraglide에는 로케일별로 빌드를 나누는 옵션(`experimentalStaticLocale`)이 있다. 써 보지는 않았다.
- **CSS가 180바이트 늘어난 까닭.** 메시지와는 상관없다. Tailwind가 `tools/`도 클래스 후보로 훑는데, `tools/i18n/inlang.ts`에 있는 낱말 `ordinal`이 Tailwind의 `ordinal` 유틸리티로 잡혀 규칙 하나가 생겼다. 테스트 파일을 쓰기 전에 `src/styles/app.css`에 `@source not '../../tools/i18n';`을 넣어 빌드하니 95,162바이트로 돌아왔다(2026-11-10 18:44 KST). 지금은 `tests/tools/i18n.test.ts`에도 같은 낱말이 있어 그 한 줄로는 95,342바이트 그대로다. `tools`와 `tests`를 통째로 빼면 94,852바이트다(2026-11-10 18:54 KST). 기준 커밋보다도 310바이트 작으니, 기준에서도 이 두 폴더의 낱말이 규칙을 만들고 있었다는 뜻이다. 확인한 뒤 되돌렸고 `app.css`는 고치지 않았다.
- `svelte-check`가 보는 파일은 2,736개다(컴파일 결과 포함).

## 포팅할 때 알아둘 것

- **요소를 인자로 넘기던 메시지.** 원본은 `ViewHeader.group-by`와 `ViewHeader.display-by`의 `{property}`에 글자가 아니라 `<span>` 요소를 넘겨 색을 입힌다. paraglide의 인자는 문자열로 들어간다. 그 부분만 달리 그리려면 그 컴포넌트를 옮길 때 문구를 마크업(`{#name}…{/name}`)으로 바꿔야 한다. 변환은 문구를 그대로 뒀다.
- **값이 없는 인자.** react-intl은 `undefined`인 인자를 빈 문자열로 넣고 paraglide는 `undefined`라는 글자를 넣는다. `centerPanel.undefined`는 원본이 `groupByProperty?.name`을 넘긴다.
- **select의 입력.** 컴파일된 메시지는 값을 `===`로 비교한다. ICU는 문자열로 바꿔 비교한다. 불리언을 넘기던 호출은 문자열로 넘겨야 한다. 지금 자료에는 select가 없다.
- **번역 파일에 없는 id.** 원본 코드에는 `en.json`에 없는 id가 있다. 테마 이름 4개(`Sidebar.default-theme`, `dark-theme`, `light-theme`, `system-theme`: 코드의 `displayName`이 문구다)와 시험용 메뉴 4개(`ViewHeader.test-*`)다. 변환 대상이 아니다. 테마 메뉴를 옮길 때 문구를 `messages-own`에 새로 넣어야 한다.
- **`en.json`과 코드의 기본 문구가 다른 곳이 있다.** 예: `CardDetail.limited-body`. 원본은 영어에서도 `en.json`을 쓰므로 `en.json`을 따랐다. 남긴 382개 전체를 코드의 기본 문구와 대조하지는 않았다.

## 확인하지 못한 것

- **원본 파서와의 일치.** `tools/i18n/icu.ts`는 원본이 쓰는 파서의 규칙(아포스트로피, 태그, `#`, 오류 조건)을 그 소스에 대한 기억으로 옮긴 것이다. 그 라이브러리는 설치돼 있지 않아 같은 입력으로 나란히 돌려 보지 못했다. 실제 자료에는 글자, 글자 아포스트로피, 단순 인자, `<b>` 태그만 있으므로 차이가 날 수 있는 곳은 이 넷의 처리뿐이다.
- **plural·select 계열의 변환.** 실제 자료가 없어 테스트의 문구로만 확인했다. paraglide로 컴파일해 `en`과 `ru`에서 결과를 맞춰 봤다(one·few·many·other, `=0`, `offset`, 서수, 중첩).
- **변환하지 않은 3개가 원본에서 영어로 나왔는지.** react-intl의 동작에 대한 이해이고 원본을 실행해 보지 않았다.
- **죽은 id의 판정.** `webapp/src`에서 문자열 리터럴로 찾은 결과다. 이 저장소에 없는 코드가 쓰던 id는 알 수 없다.
- **쓰이지만 도달할 수 없는 코드.** 컴포넌트가 `import`는 되지만 실제로는 그려지지 않는 경우까지 따지지는 않았다. `Sidebar.product-tour`는 눈에 띄어 위에 적었다.
- **한국어가 원본에서 빠진 까닭.** `ko.json`은 461개 가운데 437개가 있는데 번들 목록에 없다. 이유는 코드에서 알 수 없었다.
- **채운 한국어 번역.** 내가 옮긴 것이고 다른 사람이 검토하지 않았다.
- **다른 로케일의 번역 품질.** 구조(인자, 태그, 구문)만 봤고 문구는 읽지 않았다. 이름 치환 결과 13개만 눈으로 봤다.
- **메시지를 많이 쓸 때의 트리 셰이킹.** 메시지 2개와 5개를 쓴 빌드에서만 확인했다.
- **`check`의 소요 시간 변화.** 전후를 재지 않았다.

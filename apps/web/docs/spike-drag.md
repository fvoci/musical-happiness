# 스파이크: 포인터 이벤트로 만든 칸반 드래그

작성: 2026-11-10 19:08 KST (기준 커밋 `1faf53c0`).
갱신: 2026-11-10 20:47 KST (기준 커밋 `ce3ba0bd`). 제품 책임자의 결정 열한 가지를 반영했고, 열 끌기·여러 장 옮기기·목록이 아닌 놓을 곳을 기반이 받도록 넓혔다. 처음 판의 "제품 책임자에게 묻는 것"은 아래 "결정의 기록"이 되었다.

드래그 라이브러리와 HTML5 드래그 앤 드롭 API 없이, Pointer Events 위에 칸반의 드래그를 직접 만든 결과다. 화면은 `spikes/drag/`에 있고 서버·API·라우터·상태 라이브러리를 쓰지 않는다. 데이터는 메모리에 만든 가짜다.

## 요약

- 마우스, 터치(길게 누르기), 키보드로 **카드**를 다른 열의 원하는 자리로 옮기고, **열 자체**를 머리글로 잡아 옮기고, 가장자리에서 자동 스크롤하고, 취소하면 원래대로 돌아가는 것까지 Chromium에서 동작한다.
- 이번에 더한 것: 보드 밖에 놓으면 취소, 정렬이 걸린 보기(열만 고르고 자리는 정렬이 정한다, 안내가 나온다), 길게 누르기가 성립할 때의 진동, 조정 값을 한 모듈에 모으기, 한 루트 안의 두 단계 정렬(범위), 여러 장을 한 번에 나르기, 목록이 아닌 놓을 곳(zone).
- 테스트: 순수 함수의 단위 테스트 129건(전 55건), 흉내 낸 문서(jsdom)에서의 `Sortable` 테스트 25건(새로), 브라우저 테스트 94건 통과·50건 건너뜀(전 30건 통과·18건 건너뜀).
- 판정은 DOM을 모르는 순수 함수(`src/lib/logic/drop-target.ts`)에 있고, 입력을 다루는 부품은 `src/lib/drag/`에 있다: 조정 값(`settings.ts`), 포인터 한 번의 드래그(`pointer-drag.ts`), 자동 스크롤(`auto-scroll.ts`), 목록 사이의 정렬(`sortable.svelte.ts`).
- 드래그 코드만의 크기는 축소(minify)한 뒤 16.0 kB, gzip으로 5.7 kB다(전 10.1 kB, 3.7 kB. `sortable.svelte.ts`와 그것이 가져오는 것을 esbuild로 묶어 잰 값이고 Svelte 런타임은 빠져 있다. 두 값을 같은 스크립트로 쟀다).
- `Sortable`을 "일반 컨트롤러 + 목록 전략"으로 나누지는 않았다. 이번 요구는 모두 "목록과 그 곁의 놓을 곳"이라는 같은 모양 안에 들어와서, 나눌 경계를 여전히 추측으로 그어야 했다. 대신 이번에 더한 판정을 모두 순수 함수로 내렸으므로, 나눌 때 옮길 것은 프레임 루프·끌리는 모습·취소·초점·안내뿐이다. 카드 내용 블록과 달력은 전과 같이 `Sortable`에 맞지 않는다.
- 실제 터치 기기, Safari, Firefox, 화면 낭독기로는 여전히 확인하지 못했다. 이 기기에는 Chromium(headless shell)만 있다. 사람이 손으로 해 볼 것은 "사람이 해 볼 것" 절에 적었다.

## 결정의 기록

제품 책임자가 처음 판의 질문 1–11에 답한 것과, 그 답이 지금 어디까지 들어갔는가. 상태는 셋이다. **구현**: 이번에 만들었고 스파이크 화면에서 해 볼 수 있다. **기반**: 부품이 받게 만들었고 단위 테스트가 있으며, 제품 화면은 W3에서 만든다(스파이크 화면에는 가장 단순한 데모만 있다). **유지**: 처음 판 그대로 두기로 했다.

| #   | 질문                        | 결정                                                                 | 상태 | 고정하는 테스트                                                                                                                                                              |
| --- | --------------------------- | -------------------------------------------------------------------- | ---- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | 터치의 시작 방법            | 길게 누르기 300ms 유지. 값은 설정 한 곳에                            | 구현 | `drag-settings.test.ts`(3건), 기존 터치 테스트 7건                                                                                                                           |
| 2   | 열 자체를 끌 수 있어야 하나 | 끌 수 있어야 한다                                                    | 구현 | `drag-columns.spec.ts`(테스트 20개), `tests/ui/sortable.test.ts`의 "two sortables on one root"(7건)                                                                          |
| 3   | 키보드의 키                 | 지금대로(Space 집기, Enter 열기)                                     | 유지 | 기존 키보드 테스트 7건                                                                                                                                                       |
| 4   | 보드 밖에 놓으면            | 취소                                                                 | 구현 | `dropRegion`(12건), `drag-decisions.spec.ts`의 "outside the board"(4건), `drag-columns.spec.ts`의 "a column let go outside the board is not moved", jsdom "outside the root" |
| 5   | 미리보기의 모양             | 지금대로(빈자리가 따라다님)                                          | 유지 | 기존 테스트                                                                                                                                                                  |
| 6   | 정렬이 걸린 보기            | 열만 바꾸고 자리는 무시. "정렬 중이라 자리가 정해진다"는 안내를 보임 | 구현 | `gapFor`(8건), `keyboardDestination`의 "a view whose lists place items themselves"(3건), `dropChanges`, `drag-decisions.spec.ts`의 "a sorted view"(6건), jsdom 5건           |
| 7   | 여러 장을 한꺼번에          | 필요하다. 기반이 막지 않게, 화면은 W3                                | 기반 | `moveItems`(11건), `carriedItems`(4건), `locateItem`(3건), `dropChanges`, jsdom "a selection"(4건), `drag-foundation.spec.ts`의 "several cards at once"(6건)                 |
| 8   | 취소 때 스크롤 되돌리기     | 지금대로                                                             | 유지 | 기존 "Escape cancels…", 새 "letting go outside cancels…", "Escape puts the column back, and the board where it was"                                                          |
| 9   | 자동 스크롤의 빠르기        | 지금 값 유지(가장자리 56px, 최고 초당 900px). 설정 한 곳에           | 구현 | `drag-settings.test.ts`                                                                                                                                                      |
| 10  | 숨긴 열에 놓기              | 필요하다. 화면은 W3                                                  | 기반 | `dropRegion`, `keyboardDestination`, `gapFor`, `dropChanges`, jsdom "a zone"(4건), `drag-foundation.spec.ts`의 "a hidden column to drop on"(7건)                             |
| 11  | 진동 피드백                 | 길게 누르기가 성립할 때 짧은 진동                                    | 구현 | `drag-decisions.spec.ts`의 "the buzz of a long press"(5건)                                                                                                                   |

질문 5의 뒷부분(놓을 때의 애니메이션)과 10의 앞부분(접힌 열)은 결정에 들어 있지 않아 손대지 않았다. 접힌 열은 숨긴 열과 같은 길(zone)로 받을 수 있다.

결정을 구현하며 내가 정한 것(되돌리기 쉬운 것들이다):

- **보드 밖의 경계.** "보드"는 `Sortable`이 붙은 루트이고, 그 **보이는 영역**(루트의 사각형과 창이 겹치는 부분)의 가장자리까지가 안이다. 그 밖에서 놓으면 취소다. 여유 폭은 두지 않았다.
- **밖에 있는 동안의 모습.** 빈자리가 원래 자리로 돌아가고, `active.outside`가 참이 되고, `notice`가 "놓으면 취소된다"는 문구를 낸다. 스파이크 화면은 끌리는 카드를 반투명하게 하고 문구를 화면 위쪽에 띄운다.
- **가장자리 스크롤과의 관계.** 밖에서는 아무것도 스크롤하지 않는다. 자세한 규칙은 "자동 스크롤".
- **정렬된 보기에서 빈자리가 서는 곳.** 호출부가 준 "정렬상의 자리"(없으면 목록의 끝). 같은 열에서는 원래 자리.
- **진동의 길이.** 15ms(`DRAG_SETTINGS.touchVibration`). 근거 없이 고른 값이다. 실제 기기에서 정해야 한다.
- **키보드로 놓을 곳에 가는 길.** 마지막 목록의 다음. "키보드와 안내" 절.

## 만든 것

| 파일                                           | 줄  | 내용                                                                                   |
| ---------------------------------------------- | --- | -------------------------------------------------------------------------------------- |
| `src/lib/logic/drop-target.ts`                 | 547 | 순수 함수: 놓일 자리, 보드 밖·놓을 곳의 판정, 여러 장의 이동, 키보드 이동, 스크롤 세기 |
| `src/lib/drag/settings.ts`                     | 31  | 조정 값의 기본값. 드래그의 느낌을 바꾸려면 여기만 고친다                               |
| `src/lib/drag/pointer-drag.ts`                 | 298 | 누름에서 뗌까지 포인터 한 번의 드래그. 길게 누르기가 성립하면 진동                     |
| `src/lib/drag/auto-scroll.ts`                  | 92  | 한 프레임의 자동 스크롤과, 취소 때 되돌릴 스크롤 위치의 기억                           |
| `src/lib/drag/sortable.svelte.ts`              | 714 | 목록 안·목록 사이의 정렬. 포인터와 키보드, 범위, 화면이 그릴 상태(`$state`)            |
| `tests/logic/drop-target.test.ts`              | 435 | 처음 판의 단위 테스트 55건(고치지 않았다)                                              |
| `tests/logic/drop-target-destinations.test.ts` | 626 | 이번에 더한 순수 함수의 단위 테스트 71건                                               |
| `tests/logic/drag-settings.test.ts`            | 39  | 조정 값이 한 곳에서 나온다는 것 3건                                                    |
| `tests/ui/sortable.test.ts`                    | 574 | 흉내 낸 문서에서의 `Sortable` 25건: 범위, 밖, 정렬된 보기, 놓을 곳, 여러 장            |
| `spikes/drag/Board.svelte`                     | 382 | 보드: 두 `Sortable`(카드, 열), 스위치 셋, 끌리는 모습, 안내                            |
| `spikes/drag/Column.svelte`                    | 161 | 열: 머리글(열의 손잡이)과 카드 목록, 빈자리                                            |
| `spikes/drag/CardFace.svelte`                  | 43  | 카드의 생김새, 여러 장일 때의 장수 표시                                                |
| `spikes/drag/data.ts`, `messages.ts`           |     | 가짜 데이터(6열, 열마다 12·30·6·18·9·24장), 주소의 옵션, 문구(영어 상수)               |
| `spikes/drag/support.ts`                       | 209 | 브라우저 테스트가 함께 쓰는 것: 위치 재기, 마우스, 손가락                              |
| `spikes/drag/drag.spec.ts`                     | 464 | 처음 판의 브라우저 테스트(카드). 본문은 그대로이고 도우미만 `support.ts`로 옮겼다      |
| `spikes/drag/drag-decisions.spec.ts`           | 406 | 보드 밖, 정렬된 보기, 진동                                                             |
| `spikes/drag/drag-columns.spec.ts`             | 491 | 열 끌기(마우스, 터치, 키보드, 오른쪽에서 왼쪽으로 쓰는 화면)                           |
| `spikes/drag/drag-foundation.spec.ts`          | 321 | 여러 장, 숨긴 열(놓을 곳)                                                              |
| `spikes/drag/measure.spec.ts`                  | 125 | 프레임 시간 측정. `FVOCI_SPIKE_MEASURE`를 주지 않으면 아무것도 재지 않는다             |

의존성은 추가하지 않았다. `package.json`은 건드리지 않았다.

실행:

```bash
pnpm --filter web run spikes                                   # http://127.0.0.1:5173/spikes/drag/
cd apps/web && FVOCI_SPIKES_PORT=18331 pnpm run spikes:e2e     # 브라우저 테스트
```

주소에 줄 수 있는 것(화면의 스위치와 같다. 테스트와, 스위치를 누르기 어려울 때를 위해): `?sizes=3,0,500`(열마다의 카드 수), `?sorted=1`(정렬된 보기), `?select=1`(카드 고르기), `?zone=1`(숨긴 열), `?dir=rtl`(오른쪽에서 왼쪽으로).

## 사람이 해 볼 것

화면 위쪽의 "What to try"에 같은 것이 짧게 적혀 있다(접을 수 있다). 360px 폭에서도 쓸 수 있게 했다(360×640과 360×700에서 화면을 찍어 확인했다).

1. **카드 끌기.** 마우스로, 또는 손가락으로 길게 눌러(300ms). 다른 열로, 같은 열 안에서, 화면 밖의 열로(가장자리에 대고 있으면 스크롤된다).
2. **진동.** Android에서 길게 누르기가 성립하는 순간 짧게(15ms) 떤다. 약하거나 없으면 값을 올려 본다(`src/lib/drag/settings.ts`의 `touchVibration`). 페이지를 연 뒤 한 번도 누른 적이 없으면 첫 번째에는 떨지 않을 수 있다("확인하지 못한 것" 참고). iOS Safari는 떨지 않는다.
3. **열 끌기.** 열의 머리글을 잡는다(마우스는 바로, 손가락은 길게 눌러). 머리글 위를 쓸면 보드가 스크롤된다.
4. **보드 밖에 놓기.** 끌다가 위쪽의 머리 부분이나 아래쪽의 줄로 가져가면 "Let go to cancel."이 뜨고 빈자리가 원래 자리로 돌아간다. 거기서 놓으면 아무 일도 없다. 도로 들어오면 이어서 끈다.
5. **정렬된 보기**(Sorted view). 켜면 열마다 카드가 일의 이름순으로 선다. 카드를 끌면 문구가 뜨고, 다른 열 위 어디에 가져가든 빈자리는 정렬상의 자리에 선다. 같은 열 안에서는 움직이지 않는다.
6. **카드 고르기**(Select cards). 켜고 카드 몇 장을 누른 뒤 그 가운데 하나를 끈다. 끌리는 카드에 장수가 붙는다.
7. **숨긴 열**(Hidden column). 켜면 보드의 맨 끝에 놓을 곳이 생긴다. 거기에 카드를 놓으면 카드가 보드에서 사라지고 수가 올라간다. "Put them back"으로 되돌린다.
8. **키보드.** 카드나 열 머리글에 초점을 두고 Space, 화살표, Space/Enter, Escape.

손으로 봐야 판단할 수 있는 것: 300ms와 진동의 길이, 자동 스크롤의 빠르기, 보드 밖의 경계에 여유가 필요한가(특히 가장자리로 스크롤하려다 밖으로 나가는 일이 잦은가), 끌리는 열이 휴대폰에서 화면을 다 가리는 것이 괜찮은가, 정렬된 보기의 문구가 손가락에 가려지지 않는가.

## 동작 방식

### 쓰는 이벤트

| 이벤트                                      | 어디에                           | 무엇을 위해                                                                               |
| ------------------------------------------- | -------------------------------- | ----------------------------------------------------------------------------------------- |
| `pointerdown`                               | 보드(루트)                       | 누른 곳의 항목을 찾아 "드래그가 될 수 있는 누름"으로 기억한다                             |
| `pointermove`, `pointerup`, `pointercancel` | `window`, 캡처 단계, 누른 동안만 | 따라가기, 놓기, 취소                                                                      |
| `lostpointercapture`                        | 보드                             | 캡처를 잃으면 취소                                                                        |
| `keydown`                                   | 보드 / `window`(캡처)            | Space로 집기 / 드래그·운반 중의 화살표, Space, Enter, Escape, Tab                         |
| `touchmove` (`passive: false`)              | 보드, 그리고 손가락이 닿은 요소  | 손가락으로 끄는 동안 브라우저의 스크롤을 막는다. Pointer Events가 아닌 유일한 입력 이벤트 |
| `contextmenu`, `selectstart`, `dragstart`   | 보드 / `document`                | 길게 누르기의 메뉴, 글자 선택, 링크·이미지의 브라우저 자체 드래그를 막는다                |
| `click`                                     | `window`, 캡처, 한 번            | 드래그 끝의 버튼 뗌이 클릭이 되지 않게 삼킨다                                             |
| `scroll`                                    | 보드, 캡처, 드래그 동안만        | 휠 등으로 스크롤되면 놓일 곳을 다시 계산한다                                              |

이벤트가 아닌 것 하나: 길게 누르기가 성립하는 순간 `navigator.vibrate`를 부른다.

드래그가 시작되면 보드가 `setPointerCapture`로 포인터를 잡는다. 항목이 아니라 보드가 잡는 까닭은 드래그 중에 항목 요소가 문서에서 빠질 수 있어서다. 캡처는 포인터가 창 밖으로 나가도 이벤트가 계속 오게 하려는 것이기도 한데, 그 경우는 테스트하지 않았다(창 밖은 이제 "보드 밖"이므로 거기서 놓으면 취소다).

스파이크 화면처럼 한 루트에 `Sortable` 둘을 붙이면 위의 리스너가 인스턴스마다 한 벌씩 붙는다. 누름 하나는 한 인스턴스만 받는다("두 단계 정렬").

### 조정 값은 한 곳에

`src/lib/drag/settings.ts`의 `DRAG_SETTINGS`가 기본값을 모두 가진다. 다른 파일에는 숫자가 없다.

| 이름             | 값  | 뜻                                                                |
| ---------------- | --- | ----------------------------------------------------------------- |
| `mouseDistance`  | 4   | 마우스가 버튼을 누른 채 이만큼(px) 움직이면 드래그가 시작된다     |
| `touchDelay`     | 300 | 손가락이 이만큼(ms) 머무르면 드래그가 시작된다                    |
| `touchTolerance` | 8   | 머무르는 손가락이 이만큼(px)까지는 움직여도 된다                  |
| `touchVibration` | 15  | 길게 누르기가 성립할 때 떠는 시간(ms). 0이면 떨지 않는다          |
| `scrollEdge`     | 56  | 스크롤 상자의 가장자리에서 이만큼(px) 안이 스크롤을 일으키는 띠다 |
| `scrollMaxSpeed` | 900 | 가장자리 그 자체에서의 스크롤 속도(px/초)                         |
| `longestFrame`   | 50  | 자동 스크롤이 한 프레임으로 쳐 주는 가장 긴 시간(ms)              |

호출부가 덮는 길은 그대로다: `Sortable`의 `pointer`·`scroll` 옵션, `pointerDrag`와 `AutoScroller`의 옵션. `longestFrame`만은 덮는 옵션이 없다.

### 드래그의 시작

- **마우스.** 버튼(주 버튼만)을 누른 채 4px 넘게 움직이면 시작한다. 움직이지 않고 떼면 평범한 클릭이다.
- **터치.** 항목 위에 손가락을 300ms 동안 8px 안에서 머무르면 시작하고, 그 순간 기기가 15ms 떤다. 그 전에 8px 넘게 움직이면 누름을 버리고 브라우저가 평소처럼 스크롤한다. 펜도 터치와 같이 다룬다(진동도).
- **키보드.** 초점이 있는 항목에서, 또는 항목의 손잡이에서 Space.

진동은 `navigator.vibrate`가 있을 때만 부르고, 부르다 오류가 나도 삼킨다. iOS Safari에는 이 함수가 없다. 마우스와 키보드에서는 부르지 않는다. 테스트는 `navigator.vibrate`를 바꿔치기해 한 번, 설정의 길이로 불리는 것을 본다. 함수를 지운 브라우저와 오류를 던지는 브라우저에서도 드래그가 되는 것을 본다.

터치를 길게 누르기로 정한 까닭(처음 판 그대로다):

1. 휴대폰 화면에서는 카드가 보드의 거의 전부를 덮는다. 카드 위에서 시작한 손가락 움직임이 스크롤이 되지 않으면 보드를 넘길 수 없다. 그래서 카드에 `touch-action: none`을 줄 수 없다.
2. `touch-action`은 손가락이 닿는 순간에 읽히고 그 뒤에는 바꿀 수 없다. "닿은 뒤에 드래그로 정해졌으니 이제 스크롤하지 마라"를 Pointer Events만으로는 말할 수 없어서, 드래그가 시작된 뒤의 `touchmove`를 `preventDefault` 한다. 브라우저는 손가락이 닿을 때 취소 가능한 리스너가 있는지를 정하므로 이 리스너는 보드에 미리 붙여 둔다.
3. 손잡이 방식(항목의 한 부분에만 `touch-action: none`)은 Pointer Events만으로 되고 더 확실하지만, 카드마다 44px짜리 손잡이 자리가 필요하고 마우스(카드 어디든 잡는다)와 방식이 달라진다.

길게 누르기의 비용: 300ms의 기다림, 방법이 눈에 보이지 않는다는 점(진동이 조금 덜어 준다), OS의 길게 누르기(문맥 메뉴, 글자 선택, iOS의 미리보기)와 겹친다는 점. 겹침은 항목의 `user-select: none`, `-webkit-touch-callout: none`, `contextmenu` 취소로 막았지만 실제 기기에서 확인하지 못했다. 또 보드에 취소 가능한 `touchmove` 리스너가 있으므로, 브라우저는 보드 위에서 스크롤을 시작하기 전에 메인 스레드의 답을 기다린다.

### 그리는 방법

`Sortable`은 DOM을 옮기지 않는다. 데이터도 놓을 때까지 바꾸지 않는다. 지금 무엇이 어디로 가는 중인지를 `active`와 `overlay`(끌리는 항목의 왼쪽 위 좌표)로 알려 주고, 화면이 그것을 그린다. `active`에서 화면이 읽는 것:

- `itemId`(잡은 것)와 `itemIds`(함께 가는 것 전부). 화면은 `itemIds`를 모두 제자리에 `display: none`으로 남긴다. 문서에서 빼지 않는 까닭: 터치 이벤트는 손가락이 처음 닿은 요소로 계속 전달되는데, 그 요소가 문서에서 빠지면 이벤트가 보드까지 올라오지 않아 스크롤을 막을 수 없게 된다.
- `gap`: **빈자리**를 그릴 곳(목록과 인덱스). 자유로운 보기에서는 포인터가 가리키는 자리, 정렬된 보기에서는 정렬상의 자리, 보드 밖에서는 원래 자리다. 놓을 곳(zone) 위에서는 `undefined`이고 빈자리가 없다.
- `to`: **지금 놓으면 일어날 일**. `{kind: 'index', listId, index}`, `{kind: 'list', listId}`, `{kind: 'zone', zoneId}` 가운데 하나이고, 보드 밖에서는 `undefined`(취소)다.
- `outside`: 포인터가 보드 밖에 있다. `placement`: 이 드래그가 자유로운 보기(`free`)의 것인지 정렬된 보기(`list`)의 것인지.
- `width`, `height`: 집을 때의 크기. 빈자리와 끌리는 모습에 쓴다.

그리고 `notice`: 보이는 글로 띄울 문구(보드 밖일 때와 정렬된 보기일 때만 값이 있다).

**끌리는 모습**은 `position: fixed`인 겹침층에 같은 컴포넌트를 한 번 더 그린 것이고 `pointermove`마다 `translate`만 바뀐다. `pointer-events: none`이다. 여러 장이면 잡은 카드 한 장에 장수를 붙인다. 열은 머리글과 앞쪽 카드 열두 장까지를 집을 때의 높이로 잘라 그린다.

**키보드로 나르는 동안**에는 끌리는 모습이 없다. 카드는 빈자리에 점선 상자 대신 카드 자체(테두리로 강조, `data-drag-carried`)를 그리고 초점을 거기에 둔다. 놓을 곳 위에 있으면 그 대역을 놓을 곳 안에 그린다. 열은 다르게 했다: 대역을 따로 그리지 않고 열 자체를 놓일 자리에 세운다(열의 순서를 미리 바꿔 그린다). 초점은 그 열의 머리글에 머문다.

놓을 때의 애니메이션은 없다.

### 자동 스크롤

드래그 동안 `requestAnimationFrame` 루프가 돈다. 프레임마다:

1. 포인터가 움직였거나 무언가 스크롤됐으면 놓일 곳을 다시 계산한다.
2. 지금 놓일 목록(또는 놓을 곳)에서 보드까지 올라가며 `data-drag-scroll="x|y"`가 붙은 상자를 모은다(열의 본문은 `y`, 보드는 `x`).
3. 상자마다 포인터가 가장자리 56px 안에 있으면 스크롤한다. 속도는 가장자리에 가까울수록 빨라져 가장자리에서 초당 900px다(`edgeScrollFactor`). 프레임 사이의 시간으로 거리를 정하고, 한 프레임에 1px이 안 되는 분량은 모아 두었다가 쓴다.
4. 스크롤됐으면 놓일 곳을 한 번 더 계산한다. 포인터가 가만히 있어도 그 밑의 항목이 바뀌기 때문이다.

**보드 밖과의 관계(이번에 정한 규칙).**

- 포인터가 보드의 보이는 영역 **밖**에 있으면 아무 상자도 스크롤하지 않는다. 밖은 취소 영역이고, 취소될 드래그가 보드를 계속 넘기는 것은 맞지 않다. 그래서 보드 자신은 **안쪽 가장자리 띠에서만** 스크롤하고, 가장자리 위가 최고 속도다. 처음 판은 "가장자리와 그 바깥"에서 스크롤했다.
- 포인터가 보드 **안**에 있는 동안, 안쪽 상자(열의 본문)는 전과 같이 자기 띠와 **자기 바깥**에서 스크롤한다. 예: 포인터가 열의 머리글 높이에 있으면 그 열은 위로 최고 속도로 올라간다. 열의 본문 밖이라도 보드 안이면 취소 영역이 아니기 때문이다.
- 경계는 안에 든다(가장자리 위의 포인터는 안이다). 손가락은 화면 밖으로 나갈 수 없으므로 화면의 가장자리에 닿은 손가락은 안이다.

취소하면(보드 밖에 놓은 것도 취소다) 자동 스크롤이 건드린 상자들의 스크롤 위치를 드래그 전으로 되돌린다. 놓으면 그대로 둔다.

### 놓일 곳 계산

두 단계다. 먼저 `dropRegion(point, {bounds, zones, lists, axis})`이 포인터가 무엇 위에 있는지를 가린다.

1. 보드의 보이는 영역(`bounds`) 밖이면 `outside`. 아무리 가까운 목록이 있어도 그렇다.
2. 안이고 놓을 곳(zone)의 사각형 위에 있으면 그 놓을 곳. 놓을 곳은 포인터가 **그 위에 있을 때만** 맞는다(가장 가까운 것을 고르지 않는다). 겹치면 문서에서 앞선 것.
3. 그 밖에는 가장 가까운 목록(`nearestList`). 가로 거리가 가장 가까운 열, 같으면 세로 거리가 가까운 열이다.

목록이 골라졌을 때, 자유로운 보기에서는 `dropTarget`이 인덱스를 낸다: 포인터를 그 목록의 보이는 상자 안으로 끌어당기고, "가운데가 포인터보다 앞에 있는 항목의 수"를 센다(`insertionIndex`). 끌리는 것들은 세지 않는다. 정렬된 보기에서는 인덱스를 구하지 않는다(항목의 사각형도 읽지 않는다).

인덱스는 언제나 **끌리는 것들을 뺀 목록**에서의 자리다. 같은 목록 안의 이동과 목록 사이의 이동을, 한 장과 여러 장을 같은 숫자로 말하기 위해서다.

알아 둘 성질(처음 판 그대로): 계산은 **지금 화면에 놓인 대로**의 사각형으로 한다. 빈자리가 열리면 그 아래 항목들이 밀리므로, 같은 포인터 위치라도 빈자리가 어디에 있었는지에 따라 답이 한 칸 다를 수 있다. 빈자리가 열린 뒤에 답이 도로 뒤집히는 일은 없다(단위 테스트가 있다). 사용자는 빈자리를 보고 있으므로 놓이는 곳은 언제나 보이는 빈자리와 같다.

### 보드 밖에 놓으면 취소 (결정 4)

포인터가 보드의 보이는 영역 밖에 있는 동안 `active.to`는 `undefined`, `active.outside`는 참, 빈자리는 원래 자리, `notice`는 `messages.outside()`의 문구다. 거기서 놓으면 Escape와 같은 길로 간다: 데이터는 그대로, 자동 스크롤은 되돌리고, 취소 안내를 낸다. 버튼을 뗀 것이 클릭이 되지도 않는다. 도로 안으로 들어오면 드래그가 이어진다.

판정은 `dropRegion`(순수 함수, 단위 테스트 12건)이 하고, "보이는 영역"은 `intersectRect(루트의 사각형, 창)`이다. 루트가 창 밖으로 스크롤되어 보이는 부분이 없으면 어디든 밖이다.

터치: 손가락은 화면 밖으로 못 나가므로, 보드가 화면을 꽉 채우면 "밖"이 없다. 스파이크 화면은 위에 머리 부분, 아래에 한 줄이 있어 거기가 밖이다(터치 테스트가 위쪽으로 본다). 제품의 보드 화면에서 손가락으로 취소할 길이 따로 필요한지는 W3에서 정해야 한다(예: 끄는 동안만 나타나는 "취소" 놓을 곳. 기반은 zone으로 받는다).

### 정렬이 걸린 보기 (결정 6)

`placement: () => 'list'`를 주면(집을 때마다 묻는다) 그 드래그는 "자리는 목록이 정한다"는 방식이 된다.

- 놓일 곳은 **목록뿐**이다. `active.to`와 `onDrop`의 `to`는 `{kind: 'list', listId}`이고 `index`가 없다(타입에 없다).
- **같은 목록**에 놓으면 아무것도 바뀌지 않고 `onDrop`도 불리지 않는다.
- **빈자리**는 포인터를 따라다니지 않는다. 다른 목록 위에서는 호출부의 `sortedIndex(itemId, listId, carried)`가 준 자리(끌리는 것을 뺀 목록에서의 인덱스)에, 그 콜백이 없으면 목록의 끝에 선다. 원래 목록 위에서는 원래 자리다. 판정은 `gapFor`(순수 함수).
- **안내.** `messages.sorted()`의 문구가 `notice`로 나오고(화면이 보이는 글로 띄운다), 집는 순간 화면 낭독기 안내로도 나온다(포인터로 집으면 그 문구만, 키보드로 집으면 "집었다"는 안내 뒤에 붙여서).
- **키보드.** ←→만 열을 바꾼다. ↑↓, Home, End는 아무것도 움직이지 않고 그 문구를 다시 안내한다. 같은 문구를 연달아 내면 라이브 영역이 다시 읽지 않으므로, 연달아 같을 때는 끝에 보이지 않는 공백(U+00A0)을 붙였다 뗐다 한다.

스파이크 화면의 "Sorted view"는 일의 이름순(같으면 id순)이다. 화면이 보여 주는 순서와 `Sortable`의 `order`가 같아야 하므로 `order: () => shown`으로 정렬된 순서를 넘긴다. 놓으면 화면은 데이터의 그 열 끝에 카드를 붙이고, 보이는 자리는 정렬이 정한다.

### 두 단계 정렬: 열 끌기 (결정 2)

한 루트에 `Sortable`을 여럿 붙이고 인스턴스마다 `scope`를 준다.

- 인스턴스는 `data-drag-scope="<scope>"`가 붙은 목록·항목·놓을 곳·대역만 읽는다. `scope`가 없는 인스턴스는 그 속성이 **없는** 것만 읽는다.
- 항목은 "자기 범위의 가장 가까운 목록"에 속한다. 목록의 항목을 모을 때 그 안에 든 같은 범위의 다른 목록으로 내려가지 않는다(jsdom 테스트가 카드 안의 목록으로 본다).
- **누름은 그 밑의 가장 안쪽 항목의 것이다**(어느 범위의 항목이든). 그 항목이 자기 범위가 아니면 인스턴스는 그 누름을 받지 않는다. 그래서 카드를 누르면 카드의 인스턴스만, 열의 머리글을 누르면 열의 인스턴스만 드래그를 시작한다. 키보드의 Space도 같다.
- 손잡이(`data-drag-handle`)는 "가장 가까운 항목"의 것이다. 열 안의 카드에 손잡이가 있어도 열의 손잡이로 치지 않는다.
- 루트 자신이 목록이어도 된다.

스파이크 화면: 보드(루트)가 열들의 목록이다(`data-drag-list="board" data-drag-scope="column"`, `axis: 'x'`). 열(`<section>`)이 항목이고 머리글이 손잡이이자 키보드로 집는 곳이다. 카드 쪽은 처음 판 그대로 범위가 없다. 열을 끄는 동안 가로 자동 스크롤은 보드의 `data-drag-scroll="x"`가 받고, 열 본문의 세로 스크롤은 건드리지 않는다(놓일 목록에서 루트까지의 상자만 모으기 때문이다).

**오른쪽에서 왼쪽으로 쓰는 화면.** 가로로 놓인 항목들은 좌표가 줄어드는 순서로 선다. `insertionIndex`에 `reversed`를 더했고 `Sortable`은 축이 `x`이고 루트의 `direction`이 `rtl`이면 그것을 켠다. 키보드의 좌우는 처음 판부터 뒤집혀 있었다. Chromium에서 `?dir=rtl`로 열의 마우스·키보드, 카드의 마우스·키보드를 확인했다(브라우저 테스트 4건). RTL에서의 자동 스크롤과 터치는 보지 않았다.

### 여러 장 (결정 7)

- `selection: () => 지금 고른 항목들`을 주면, 잡은 항목이 그 안에 있을 때 **고른 것 전부**를 나른다. 잡은 항목이 그 안에 없으면 그것 하나만 나른다(고른 것은 그대로 둔다).
- 나르는 것은 `active.itemIds`와 `onDrop`의 `itemIds`에 **데이터의 순서**(목록 순, 그 안에서 앞에서 뒤로)로 실린다. 고른 순서와 무관하다. 장수는 `itemIds.length`.
- **한 장일 때와의 양립.** `itemIds`는 언제나 배열이고 한 장이면 길이가 1이다. `itemId`는 언제나 잡은 것이다. 호출부는 두 경우를 가르지 않고 `moveItems(order, move.itemIds, move.to)`를 부르면 된다(`moveItem`은 `moveItems`의 한 장짜리다).
- `moveItems(lists, itemIds, target)`: 고른 항목들을 있던 목록들에서 빼고, 데이터의 순서대로 한 덩어리로 목표 자리에 넣는다. `target.index`는 **옮기는 것들을 모두 뺀** 목표 목록에서의 자리다. 여러 목록에서 고른 경우, 목표 자리가 고른 것들 사이인 경우(떨어져 있던 것들이 붙는다), 이미 붙어 있는 것들을 제자리에 놓는 경우(변화 없음)를 단위 테스트한다.
- `from`은 잡은 항목의 자리이고, 함께 가는 것들을 빼고 센 인덱스다(빈자리가 처음 서는 곳이다).
- 안내 문구에는 장수(`place.items`)가 실린다.

고르는 UI(Ctrl·Shift 클릭, 범위 선택, 선택의 표시)는 W3이다. 스파이크 화면에는 스위치를 켜고 카드를 하나씩 눌러 고르는 가장 단순한 것만 있다.

알아 둘 것 하나: 잡은 카드의 **위쪽**에 있는 고른 카드가 숨겨지면 그만큼 목록이 위로 당겨져, 집자마자 포인터 밑의 카드가 바뀌고 빈자리가 한 칸 내려갈 수 있다. 한 장일 때는 빈자리가 카드의 높이를 그대로 채워서 생기지 않는 일이다. W3에서 여러 장의 미리보기를 정할 때 같이 봐야 한다(예: 빈자리를 숨긴 카드들의 높이만큼 열기).

### 놓을 곳: 목록이 아닌 목표 (결정 10)

`data-drag-zone="<id>"`가 붙은 요소가 놓을 곳이다. 숨긴 열, 접힌 열, 휴지통 같은 것을 받는다.

- 포인터가 그 위에 있으면 `active.to`가 `{kind: 'zone', zoneId}`가 되고 `active.gap`은 `undefined`다. 화면은 놓을 곳을 강조한다.
- 거기서 놓으면 `onDrop`의 `to`가 `{kind: 'zone', zoneId}`다. **인덱스가 없다.** 맨 위에 넣을지 맨 아래에 넣을지는 호출부가 정한다.
- 자동 스크롤은 놓을 곳에서 루트까지의 상자를 대상으로 한다(화면 밖의 놓을 곳으로 스크롤해 갈 수 있다. 터치 테스트가 있다).
- 범위는 목록·항목과 같이 `data-drag-scope`로 가른다.
- **키보드.** 포인터는 문서에서 놓을 곳을 찾지만, 키보드는 순서를 알아야 하므로 `zones: () => ['id', …]`로 받는다. 놓을 곳들은 **마지막 목록의 다음**에 그 순서로 선다: 마지막 목록에서 "다음 목록"으로 가는 키(세로 목록이면 →)를 누르면 첫 놓을 곳, 더 누르면 다음 놓을 곳, 반대 키로 되돌아온다. 놓을 곳에서 목록으로 돌아올 때는 마지막으로 있던 인덱스로 간다(그 목록이 짧으면 끝). 놓을 곳 위에서 ↑↓, Home, End는 아무 일도 하지 않는다. `zones`를 주지 않으면 키보드는 놓을 곳에 가지 못한다. 판정은 `keyboardDestination`(순수 함수).
- 키보드로 놓을 곳 위에 있는 동안 초점을 둘 대역(`data-drag-carried`)은 화면이 놓을 곳 안에 그린다.
- 놓은 뒤 항목이 루트에서 사라지면(숨긴 열로 간 카드) 초점을 돌려줄 곳이 없다. 어디에 초점을 둘지는 화면의 일이다.

스파이크 화면의 "Hidden column"은 보드의 맨 끝에 놓을 곳 하나를 둔다.

### 창 크기가 바뀌면 취소 (2026-11-10 20:58 KST)

플랜의 "화면 크기의 세 단계"가 정한 방침이다: 쓰는 도중에 화면 크기가 바뀌면(펼치기·접기, 화면 나누기, 회전) 진행 중인 드래그는 취소한다.

- **전에는**(2026-11-10 20:55 KST에 Playwright로 확인) 창 크기가 바뀌어도 드래그가 그대로 이어졌다. 끌리는 카드와 빈자리가 남고, 놓일 자리는 바뀌기 전의 포인터 좌표로 새 배치에서 다시 계산됐다. 626×890에서 890×626으로 돌린 실행에서는 포인터가 마침 보드의 보이는 영역 밖이 되어 "밖에 놓으면 취소"로 끝났는데, 그것은 좌표가 우연히 그렇게 떨어진 것이다.
- **지금은** `Sortable`이 붙어 있는 동안 창의 `resize`를 듣는다. 드래그(포인터든 키보드로 나르는 중이든)가 있는데 창의 **너비가 달라졌거나 높이가 120px 넘게 달라지면** Escape와 같은 길로 취소한다: 순서 그대로, 자동 스크롤 되돌림, 취소 안내, 키보드였다면 초점이 카드로 돌아간다. 판정은 순수 함수 `windowChanged`(`drop-target.ts`), 값은 `DRAG_SETTINGS.resizeTolerance`다.
- 높이에 여유를 둔 까닭: 휴대폰의 브라우저는 주소 막대를 밀어 넣고 빼면서 창의 높이를 바꾼다. 그것으로 드래그가 끊기면 안 된다. 120px은 근거 없이 고른 값이고 실제 기기에서 확인하지 못했다. 높이만 크게 바뀌는 경우(화면을 위아래로 나누기)는 취소한다.
- 드래그가 없을 때의 크기 변화는 아무것도 하지 않는다. 열의 스크롤 위치는 그대로 남는다(브라우저 테스트로 확인).
- 테스트: `windowChanged` 3건(`tests/logic/drop-target-destinations.test.ts`), 브라우저 `spikes/drag/drag-window.spec.ts`의 "the window changes size during a drag" 6개(마우스 3, 터치 1, 키보드 1, 드래그 없을 때 1).

### 시작 방식은 포인터 종류가 정한다 (확인, 2026-11-10 20:58 KST)

드래그의 시작 방식(마우스는 4px 움직임, 손가락·펜은 300ms 머무름)은 `pointer-drag.ts`가 누름마다 `PointerEvent.pointerType`으로 가른다. 화면 너비나 뷰포트를 보는 곳은 드래그 코드에 없다(`matchMedia`, `innerWidth`로 방식을 정하는 곳이 없다. `innerWidth`는 보이는 영역의 사각형을 구하는 데만 쓰고 부를 때마다 읽는다). 그래서 터치와 마우스를 함께 쓰는 태블릿에서는 누름마다 맞는 방식이 된다. 브라우저 테스트 둘이 고정한다(`drag-window.spec.ts`의 "how a drag begins follows the pointer, not the width of the window"): 휴대폰 너비(360×740)의 창에서 마우스는 기다림 없이 바로 끌고, 데스크톱 너비(1180×820)의 화면에서 손가락은 바로 움직이면 스크롤하고 머물러야 집는다.

### 취소

Escape, 보드 밖에 놓기, `pointercancel`, `lostpointercapture`, 그리고 코드에서 부르는 `sortable.cancel()`이 같은 길로 간다: 데이터는 애초에 바뀐 적이 없으므로 `active`를 지우고, 자동 스크롤을 되돌리고, 안내 문구를 낸다.

Escape로 취소했을 때는 버튼이나 손가락이 아직 눌려 있다. 그 누름의 나머지는 아무 일도 하지 않게 했다: 손가락은 떼기 전까지 스크롤하지 않고, 버튼을 떼도 클릭이 되지 않는다.

### 키보드와 안내

카드(세로 목록):

| 키                | 동작                                                                                    |
| ----------------- | --------------------------------------------------------------------------------------- |
| Space (카드에서)  | 집는다                                                                                  |
| ↑ ↓               | 같은 열에서 한 칸. 끝에서 멈춘다. 정렬된 보기에서는 움직이지 않고 안내를 다시 읽는다    |
| ← →               | 옆 열로. 인덱스를 유지하고, 그 열이 더 짧으면 맨 끝. 마지막 열의 다음은 놓을 곳(있으면) |
| Home, End         | 열의 맨 앞, 맨 끝. 정렬된 보기에서는 ↑↓와 같다                                          |
| Space, Enter      | 놓는다                                                                                  |
| Escape            | 취소하고 원래 자리로                                                                    |
| Tab               | 막는다. 놓거나 취소할 때까지 초점이 카드에 머문다                                       |
| Enter (집지 않고) | 카드를 연다(스파이크의 화면이 하는 일이고 `Sortable`의 일이 아니다)                     |

열(가로 목록 하나): 머리글에서 Space로 집고, ←→로 한 칸, Home·End로 맨 앞·맨 끝, Space·Enter로 놓고 Escape로 취소한다. ↑↓는 "옆 목록"이라 목록이 하나뿐인 여기서는 아무 일도 하지 않는다. 오른쪽에서 왼쪽으로 쓰는 화면에서는 ←→가 뒤집힌다(카드도 열도).

나르는 동안 마우스나 손가락으로 어디든 누르면 취소한다. 이동할 때마다 대역이 보이도록 `scrollIntoView({block: 'nearest', inline: 'nearest'})`를 부른다. 놓거나 취소한 뒤에는 초점이 집었던 곳(항목, 또는 손잡이에서 집었으면 손잡이)으로 돌아간다.

안내는 화면이 그리는 `aria-live="assertive"` 영역에 `sortable.announcement`를 넣는 방식이다. 문구는 `Sortable`에 넣지 않고 `messages` 옵션으로 받는다(스파이크에서는 `spikes/drag/messages.ts`의 영어 상수, 제품에서는 paraglide 메시지를 넘기면 된다). 집었을 때, 옮길 때마다, 놓았을 때, 취소했을 때 알린다. 포인터로 놓거나 취소했을 때도 알린다. 스파이크 화면은 카드와 열의 안내 영역을 따로 둔다.

## 공개 형태

### 순수 함수: `src/lib/logic/drop-target.ts`

```ts
interface Point { x: number; y: number }
interface Rect { left: number; top: number; width: number; height: number }
type Axis = 'x' | 'y'; // 목록이 항목을 쌓는 방향

interface ListBox { id: string; rect: Rect; items: readonly Rect[] } // 보이는 상자, 끌리는 것들을 뺀 항목들
interface ZoneBox { id: string; rect: Rect }
interface DropTarget { listId: string; index: number } // index: 끌리는 것들을 뺀 목록에서의 자리
interface ListOrder { id: string; items: readonly string[] } // 데이터로서의 순서

type Placement = 'free' | 'list'; // 자리를 사람이 정하는가, 목록(정렬)이 정하는가
type Destination = // 놓는다는 것의 뜻
  | { kind: 'index'; listId: string; index: number }
  | { kind: 'list'; listId: string } // 인덱스가 없다
  | { kind: 'zone'; zoneId: string }; // 인덱스가 없다

// 기하
clampToRect(point, rect): Point
containsPoint(rect, point): boolean // 가장자리는 안. 넓이 없는 사각형은 아무것도 담지 않는다
intersectRect(first, second): Rect // 겹치지 않으면 넓이 0
nearestList<L extends { rect: Rect }>(point, lists, axis = 'y'): L | undefined
insertionIndex(point, items: readonly Rect[], axis = 'y', reversed = false): number
dropTarget(point, lists: readonly ListBox[], options?: { axis?; bounds?; reversed? }): DropTarget | undefined
edgeScrollFactor(position, start, size, edge): number // -1 … 1

// 포인터가 무엇 위에 있는가
type DropRegion<L> = { kind: 'outside' } | { kind: 'zone'; zoneId: string } | { kind: 'list'; list: L };
dropRegion<L extends { rect: Rect }>(
  point,
  scene: { bounds?: Rect; zones?: readonly ZoneBox[]; lists: readonly L[]; axis?: Axis },
): DropRegion<L> | undefined

// 순서(데이터)
locateItem(lists, itemId, without: readonly string[] = []): DropTarget | undefined
carriedItems(lists, grabbedId, selected: readonly string[] = []): string[] // 데이터의 순서로
moveItems(lists, itemIds: readonly string[], target: DropTarget): ListOrder[] // 입력을 바꾸지 않는다
moveItem(lists, itemId, target: DropTarget): ListOrder[] // moveItems의 한 장짜리
dropChanges(lists, itemIds, destination: Destination): boolean // 놓으면 바뀌는 것이 있는가
sameTarget(first: DropTarget, second: DropTarget): boolean
sameDestination(first: Destination | undefined, second: Destination | undefined): boolean

// 빈자리가 설 곳
gapFor(
  destination: Destination | undefined, // undefined: 보드 밖
  scene: { from: DropTarget; lists; carried: readonly string[]; sortedIndex?: (listId) => number | undefined },
): DropTarget | undefined // undefined: 놓을 곳 위

// 키보드
type KeyboardStep = 'previous' | 'next' | 'first' | 'last' | 'previousList' | 'nextList';
keyboardTarget(current: DropTarget, step, lists, carried: string | readonly string[]): DropTarget
keyboardDestination(
  current: Destination,
  step: KeyboardStep,
  scene: { lists; zones?: readonly string[]; carried: readonly string[]; placement?: Placement; resumeIndex?: number },
): Destination
```

DOM 타입은 없다. `DOMRect`는 `Rect`의 모양을 갖추고 있어 그대로 넘길 수 있다. 좌표계는 호출하는 쪽이 하나로 맞추기만 하면 된다(`Sortable`은 뷰포트 좌표).

### 재사용 부품: `src/lib/drag/`

```ts
// settings.ts
const DRAG_SETTINGS: {
  mouseDistance: 4; touchDelay: 300; touchTolerance: 8; touchVibration: 15;
  scrollEdge: 56; scrollMaxSpeed: 900; longestFrame: 50;
};

// pointer-drag.ts — 프레임워크와 무관하다
interface PointerDragOptions { mouseDistance: number; touchDelay: number; touchTolerance: number; touchVibration: number }
interface PointerDragSession {
  start(point: Point): void; // 한 번
  move(point: Point): void;
  drop(point: Point): void; // start 뒤에 drop 과 cancel 가운데 정확히 하나
  cancel(): void;
}
type PointerDragSource = (event: PointerEvent) => PointerDragSession | undefined;
pointerDrag(root: HTMLElement, source: PointerDragSource, options?: Partial<PointerDragOptions>): { cancel(): void; destroy(): void }

// auto-scroll.ts — 프레임워크와 무관하다
interface AutoScrollOptions { edge: number; maxSpeed: number }
interface ScrollBox { element: HTMLElement; axis: Axis }
class AutoScroller {
  constructor(options?: Partial<AutoScrollOptions>);
  step(point: Point, boxes: readonly ScrollBox[], elapsed: number): boolean; // 움직였는가
  restore(): void; // 건드린 상자를 처음 위치로
  clear(): void;
}

// sortable.svelte.ts — Svelte 5 ($state)
interface SortableMove {
  itemId: string; // 잡은 것
  itemIds: readonly string[]; // 놓은 것 전부, 데이터의 순서. 한 장이면 길이 1
  from: DropTarget; // 잡은 것의 자리(함께 가는 것들을 빼고 센다)
  to: Destination; // kind 가 'list' 나 'zone' 이면 index 가 없다
}
interface SortableActive {
  itemId: string;
  itemIds: readonly string[];
  mode: 'pointer' | 'keyboard';
  placement: Placement;
  from: DropTarget;
  to: Destination | undefined; // undefined: 포인터가 보드 밖. 놓으면 취소
  gap: DropTarget | undefined; // 빈자리를 그릴 곳. undefined: 놓을 곳 위
  outside: boolean;
  width: number;
  height: number;
}
interface SortablePlace { item: string; items: number; list: string; position: number; count: number }
interface SortableZonePlace { item: string; items: number; zone: string }
interface SortableMessages {
  pickedUp(place: SortablePlace): string;
  moved(place: SortablePlace): string;
  dropped(place: SortablePlace): string;
  cancelled(place: SortablePlace): string;
  sorted?(): string; // placement 가 'list' 일 수 있으면 준다. 보이는 글과 낭독 둘 다
  outside?(): string; // 보드 밖에 있는 동안 보이는 글
  movedToZone?(place: SortableZonePlace): string; // 놓을 곳이 있으면 준다
  droppedInZone?(place: SortableZonePlace): string;
}
interface SortableOptions {
  order: () => readonly ListOrder[]; // 화면이 보여 주는 대로의 순서
  onDrop: (move: SortableMove) => void; // 바뀌는 것이 있는 놓기마다 한 번
  messages: SortableMessages;
  itemLabel?: (itemId: string) => string;
  listLabel?: (listId: string) => string;
  zoneLabel?: (zoneId: string) => string;
  axis?: Axis;
  scope?: string; // 한 루트의 여러 인스턴스를 가른다
  placement?: () => Placement; // 집을 때마다 묻는다. 기본 'free'
  sortedIndex?: (itemId: string, listId: string, carried: readonly string[]) => number | undefined;
  selection?: () => readonly string[]; // 지금 고른 항목들
  zones?: () => readonly string[]; // 키보드가 갈 수 있는 놓을 곳, 순서대로
  pointer?: Partial<PointerDragOptions>;
  scroll?: Partial<AutoScrollOptions>;
}
class Sortable {
  constructor(options: SortableOptions);
  readonly active: SortableActive | undefined; // 반응형
  readonly overlay: Point | undefined; // 반응형. 포인터로 끄는 항목의 왼쪽 위(뷰포트 좌표)
  readonly announcement: string; // 반응형. 라이브 영역에 넣는다
  readonly notice: string; // 반응형. 보이는 글로 띄운다. 없으면 ''
  readonly attach: (root: HTMLElement) => () => void; // {@attach sortable.attach}
  cancel(): void;
}
```

처음 판과 달라져 호출부가 고쳐야 하는 것: `active.to`가 `DropTarget`이 아니라 `Destination | undefined`가 되었다(빈자리는 `active.gap`으로 그린다). `onDrop`의 `to`도 `Destination`이다(`kind`를 보고 쓴다). `keyboardTarget`의 넷째 인자는 여전히 id 하나를 받는다.

화면이 지킬 약속(속성):

| 속성                       | 붙이는 곳                                                                                     |
| -------------------------- | --------------------------------------------------------------------------------------------- |
| `data-drag-list="<id>"`    | 목록의 보이는 상자(스크롤한다면 스크롤하는 그 요소). 루트 자신이어도 된다                     |
| `data-drag-item="<id>"`    | 목록 안의 항목. 항목이나 그 손잡이가 초점을 받을 수 있어야 한다                               |
| `data-drag-zone="<id>"`    | 목록이 아닌 놓을 곳                                                                           |
| `data-drag-scroll="x\|y"`  | 가장자리에서 스크롤할 상자. 루트이거나 루트와 목록(놓을 곳) 사이의 요소                       |
| `data-drag-handle`         | 항목 안에 있으면 그 부분에서만 포인터 드래그가 시작된다. 초점을 받으면 Space로 집는 곳도 된다 |
| `data-drag-carried`        | 키보드로 나르는 동안 화면이 그리는 대역. 초점을 받는다                                        |
| `data-drag-scope="<이름>"` | 목록·항목·놓을 곳·대역에. 어느 인스턴스의 것인지. 없으면 `scope` 없는 인스턴스의 것           |

끌리는 항목은 드래그 동안 문서에 남아 있어야 한다(숨겨도 된다). 항목 안의 `input`, `textarea`, `select`, `button`, `a[href]`, `contenteditable`에서 시작한 누름은 드래그가 되지 않는다.

스파이크 화면이 쓰는 모습(`spikes/drag/Board.svelte`에서 줄였다):

```svelte
const cards = new Sortable({
  order: () => shown,
  onDrop: ({ itemIds, to }) => {
    if (to.kind === 'index') order = moveItems(order, itemIds, to);
    else if (to.kind === 'list') order = moveItems(order, itemIds, { listId: to.listId, index: Infinity });
    else hide(itemIds); // to.kind === 'zone'
  },
  messages: MESSAGES,
  placement: () => (sorted ? 'list' : 'free'),
  sortedIndex: (itemId, listId, carried) => …,
  selection: () => (selecting ? selected : []),
  zones: () => (zoneShown ? ['icebox'] : []),
});
const columns = new Sortable({
  scope: 'column',
  axis: 'x',
  order: () => [{ id: 'board', items: order.map((list) => list.id) }],
  onDrop: …,
  messages: COLUMN_MESSAGES,
});
…
<div data-drag-list="board" data-drag-scope="column" data-drag-scroll="x"
     {@attach cards.attach} {@attach columns.attach}>
  <section data-drag-item={column.id} data-drag-scope="column">
    <div data-drag-handle tabindex="0">…머리글…</div>
    <div data-drag-list={column.id} data-drag-scroll="y">
      <div data-drag-item={card.id} tabindex="0">…</div>
    </div>
  </section>
  <aside data-drag-zone="icebox">…</aside>
</div>
{#if cards.notice || columns.notice}<div>…보이는 문구…</div>{/if}
<div aria-live="assertive" aria-atomic="true" class="sr-only">{cards.announcement}</div>
```

## 테스트

### 단위 테스트 (`tests/logic/`, 129건. 전 55건)

`drop-target.test.ts`(55건)는 처음 판 그대로다. 고치지 않았고 그대로 통과한다.

`drop-target-destinations.test.ts`(71건):

| 함수                          | 건수 | 보는 것                                                                                                                                                                                         |
| ----------------------------- | ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `containsPoint`               | 3    | 안과 가장자리, 가장자리를 막 넘은 곳, 넓이 없는 사각형                                                                                                                                          |
| `intersectRect`               | 3    | 안에 든 것, 삐져나온 것, 만나지 않는 것                                                                                                                                                         |
| `dropRegion`                  | 12   | 목록 위, 목록 사이(가장 가까운 것), 밖(네 방향), 경계는 안, 놓을 곳 위와 곁, 목록 위에 놓인 놓을 곳, 겹친 놓을 곳, 밖에 있는 놓을 곳, `bounds` 없음, 넓이 없는 `bounds`, 아무것도 없음, `x`축   |
| `insertionIndex`·`dropTarget` | 3    | 오른쪽에서 왼쪽으로 놓인 항목(`reversed`), 왼쪽에서 오른쪽과 거울상인지                                                                                                                         |
| `sameDestination`             | 3    |                                                                                                                                                                                                 |
| `locateItem`                  | 3    | 함께 가는 것들을 빼고 센 인덱스                                                                                                                                                                 |
| `carriedItems`                | 4    | 고른 것 없음, 잡은 것이 고른 것에 없음, 고른 것 전부(데이터의 순서), 목록에 없는 것                                                                                                             |
| `moveItems`                   | 11   | 한 목록의 여러 장, 여러 목록에서, 그 목록들 가운데 하나로, 옮기는 것을 뺀 인덱스, 고른 것들 사이의 자리, 제자리, 끝을 넘는 인덱스와 `Infinity`, 모르는 id, 중복, 입력 불변, `moveItem`과 같은지 |
| `dropChanges`                 | 7    | 제자리, 다른 자리, 붙어 있는 여러 장의 제자리, 떨어진 여러 장, 정렬된 목록(다른 목록에서 올 때만), 놓을 곳(언제나), 모르는 것                                                                   |
| `gapFor`                      | 8    | 자리, 밖(원래 자리), 놓을 곳(없음), 정렬상의 자리, 원래 목록(원래 자리), 콜백 없음(끝), 끝은 나르는 것을 빼고, 범위 밖의 인덱스                                                                 |
| `keyboardTarget`              | 2    | 여러 장을 나를 때의 끝, 걸음의 끝이 `moveItems`와 맞는지                                                                                                                                        |
| `keyboardDestination`         | 12   | 자유로운 보기 8(목록 안팎, 놓을 곳으로 가고 오기, 돌아올 인덱스, 놓을 곳 위의 ↑↓, 놓을 곳 없음, 모르는 id), 정렬된 보기 3(목록 안은 제자리, 목록 사이, 놓을 곳), 열 한 줄 1                     |

`drag-settings.test.ts`(3건): `DRAG_SETTINGS`의 값, `POINTER_DRAG_DEFAULTS`와 `AUTO_SCROLL_DEFAULTS`가 거기서 나온다는 것.

### 흉내 낸 문서에서의 테스트 (`tests/ui/sortable.test.ts`, 25건. 새로)

`Sortable`이 문서를 읽는 부분은 순수 함수가 될 수 없어서 jsdom에서 본다. 배치가 없으므로 요소마다 `data-rect`로 사각형을 적고 `getBoundingClientRect`가 그것을 읽게 했다. 포인터 이벤트와 키 이벤트를 직접 보내고, 프레임은 테스트가 손으로 돌린다. jsdom에 없는 것(`CSS.escape`, `scrollIntoView`, 포인터 캡처)은 테스트가 채운다.

| 묶음                                      | 건수 | 보는 것                                                                                                                                                                  |
| ----------------------------------------- | ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| two sortables on one root                 | 7    | 카드를 누르면 카드만·머리글을 누르면 묶음만, 묶음은 머리글로만, 안쪽 항목의 손잡이는 바깥 항목의 것이 아님, Space도 같음, 묶음의 이동이 카드 쪽에 가지 않음, 키보드, RTL |
| the items of a list                       | 2    | 카드 안에 든 같은 범위의 목록의 항목은 세지 않는다, 가운데로 자리를 정한다                                                                                               |
| outside the root                          | 3    | 밖에서의 `active`와 `notice`, 놓으면 취소와 안내, 도로 들어오기, 창 밖으로 나간 루트의 부분                                                                              |
| a view whose lists place items themselves | 5    | 포인터는 목록만 고른다·`sortedIndex`가 받는 인자·`to`에 `index`가 없다, 콜백 없으면 끝, 같은 목록, 키보드와 다시 읽기, `placement`는 집을 때 묻는다                      |
| a zone                                    | 4    | 놓을 곳에 놓기, 다른 범위의 놓을 곳, 키보드로 가고 오기, `zones` 없으면 못 간다                                                                                          |
| a selection                               | 4    | 고른 것 전부(데이터의 순서), 나르는 것을 뺀 인덱스, 고르지 않은 것은 하나만, 키보드                                                                                      |

테스트가 실제로 무언가를 잡는지 한 번 확인했다: 목록의 항목을 가리는 조건을 일부러 무력하게 하니 3건이 실패했다(되돌렸다).

`pointer-drag.ts`와 `auto-scroll.ts`에는 여전히 그 파일만의 단위 테스트가 없다. `pointer-drag.ts`는 위의 jsdom 테스트가 마우스의 길로 지나가고, 터치·진동·자동 스크롤은 브라우저 테스트로만 본다.

### 브라우저 테스트 (`spikes/drag/*.spec.ts`, 94건 통과. 전 30건)

프로젝트는 둘이다: `desktop`(Desktop Chrome, 1280×720)과 `touch`(Pixel 7, 412×839). 마우스는 `page.mouse`로, 터치는 CDP 세션의 `Input.dispatchTouchEvent`로 `touchStart` → 프레임마다 `touchMove` → `touchEnd`/`touchCancel`을 보낸다. 이 길로 보낸 터치는 Chromium의 제스처 처리를 그대로 거친다. 테스트는 끝에 화면의 `data-column`의 순서와 그 안 `data-card`의 순서를 읽어 비교한다.

| 파일                      | desktop | touch | 내용                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| ------------------------- | ------- | ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `drag.spec.ts`            | 16      | 14    | 처음 판의 30건. 마우스 9, 터치 7, 키보드 7×2. **테스트 본문은 한 글자도 바꾸지 않았다**(도우미 함수를 `support.ts`로 옮기고 가져오게만 했다). 도우미를 옮기기 전의 파일 그대로도 새 코드에서 30건이 통과하는 것을 먼저 확인했다                                                                                                                                                                                                                                                                        |
| `drag-decisions.spec.ts`  | 9       | 10    | **보드 밖**: 밖에 놓으면 순서·스크롤 그대로·취소 안내·밖에서는 스크롤 없음, 아래쪽도 밖, 도로 들어오면 이어 끈다, 손가락으로 위쪽에 놓기. **정렬된 보기**: 다른 열에 놓으면 정렬상의 자리(포인터가 어디에 있든), 같은 열은 변화 없음, 문구가 보인다, 손가락, 키보드(↑↓는 안내를 다시 읽는다), 스위치. **진동**: 길게 누르면 한 번·설정의 길이, 쓸기와 짧은 누름은 떨지 않음, `vibrate`가 없거나 오류를 던져도 드래그는 된다, 마우스·키보드는 떨지 않음                                                 |
| `drag-columns.spec.ts`    | 17      | 10    | **마우스**: 머리글로 열 옮기기(카드는 그대로), 앞쪽으로, 카드를 누르면 열이 끌리지 않음, 열 본문의 빈 곳을 눌러도 안 끌림, 머리글 클릭, 오른쪽 가장자리에서 보드가 스크롤되어 맨 끝으로, Escape(순서와 스크롤), 보드 밖. **터치**: 머리글을 길게 눌러 옮기기(보드가 스크롤된다), 머리글 위 쓸기는 스크롤, 카드를 길게 누르면 카드. **키보드**: 집기·옮기기·놓기와 안내·초점, 화면 밖으로 나르면 보드가 따라옴, Escape, Tab, 카드와 열이 섞이지 않음. **RTL**: 열의 마우스·키보드, 카드의 키보드·마우스 |
| `drag-foundation.spec.ts` | 12      | 6     | **여러 장**: 고른 것 전부가 데이터의 순서로 간다, 고르지 않은 카드는 혼자 간다, 떨어진 것들이 붙는다, Escape, 키보드, 정렬된 보기에서. **숨긴 열**: 놓으면 놓을 곳만 전달된다(자리 없음), 곁은 놓을 곳이 아니다, 놓을 곳이 없을 때, 손가락으로 화면 밖의 놓을 곳까지, 키보드로 가고 오기, Escape                                                                                                                                                                                                       |

건너뛴 50건: 다른 프로젝트 몫의 테스트(마우스 테스트는 `touch`에서, 터치 테스트는 `desktop`에서) 48건과 측정 파일의 2건.

드래그의 묶음을 여섯 번 돌려 여섯 번 모두 통과했다(다른 작업이 같이 도는 기기에서, 그 사이에 스파이크 화면의 그리기를 한 번 고쳤다).

출력(2026-11-10 20:46 KST, `FVOCI_SPIKES_PORT=18331 pnpm --filter web run spikes:e2e`. 편집기 스파이크의 20건이 함께 돈다. 통과한 줄 114개와 건너뛴 줄 50개는 줄였다):

```
Running 164 tests using 10 workers
  …
  ✓  163 [touch] › spikes/drag/drag.spec.ts:448:3 › keyboard › reaching for the pointer gives a carried card up (755ms)
  ✓  164 [touch] › spikes/drag/drag.spec.ts:458:3 › keyboard › Enter on a card that is not carried opens it (930ms)

  50 skipped
  114 passed (33.5s)
```

### 그 밖의 검증 (2026-11-10 20:44–20:45 KST)

```
pnpm --filter web run check    svelte-check: 2824 FILES 0 ERRORS 0 WARNINGS / eslint 통과 / All matched files use Prettier code style! / licences: ok / names: ok (374 files) / message files: up to date (18 locales) / message keys: ok
pnpm --filter web run test     TZ=UTC                 Test Files 29 passed (29), Tests 846 passed (846)
                               TZ=Asia/Seoul          Test Files 29 passed (29), Tests 846 passed (846)
                               TZ=Pacific/Kiritimati  Test Files 29 passed (29), Tests 846 passed (846)
pnpm --filter web run build    fonts: up to date / ✓ built in 827ms
```

846건 가운데 이 스파이크의 것이 154건이다(순수 함수 129, jsdom 25). `check`와 `test`와 `build`는 이 문서의 위 두 출력 칸을 채우기 전의 문서로 돌렸고, 그 뒤 문서만 Prettier와 이름 검사로 다시 확인했다.

## 다른 네 곳에 쓸 수 있는가

층별로 답이 다르다. 아래 두 층(`pointer-drag.ts`, `auto-scroll.ts`)과 기하 함수는 네 곳 모두에서 그대로 쓸 수 있다. `Sortable`은 "목록들 + 인덱스, 그리고 그 곁의 놓을 곳"이라는 목표를 전제로 하므로 그 모양에 맞는 곳에서만 쓸 수 있다. 이번 일반화로 달라진 칸은 굵게 적었다.

| 곳                                  | `pointer-drag`·`auto-scroll` | `Sortable`                                        | 새로 필요한 것                                             |
| ----------------------------------- | ---------------------------- | ------------------------------------------------- | ---------------------------------------------------------- |
| 칸반: 열 순서                       | 그대로                       | **그대로(`scope`, `axis: 'x'`). 스파이크에 있다** | 열의 끌리는 모습, 옮길 수 없는 열이 있다면 그 제한         |
| 칸반: 숨긴 열·접힌 열에 놓기        | 그대로                       | **그대로(zone)**                                  | 화면(W3)                                                   |
| 칸반: 여러 장                       | 그대로                       | **그대로(`selection`)**                           | 고르는 UI와 미리보기(W3)                                   |
| 표: 열 순서                         | 그대로                       | 그대로(`axis: 'x'`), **브라우저에서 돌려 봤다**   | 고정 열, 삽입선 그리기. **RTL은 코드와 테스트가 있다**     |
| 표: 열 너비                         | 그대로                       | 쓰지 않는다                                       | 즉시 시작하는 손잡이, 키보드 조절                          |
| 사이드바: 보드를 다른 범주로        | 그대로                       | 그대로, 조건부                                    | **접힌 범주는 zone으로 받는다.** 링크인 항목의 실기기 확인 |
| 사이드바: 범주 순서                 | 그대로                       | **그대로(`scope`)**                               | 화면(W3)                                                   |
| 카드 내용 블록 (2차원)              | 그대로                       | 맞지 않는다                                       | 새 순수 함수(블록 + 방향), 목표를 모르는 일반 컨트롤러     |
| 달력: 일정을 다른 날로, 기간 늘리기 | 그대로                       | 맞지 않는다                                       | 날짜 계산, 기간 손잡이, 일반 컨트롤러                      |

**표의 열 순서.** 머리글 행이 목록 하나, 머리글 칸이 항목이고 축이 `x`다. 스파이크의 열 끌기가 같은 구성(`axis: 'x'`, 목록 하나, 가로 자동 스크롤, 키보드의 ←→)이고 브라우저 테스트가 있다. 남은 것: (1) 옮길 수 없는 열(제목 열)과 그 앞에는 놓을 수 없다는 제한. "놓을 수 있는 인덱스의 범위"나 `canDrop` 같은 옵션이 여전히 없다. (2) 표에서는 빈자리를 열기보다 열 사이에 삽입선을 그리는 편이 맞다. `active.gap.index`로 그릴 수 있으므로 화면 쪽 일이다. RTL은 `reversed`로 받는다.

**표의 열 너비.** 정렬이 아니라 "누른 곳에서 얼마나 움직였는가"만 필요하다. `pointerDrag`의 세션을 직접 쓰면 된다. 더할 것: 손잡이는 길게 누르기 없이 바로 시작해야 한다. 지금은 시작 규칙이 포인터 종류로 고정되어 있다(마우스 = 거리, 터치 = 지연). `touchDelay: 0`과 손잡이의 `touch-action: none`으로 될 것으로 보이나 해 보지 않았다. 키보드 조절은 따로 짜야 한다.

**사이드바: 보드를 다른 범주로.** 범주가 위아래로 쌓인 목록들이고 항목도 세로로 쌓인다. 그대로 붙일 수 있다. 접힌 범주는 목록 상자의 높이가 0이라 맞히기 어려웠는데, 이제 접힌 범주의 머리글을 놓을 곳(zone)으로 표시하면 "그 범주에 놓았다"를 받는다(자리는 호출부가 정한다). 머리글 위에 머무르면 펼치는 동작은 여전히 없다. 항목이 링크(`<a>`)인 경우의 터치는 실제 기기에서 봐야 한다.

**사이드바: 범주 순서.** 범주 자체가 바깥 목록의 항목이 되는 두 단계 정렬이다. 처음 판에서는 고쳐야 했고, 이제 `scope`로 받는다. 칸반의 열과 카드가 같은 모양이다. 범주 머리글만 잡게 하는 것은 `data-drag-handle`이다.

**카드 내용 블록.** 처음 판 그대로다. 놓는 곳이 "어느 블록의 왼쪽·오른쪽" 또는 "어느 행의 위·아래"라서 `Destination`의 세 가지 어느 것도 아니다. 필요한 것: (1) 포인터와 블록 사각형들에서 `{ dstBlockId, moveTo }`를 내는 새 순수 함수. `nearestList`, `clampToRect`, `containsPoint`는 재료로 쓸 수 있다. (2) `Sortable`에서 목표와 무관한 절반(프레임 루프, 자동 스크롤 연결, 끌리는 모습의 좌표, 놓기·취소, 초점 복귀, 안내)을 떼어 "포인터 → 목표"와 "키 → 목표" 두 함수를 주입받는 일반 컨트롤러로 만드는 것. (3) 미리보기는 삽입선이어야 한다. (4) 키보드로 네 방향이 무엇을 뜻하는지는 제품이 정해야 한다.

**달력.** 처음 판 그대로다. 여러 날에 걸친 일정은 어느 한 칸의 항목이 아니므로 `Sortable`의 모델에 맞지 않는다. `pointerDrag`를 직접 쓰고 날짜 차이로 기간을 평행 이동하는 것이 맞다. 기간 늘리기는 즉시 시작하는 손잡이(열 너비와 같은 요구)다.

정리: 일반 컨트롤러로 나누는 일은 카드 내용 블록이나 달력을 실제로 만들 때 하는 것이 맞다. 그때 `Sortable`에서 떼어 낼 것은 위 (2)의 절반이고, 목록에 관한 판정은 이미 `drop-target.ts`에 있다.

## 한계 (측정하고 관찰한 것)

### 카드 수와 프레임 시간

`spikes/drag/measure.spec.ts`, headless Chromium(Playwright 1.65의 빌드), Vite 개발 서버(프로덕션 빌드 아님), AMD Ryzen 9 5950X, WSL2. 두 열에 같은 수의 카드를 모두 그리고(가상화 없음) 마우스로 한 장을 집어, 옆 열 위를 프레임마다 한 번씩 위아래로 120번 움직이고("이동"), 이어서 열의 아래 가장자리에 2초 동안 멈춰 자동 스크롤시켰다("스크롤"). 프레임 사이의 시간을 `requestAnimationFrame`으로 쟀다. 60Hz에서 한 프레임은 16.7ms다.

처음 판에서 잰 값(2026-11-10 19:04 KST, 한 번, 처음 판의 코드):

| 열마다 카드 | 이동: 95% | 이동: 최장 | 이동: 20ms 넘은 프레임 | 스크롤: 95% | 스크롤: 최장 | 스크롤: 20ms 넘은 프레임 | 놓기   |
| ----------- | --------- | ---------- | ---------------------- | ----------- | ------------ | ------------------------ | ------ |
| 100         | 16.7ms    | 16.8ms     | 238 중 0               | 16.7ms      | 16.8ms       | 118 중 0                 | 10.5ms |
| 500         | 16.7ms    | 16.8ms     | 254 중 0               | 16.8ms      | 16.8ms       | 118 중 0                 | 4.5ms  |
| 1000        | 16.8ms    | 33.4ms     | 263 중 5               | 33.3ms      | 33.4ms       | 109 중 9                 | 11.3ms |
| 2000        | 50.0ms    | 66.7ms     | 265 중 20              | 66.6ms      | 66.7ms       | 75 중 21                 | 15.4ms |
| 5000        | 150.0ms   | 200.0ms    | 308 중 21              | 216.6ms     | 216.7ms      | 21 중 18                 | 46.3ms |

**이번에 다시 잰 값은 믿을 만한 표가 되지 못했다.** 2026-11-10 20:29–20:38 KST에, 다른 작업들이 같이 도는 기기에서(부하 평균 8–20, 논리 코어 32) 기준 커밋 `ce3ba0bd`의 코드와 지금 코드를 번갈아 쟀는데 실행마다 값이 흔들렸다. 20ms를 넘은 프레임의 수만 옮긴다("이동 중 늦은 것 / 스크롤 중 늦은 것", 각각 "전체 중 몇"):

| 코드 | 시각  | 500장               | 1000장                | 2000장               |
| ---- | ----- | ------------------- | --------------------- | -------------------- |
| 기준 | 20:29 | 255 중 0 / 118 중 0 | 260 중 14 / 98 중 18  | 263 중 19 / 70 중 19 |
| 기준 | 20:29 | 254 중 0 / 118 중 0 | 264 중 10 / 89 중 20  | 263 중 19 / 69 중 20 |
| 기준 | 20:37 | 254 중 0 / 118 중 0 | 260 중 14 / 93 중 18  | 267 중 18 / 46 중 15 |
| 지금 | 20:36 | 239 중 0 / 118 중 0 | 253 중 0 / 119 중 0   | 257 중 10 / 65 중 36 |
| 지금 | 20:38 | 241 중 0 / 109 중 9 | 312 중 12 / 108 중 11 | 284 중 13 / 56 중 40 |

5000장은 한 번씩만 쟀다: 기준 338 중 22 / 20 중 17, 지금 386 중 59 / 22 중 22. 어느 쪽이든 쓸 수 없다.

읽을 수 있는 것과 없는 것:

- 같은 조건에서 기준 커밋의 코드도 처음 판의 표보다 나쁘게 나왔다(1000장의 "이동"에서 5 → 10–14). 기기의 부하 탓으로 본다. 그러니 위의 두 표를 서로 견줄 수는 없다.
- 지금 코드가 기준보다 느려졌다는 증거는 없다. 1000장은 기준과 같거나 낫다. 다만 2000장의 "스크롤"에서는 늦은 프레임이 더 많다(대신 긴 프레임은 짧다: 95%가 50–67ms, 기준은 67–150ms). 어느 쪽이 나은지 이 측정으로는 말할 수 없다.
- 지금 코드의 두 실행도 서로 다르다(1000장에서 0과 12). 한 번의 값으로 결론을 내면 안 된다.
- 처음 판의 결론(열마다 500장까지는 무리가 없고, 1000장부터 프레임이 빠지기 시작하며, 2000장은 끊김이 보인다)을 바꿀 근거는 없다. 조용한 기기에서 다시 재야 한다.

이번에 프레임마다의 일에서 바뀐 것: (1) 목록과 놓을 곳의 상자를 문서에서 한 번에 찾는다(선택자 하나). (2) 항목마다 "가장 가까운 목록이 이 목록인가"를 묻는 것은 그 목록 안에 같은 범위의 목록이 실제로 있을 때만 한다. 처음에는 항목마다 물었고, 측정을 보고 고쳤다. (3) 스파이크의 열은 빈자리가 움직일 때 자기 열의 목록만 다시 짜고, 카드의 항목 객체를 그대로 둔다(전에는 빈자리가 움직일 때마다 모든 열이 모든 카드를 새 객체로 다시 짰다. 이것은 부품이 아니라 화면의 일이다).

비용은 여전히 포인터 밑에 있는 **한 열**의 카드 수에 비례한다(그 열의 모든 카드의 `getBoundingClientRect`와, 빈자리가 움직일 때의 재배치). 정렬된 보기에서는 항목의 사각형을 읽지 않으므로 이 비용이 없을 것이나 재지 않았다. 줄일 길(이분 탐색, 보이는 범위만 읽기, 가상화)은 어느 것도 하지 않았다.

재지 않은 것: 실제 휴대폰에서의 프레임 시간, 프로덕션 빌드에서의 값, 카드가 실제 제품처럼 무거울 때의 값, 열을 끌 때의 값.

### 그 밖에 관찰한 것

- **경로에 따라 한 칸 다를 수 있다.** "놓일 곳 계산"에 적은 대로다.
- **놓을 때 애니메이션이 없다.**
- **집자마자 스크롤될 수 있다.** 열의 보이는 영역 맨 아래 56px 안에 있는 카드를 집으면 곧바로 열이 내려간다.
- **머리글 높이로 카드를 끌면 지나가는 열이 위로 스크롤된다.** "자동 스크롤"에 적은 규칙의 결과다.
- **가장자리로 스크롤하려다 보드 밖으로 나가면 스크롤이 멈추고 취소 문구가 뜬다.** 보드가 창의 가장자리에 닿아 있으면 마우스로는 창 밖으로 나가야 생기는 일이지만, 보드 옆에 사이드바가 붙으면 왼쪽으로 스크롤하려다 쉽게 넘어갈 수 있다. 경계 밖에 "아직 취소가 아닌" 여유 폭을 둘지는 사람이 해 보고 정해야 한다. 지금은 0이고 설정도 없다.
- **보드가 화면을 꽉 채우면 손가락으로는 밖에 놓을 수 없다.** "보드 밖에 놓으면 취소"에 적었다.
- **보이는 영역은 창으로만 자른다.** 루트가 스크롤되는 조상에 가려져 있어도 가려진 부분은 "안"으로 친다.
- **끌리는 열은 열만큼 크다.** 휴대폰에서는 화면을 거의 다 가린다. 줄여 그릴지는 화면의 일이다.
- **열을 끌 때의 문턱은 열의 가운데다.** 열이 넓어서 옆 열의 가운데를 넘기려면 꽤 움직여야 한다. 카드와 같은 규칙을 그대로 쓴 것이다.
- **여러 장을 집으면 빈자리가 처음부터 한 칸 어긋날 수 있다.** "여러 장"에 적었다.
- **정렬된 보기와 여러 장.** 빈자리는 잡은 카드의 정렬상 자리 하나에만 선다. 나머지 카드가 어디로 갈지는 놓은 뒤에야 보인다.
- **`sortedIndex`는 목표가 바뀔 때만 불린다.** 끄는 동안 데이터가 바뀌면 빈자리가 낡는다.
- **가상화된 목록은 지원하지 않는다.** 인덱스는 문서에 있는 `data-drag-item`을 세어 얻는다.
- **드래그 중에 데이터가 바뀌는 경우를 다루지 않는다.** 화면이 `sortable.cancel()`을 부르는 것이 지금 할 수 있는 전부다.
- **모든 카드와 모든 열 머리글이 Tab 정지점이다.** roving tabindex는 만들지 않았다.
- **창이 초점을 잃어도 취소하지 않는다.**
- **두 번째 손가락은 무시한다.**
- **카드를 키보드로 나르는 중에 열을, 또는 그 반대를 집을 수는 없다.** 나르는 인스턴스가 키를 모두 가져간다. 다만 한 인스턴스가 나르는 중에 다른 인스턴스의 항목을 **포인터로** 누르면, 나르던 것은 취소되고 그 누름이 새 드래그가 될 수 있다(해 보지 않았다).

### 스파이크를 하며 걸린 것 (다음 스파이크를 위해)

- `src/styles/app.css`가 `@source not '../../spikes'`로 스파이크 폴더를 빼므로, 스파이크의 스타일시트에서 `@source './'`(폴더)로는 클래스가 다시 들어오지 않는다. `@source './*.svelte'`처럼 **파일 글롭**으로 적어야 들어온다.
- `releasePointerCapture` 뒤의 `lostpointercapture`는 바로 오지 않고 그 포인터의 다음 이벤트와 함께 온다.
- Playwright의 추적(`trace: 'retain-on-failure'`)은 동작마다 문서 전체를 복사한다. 카드 수천 장을 재는 테스트에서는 꺼야 한다.
- Playwright 실행을 밖에서 `timeout`으로 죽이면 Vite 개발 서버가 남는다.
- 소스에 U+00A0을 글자 그대로 적으면 ESLint(`no-irregular-whitespace`)가 막는다. `String.fromCodePoint(0xa0)`로 적었다.
- 테스트에서 목표 카드가 열의 보이는 영역 밖에 있으면 그 좌표는 보드 밖이 될 수 있다. 이제 보드 밖은 취소이므로, 그런 테스트는 조용히 "아무 일도 없음"으로 끝난다(두 번 걸렸다).
- Svelte 5에서 `{#each}`에 넘기는 배열을 매번 새 객체로 채우면 키가 같아도 항목마다의 효과가 모두 다시 돈다. 그리고 자식이 프롭을 `$derived` 안에서 읽으면 부모의 그 식이 읽는 신호에 바로 묶인다. 자주 바뀌는 상태(`active`)에서 뽑은 값은 자식 쪽에서 `$derived`로 한 번 받아 두면 값이 같을 때 아래가 다시 돌지 않는다.

## 확인하지 못한 것

- **실제 터치 기기.** Android Chrome과 iOS Safari 어느 쪽도 손으로 만져 보지 않았다. 터치 테스트는 데스크톱 headless Chromium에 Pixel 7의 화면 크기와 터치 입력을 흉내 낸 것이다. 특히 보지 못한 것: 길게 누를 때의 OS 메뉴·글자 선택, iOS에서 길게 누른 뒤의 `touchmove` 취소가 스크롤을 막는지, 300ms가 OS의 길게 누르기보다 충분히 짧은지, 손가락이 끌리는 카드와 문구를 가리는 정도.
- **진동이 실제로 느껴지는가.** 테스트는 `navigator.vibrate`가 불리는 것만 본다. 15ms가 느껴지는 길이인지, 기기의 설정(무음, 햅틱 끔)에 따라 어떻게 되는지 모른다. 또 Chromium은 사용자가 그 페이지를 한 번도 누른 적이 없으면 `vibrate`를 무시하는 것으로 알고 있는데, 손가락을 대고만 있는 것이 "누른 적"으로 쳐지는지 확인하지 못했다. 쳐지지 않는다면 페이지를 연 뒤 첫 길게 누르기는 떨지 않는다.
- **Safari와 Firefox.** 이 기기에 설치되어 있지 않다.
- **화면 낭독기.** 안내 영역의 글자가 맞게 바뀌는 것과 초점의 위치만 테스트로 확인했다. NVDA·VoiceOver·TalkBack이 실제로 무엇을 읽는지, 같은 문구에 U+00A0을 붙이는 방법이 다시 읽게 하는지, 열을 키보드로 나를 때 머리글이 문서 안에서 옮겨지며 초점이 어떻게 읽히는지 모른다.
- **사람 손으로 한 조작.** 화면은 Playwright로만 움직였다. "사람이 해 볼 것"의 항목들은 모두 사람이 해 봐야 판단할 수 있다.
- **펜.** 터치와 같은 길로 가게만 해 두었다.
- **RTL.** Chromium에서 열과 카드의 마우스·키보드를 확인했다. RTL에서의 자동 스크롤, 터치, 놓을 곳은 보지 않았다.
- **카드의 손잡이(`data-drag-handle`)와 링크인 항목.** 손잡이는 열의 머리글로 브라우저에서 돌렸고, 카드 안의 손잡이는 jsdom 테스트만 있다. 링크인 항목은 테스트가 없다.
- **놓을 곳이 여러 개일 때, 열의 범위에 놓을 곳이 있을 때.** 단위·jsdom 테스트만 있다. 브라우저에서는 카드의 놓을 곳 하나만 돌렸다.
- **드래그 중의 휠 스크롤, 창 밖으로 나간 포인터.** 코드는 있고 테스트는 없다.
- **확대된 화면(브라우저 확대, 두 손가락 확대), iframe 안, 세 겹 이상의 스크롤 상자.**
- **어두운 화면에서의 모습.** 토큰만 썼으므로 따라올 것으로 보이나 찍어 보지 않았다.
- **Storybook과 제품 빌드.** 이 코드는 아직 제품의 어느 화면에도 들어가 있지 않다. 제품 빌드(`build`)는 통과하지만 드래그 코드는 그 번들에 없다.

## 참고한 것

코드는 모두 새로 썼다. `github.com/fvoci/groundzero`의 `board.ts`와 `e2e/board-drag.spec.ts`는 열어 보지 않았다.

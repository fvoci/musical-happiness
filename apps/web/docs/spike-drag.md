# 스파이크: 포인터 이벤트로 만든 칸반 드래그

작성: 2026-11-10 19:08 KST. 기준 커밋: `1faf53c0` ("Add the layering lint rule and a place for spikes").

드래그 라이브러리와 HTML5 드래그 앤 드롭 API 없이, Pointer Events 위에 칸반 카드의 드래그를 직접 만들어 본 결과다. 화면은 `spikes/drag/`에 있고 서버·API·라우터·상태 라이브러리를 쓰지 않는다. 데이터는 메모리에 만든 가짜다.

## 요약

- 마우스, 터치(길게 누르기), 키보드로 카드를 다른 열의 원하는 자리로 옮기고, 같은 열 안에서 순서를 바꾸고, 가장자리에서 자동 스크롤하고, 취소하면 원래대로 돌아가는 것까지 Chromium에서 동작한다. 브라우저 테스트 30건과 단위 테스트 55건이 통과한다.
- 놓일 자리 계산은 DOM을 모르는 순수 함수(`src/lib/logic/drop-target.ts`)이고, 입력을 다루는 부품은 `src/lib/drag/`에 세 층으로 나뉘어 있다: 포인터 한 번의 드래그(`pointer-drag.ts`), 자동 스크롤(`auto-scroll.ts`), 목록 사이의 정렬(`sortable.svelte.ts`).
- 드래그 코드만의 크기는 축소(minify)한 뒤 약 10 kB, gzip으로 약 3.7 kB다(`sortable.svelte.ts`와 그것이 가져오는 것을 esbuild로 묶어 잰 어림값이고, Svelte 런타임은 빠져 있다).
- 나머지 네 곳 가운데 **표의 열 순서**와 **사이드바의 보드 이동**은 `Sortable`을 거의 그대로 쓸 수 있다. **열 너비 조절**, **달력**, **카드 내용 블록**은 아래 두 층(`pointer-drag`, `auto-scroll`)과 기하 함수는 그대로 쓰지만 `Sortable`은 맞지 않는다. "목록 + 인덱스"가 아닌 목표(블록의 왼쪽·오른쪽·위·아래, 날짜, 너비)를 받도록 `Sortable`의 절반(프레임 루프, 끌리는 모습, 취소, 초점, 안내)을 일반화해야 한다. 자세한 것은 "다른 네 곳" 절.
- 실제 터치 기기, Safari, Firefox, 화면 낭독기로는 확인하지 못했다. 이 기기에는 Chromium(headless shell)만 있다.

## 만든 것

| 파일                                 | 줄  | 내용                                                                       |
| ------------------------------------ | --- | -------------------------------------------------------------------------- |
| `src/lib/logic/drop-target.ts`       | 240 | 순수 함수: 놓일 자리, 키보드 이동, 순서 변경, 가장자리 스크롤 세기         |
| `tests/logic/drop-target.test.ts`    | 435 | 위의 단위 테스트 55건                                                      |
| `src/lib/drag/pointer-drag.ts`       | 275 | 누름에서 뗌까지 포인터 한 번의 드래그. 목록이나 놓일 자리를 모른다         |
| `src/lib/drag/auto-scroll.ts`        | 87  | 한 프레임의 자동 스크롤과, 취소 때 되돌릴 스크롤 위치의 기억               |
| `src/lib/drag/sortable.svelte.ts`    | 405 | 목록 안·목록 사이의 정렬. 포인터와 키보드, 화면이 그릴 상태(`$state`)      |
| `spikes/drag/index.html`, `main.ts`  |     | 스파이크 화면의 진입점                                                     |
| `spikes/drag/Board.svelte`           | 100 | 보드: 열을 늘어놓고, 끌리는 카드와 안내 영역을 그린다                      |
| `spikes/drag/Column.svelte`          | 102 | 열: 카드 목록과 빈자리                                                     |
| `spikes/drag/CardFace.svelte`        | 33  | 카드의 생김새                                                              |
| `spikes/drag/data.ts`, `messages.ts` |     | 가짜 데이터(6열, 열마다 12·30·6·18·9·24장), 안내 문구(영어 상수)           |
| `spikes/drag/drag.css`               | 13  | 앱의 스타일시트 + 이 화면의 클래스                                         |
| `spikes/drag/drag.spec.ts`           | 606 | Playwright 테스트                                                          |
| `spikes/drag/measure.spec.ts`        | 125 | 프레임 시간 측정. `FVOCI_SPIKE_MEASURE`를 주지 않으면 아무것도 재지 않는다 |

기존 파일은 고치지 않았다. 의존성도 추가하지 않았다.

실행:

```bash
pnpm --filter web run spikes                                   # http://127.0.0.1:5173/spikes/drag/
cd apps/web && FVOCI_SPIKES_PORT=18331 pnpm run spikes:e2e     # 브라우저 테스트
```

주소에 `?sizes=3,0,500`처럼 열마다의 카드 수를 줄 수 있다(빈 열, 아주 긴 열을 볼 때).

## 동작 방식

### 쓰는 이벤트

| 이벤트                                      | 어디에                           | 무엇을 위해                                                                               |
| ------------------------------------------- | -------------------------------- | ----------------------------------------------------------------------------------------- |
| `pointerdown`                               | 보드(루트)                       | 누른 곳의 카드를 찾아 "드래그가 될 수 있는 누름"으로 기억한다                             |
| `pointermove`, `pointerup`, `pointercancel` | `window`, 캡처 단계, 누른 동안만 | 따라가기, 놓기, 취소                                                                      |
| `lostpointercapture`                        | 보드                             | 캡처를 잃으면 취소                                                                        |
| `keydown`                                   | 보드 / `window`(캡처)            | Space로 집기 / 드래그·운반 중의 화살표, Space, Enter, Escape, Tab                         |
| `touchmove` (`passive: false`)              | 보드, 그리고 손가락이 닿은 요소  | 손가락으로 끄는 동안 브라우저의 스크롤을 막는다. Pointer Events가 아닌 유일한 입력 이벤트 |
| `contextmenu`, `selectstart`, `dragstart`   | 보드 / `document`                | 길게 누르기의 메뉴, 글자 선택, 링크·이미지의 브라우저 자체 드래그를 막는다                |
| `click`                                     | `window`, 캡처, 한 번            | 드래그 끝의 버튼 뗌이 카드의 클릭이 되지 않게 삼킨다                                      |
| `scroll`                                    | 보드, 캡처, 드래그 동안만        | 휠 등으로 스크롤되면 놓일 자리를 다시 계산한다                                            |

드래그가 시작되면 보드가 `setPointerCapture`로 포인터를 잡는다. 카드가 아니라 보드가 잡는 까닭은 드래그 중에 카드 요소가 문서에서 빠질 수 있어서다. 캡처는 포인터가 창 밖으로 나가도 이벤트가 계속 오게 하려는 것이기도 한데, 그 경우는 테스트하지 않았다.

### 드래그의 시작

- **마우스.** 버튼(주 버튼만)을 누른 채 4px 넘게 움직이면 시작한다. 움직이지 않고 떼면 평범한 클릭이고 카드가 "열린다"(스파이크에서는 화면 아래에 id를 적는다).
- **터치.** 카드 위에 손가락을 300ms 동안 8px 안에서 머무르면 시작한다(길게 누르기). 그 전에 8px 넘게 움직이면 누름을 버리고 브라우저가 평소처럼 스크롤한다. 펜도 터치와 같이 다룬다.
- **키보드.** 초점이 있는 카드에서 Space.

터치를 길게 누르기로 정한 까닭:

1. 휴대폰 화면에서는 카드가 보드의 거의 전부를 덮는다. 카드 위에서 시작한 손가락 움직임이 스크롤이 되지 않으면 보드를 넘길 수 없다. 그래서 카드에 `touch-action: none`을 줄 수 없다.
2. `touch-action`은 손가락이 닿는 순간에 읽히고 그 뒤에는 바꿀 수 없다. "닿은 뒤에 드래그로 정해졌으니 이제 스크롤하지 마라"를 Pointer Events만으로는 말할 수 없어서, 드래그가 시작된 뒤의 `touchmove`를 `preventDefault` 한다. 브라우저는 손가락이 닿을 때 취소 가능한 리스너가 있는지를 정하므로 이 리스너는 보드에 미리 붙여 둔다.
3. 손잡이 방식(카드의 한 부분에만 `touch-action: none`)은 Pointer Events만으로 되고 더 확실하지만, 카드마다 44px짜리 손잡이 자리가 필요하고 마우스(카드 어디든 잡는다)와 방식이 달라진다. `data-drag-handle`로 손잡이에서만 시작하게 하는 것은 구현해 두었으나(브라우저 테스트는 없다), 그때도 터치의 시작 규칙은 길게 누르기다.

길게 누르기의 비용: 300ms의 기다림, 방법이 눈에 보이지 않는다는 점, OS의 길게 누르기(문맥 메뉴, 글자 선택, iOS의 미리보기)와 겹친다는 점. 겹침은 카드의 `user-select: none`, `-webkit-touch-callout: none`, `contextmenu` 취소로 막았지만 실제 기기에서 확인하지 못했다. 또 보드에 취소 가능한 `touchmove` 리스너가 있으므로, 브라우저는 보드 위에서 스크롤을 시작하기 전에 메인 스레드의 답을 기다린다(메인 스레드가 바쁘면 스크롤 시작이 늦어진다).

### 그리는 방법

`Sortable`은 DOM을 옮기지 않는다. 데이터도 놓을 때까지 바꾸지 않는다. 지금 무엇이 어디로 가는 중인지를 `active`(`itemId`, `from`, `to`, 크기)와 `overlay`(끌리는 카드의 왼쪽 위 좌표)로 알려 주고, 화면이 그것을 그린다.

- **원래 카드**는 제자리에 `display: none`으로 남는다. 문서에서 빼지 않는 까닭: 터치 이벤트는 손가락이 처음 닿은 요소로 계속 전달되는데, 그 요소가 문서에서 빠지면 이벤트가 보드까지 올라오지 않아 스크롤을 막을 수 없게 된다. (대비로 손가락이 닿은 요소 자체에도 `touchmove` 리스너를 붙인다.)
- **빈자리**는 `active.to`가 가리키는 열의 그 인덱스에 그리는, 집을 때의 카드 높이와 같은 점선 상자다. 다른 카드들이 그만큼 밀린다.
- **끌리는 카드**는 `position: fixed`인 겹침층에 같은 카드 컴포넌트를 한 번 더 그린 것이다. `pointermove`마다 `translate`만 바뀐다. `pointer-events: none`이다.
- **키보드로 나르는 동안**에는 끌리는 카드가 없다. 빈자리에 점선 상자 대신 카드 자체(테두리로 강조)를 그리고 초점을 거기에 둔다.

놓을 때의 애니메이션은 없다. 끌리는 카드가 사라지고 카드가 새 자리에 나타난다.

### 자동 스크롤

드래그 동안 `requestAnimationFrame` 루프가 돈다. 프레임마다:

1. 포인터가 움직였거나 무언가 스크롤됐으면 놓일 자리를 다시 계산한다.
2. 지금 놓일 열에서 보드까지 올라가며 `data-drag-scroll="x|y"`가 붙은 상자를 모은다(열의 본문은 `y`, 보드는 `x`).
3. 상자마다 포인터가 가장자리 56px 안에 있으면 스크롤한다. 속도는 가장자리에 가까울수록 빨라져 가장자리와 그 바깥에서 초당 900px다(`edgeScrollFactor`). 프레임 사이의 시간으로 거리를 정하고, 한 프레임에 1px이 안 되는 분량은 모아 두었다가 쓴다.
4. 스크롤됐으면 놓일 자리를 한 번 더 계산한다. 포인터가 가만히 있어도 그 밑의 카드가 바뀌기 때문이다.

취소하면 자동 스크롤이 건드린 상자들의 스크롤 위치를 드래그 전으로 되돌린다. 놓으면 그대로 둔다.

### 놓일 자리 계산

`dropTarget(point, lists, {axis, bounds})`가 한다.

1. 포인터를 보드의 보이는 영역(`bounds`) 안으로 끌어당긴다. 보드 밖으로 스크롤되어 나간 열이 포인터 밑에 있는 것으로 잡히지 않게 하기 위해서다.
2. 열을 고른다(`nearestList`). 가로 거리가 가장 가까운 열, 같으면 세로 거리가 가까운 열이다. 처음에는 단순한 직선 거리로 만들었는데, 짧은 열의 아래쪽 빈 곳에 놓으려 하면 옆의 긴 열이 더 가깝게 잡혔다(브라우저 테스트 "빈 열에 놓기"에서 드러났다). 열이 나란히 있을 때는 가로가 먼저여야 한다.
3. 포인터를 그 열의 보이는 상자 안으로 끌어당긴다. 열 위쪽(머리글)에 있는 포인터는 "보이는 첫 카드 앞"이 되고, 스크롤로 가려진 목록의 맨 앞이 되지 않는다.
4. 인덱스는 "가운데가 포인터보다 앞에 있는 카드의 수"다(`insertionIndex`). 끌리는 카드 자신은 세지 않는다.

인덱스는 언제나 **끌리는 카드를 뺀 목록**에서의 자리다. 같은 열 안의 이동과 열 사이의 이동을 같은 숫자로 말하기 위해서다.

알아 둘 성질: 계산은 **지금 화면에 놓인 대로**의 사각형으로 한다. 빈자리가 열리면 그 아래 카드들이 밀리므로, 같은 포인터 위치라도 빈자리가 어디에 있었는지에 따라 답이 한 칸 다를 수 있다(빈자리를 아래로 보내려면 포인터가 "밀려 있는" 다음 카드의 가운데를 넘어야 한다). 빈자리가 열린 뒤에 답이 도로 뒤집히는 일은 없다(단위 테스트가 있다). 사용자는 빈자리를 보고 있으므로 놓이는 곳은 언제나 보이는 빈자리와 같다. 브라우저 테스트가 목표 카드를 "지금 놓인 자리에서" 재서 두 번 다가가는 것은 이 때문이다.

보드 밖 아무 데나 놓아도 가장 가까운 자리에 놓인다. "밖에 놓으면 취소"는 넣지 않았다.

### 취소

Escape, `pointercancel`, `lostpointercapture`, 그리고 코드에서 부르는 `sortable.cancel()`이 같은 길로 간다: 데이터는 애초에 바뀐 적이 없으므로 `active`를 지우고, 자동 스크롤을 되돌리고, 안내 문구를 낸다.

Escape로 취소했을 때는 버튼이나 손가락이 아직 눌려 있다. 그 누름의 나머지는 아무 일도 하지 않게 했다: 손가락은 떼기 전까지 스크롤하지 않고, 버튼을 떼도 클릭이 되지 않는다(취소 뒤 원래 카드 위에서 버튼을 떼면 카드가 열리는 것을 막는다).

### 키보드와 안내

| 키                | 동작                                                                           |
| ----------------- | ------------------------------------------------------------------------------ |
| Space (카드에서)  | 집는다                                                                         |
| ↑ ↓               | 같은 열에서 한 칸. 끝에서 멈춘다                                               |
| ← →               | 옆 열로. 인덱스를 유지하고, 그 열이 더 짧으면 맨 끝. RTL에서는 방향이 뒤집힌다 |
| Home, End         | 열의 맨 앞, 맨 끝                                                              |
| Space, Enter      | 놓는다                                                                         |
| Escape            | 취소하고 원래 자리로                                                           |
| Tab               | 막는다. 놓거나 취소할 때까지 초점이 카드에 머문다                              |
| Enter (집지 않고) | 카드를 연다(스파이크의 화면이 하는 일이고 `Sortable`의 일이 아니다)            |

나르는 동안 마우스나 손가락으로 어디든 누르면 취소한다. 이동할 때마다 카드가 보이도록 `scrollIntoView({block: 'nearest', inline: 'nearest'})`를 부른다. 놓거나 취소한 뒤에는 초점이 그 카드로 돌아간다.

안내는 화면이 그리는 `aria-live="assertive"` 영역에 `sortable.announcement`를 넣는 방식이다. 문구는 `Sortable`에 넣지 않고 `messages` 옵션으로 받는다(스파이크에서는 `spikes/drag/messages.ts`의 영어 상수, 제품에서는 paraglide 메시지를 넘기면 된다). 집었을 때, 옮길 때마다, 놓았을 때, 취소했을 때 "카드 이름, N개 중 몇 번째, 어느 열"을 알린다. 포인터로 놓거나 취소했을 때도 알린다. 카드에는 `aria-describedby`로 사용법 문장이 붙는다.

## 공개 형태

### 순수 함수: `src/lib/logic/drop-target.ts`

```ts
interface Point { x: number; y: number }
interface Rect { left: number; top: number; width: number; height: number }
type Axis = 'x' | 'y'; // 목록이 항목을 쌓는 방향

interface ListBox { id: string; rect: Rect; items: readonly Rect[] } // 보이는 상자, 끌리는 것을 뺀 항목들
interface DropTarget { listId: string; index: number } // index: 끌리는 것을 뺀 목록에서의 자리
interface ListOrder { id: string; items: readonly string[] } // 데이터로서의 순서

clampToRect(point, rect): Point
nearestList<L extends { rect: Rect }>(point, lists, axis = 'y'): L | undefined
insertionIndex(point, items: readonly Rect[], axis = 'y'): number
dropTarget(point, lists: readonly ListBox[], options?: { axis?: Axis; bounds?: Rect }): DropTarget | undefined
sameTarget(first, second): boolean

locateItem(lists: readonly ListOrder[], itemId): DropTarget | undefined
moveItem(lists: readonly ListOrder[], itemId, target: DropTarget): ListOrder[] // 입력을 바꾸지 않는다

type KeyboardStep = 'previous' | 'next' | 'first' | 'last' | 'previousList' | 'nextList';
keyboardTarget(current: DropTarget, step: KeyboardStep, lists: readonly ListOrder[], itemId): DropTarget

edgeScrollFactor(position, start, size, edge): number // -1 … 1
```

DOM 타입은 없다. `DOMRect`는 `Rect`의 모양을 갖추고 있어 그대로 넘길 수 있다. 좌표계는 호출하는 쪽이 하나로 맞추기만 하면 된다(스파이크는 뷰포트 좌표).

### 재사용 부품: `src/lib/drag/`

```ts
// pointer-drag.ts — 프레임워크와 무관하다
interface PointerDragOptions { mouseDistance: number; touchDelay: number; touchTolerance: number } // 기본 4, 300, 8
interface PointerDragSession {
  start(point: Point): void; // 한 번
  move(point: Point): void;
  drop(point: Point): void; // start 뒤에 drop 과 cancel 가운데 정확히 하나
  cancel(): void;
}
type PointerDragSource = (event: PointerEvent) => PointerDragSession | undefined;
pointerDrag(root: HTMLElement, source: PointerDragSource, options?): { cancel(): void; destroy(): void }

// auto-scroll.ts — 프레임워크와 무관하다
interface AutoScrollOptions { edge: number; maxSpeed: number } // 기본 56px, 900px/s
interface ScrollBox { element: HTMLElement; axis: Axis }
class AutoScroller {
  constructor(options?: Partial<AutoScrollOptions>);
  step(point: Point, boxes: readonly ScrollBox[], elapsed: number): boolean; // 움직였는가
  restore(): void; // 건드린 상자를 처음 위치로
  clear(): void;
}

// sortable.svelte.ts — Svelte 5 ($state)
interface SortableMove { itemId: string; from: DropTarget; to: DropTarget }
interface SortableActive { itemId: string; mode: 'pointer' | 'keyboard'; from: DropTarget; to: DropTarget; width: number; height: number }
interface SortableMessages { pickedUp(place): string; moved(place): string; dropped(place): string; cancelled(place): string }
interface SortableOptions {
  order: () => readonly ListOrder[]; // 부르는 시점의 순서
  onDrop: (move: SortableMove) => void; // 자리가 바뀐 놓기마다 한 번
  messages: SortableMessages;
  itemLabel?: (itemId: string) => string;
  listLabel?: (listId: string) => string;
  axis?: Axis;
  pointer?: Partial<PointerDragOptions>;
  scroll?: Partial<AutoScrollOptions>;
}
class Sortable {
  constructor(options: SortableOptions);
  readonly active: SortableActive | undefined; // 반응형
  readonly overlay: Point | undefined; // 반응형. 포인터로 끄는 카드의 왼쪽 위(뷰포트 좌표)
  readonly announcement: string; // 반응형
  readonly attach: (root: HTMLElement) => () => void; // {@attach sortable.attach}
  cancel(): void;
}
```

화면이 지킬 약속(속성):

| 속성                      | 붙이는 곳                                                       |
| ------------------------- | --------------------------------------------------------------- |
| `data-drag-list="<id>"`   | 목록의 보이는 상자(스크롤한다면 스크롤하는 그 요소)             |
| `data-drag-item="<id>"`   | 목록 안의 항목. 초점을 받을 수 있어야 한다                      |
| `data-drag-scroll="x\|y"` | 가장자리에서 스크롤할 상자. 루트이거나 루트와 목록 사이의 요소  |
| `data-drag-handle`        | 항목 안에 있으면 그 부분에서만 포인터 드래그가 시작된다         |
| `data-drag-carried`       | 키보드로 나르는 동안 화면이 빈자리에 그리는 대역. 초점을 받는다 |

끌리는 항목은 드래그 동안 문서에 남아 있어야 한다(숨겨도 된다). 항목 안의 `input`, `textarea`, `select`, `button`, `a[href]`, `contenteditable`에서 시작한 누름은 드래그가 되지 않는다.

스파이크 화면이 쓰는 모습(`spikes/drag/Board.svelte`):

```svelte
const sortable = new Sortable({
  order: () => order,
  onDrop: ({ itemId, to }) => { order = moveItem(order, itemId, to); },
  messages: MESSAGES,
});
…
<div data-drag-scroll="x" {@attach sortable.attach}> … </div>
<div aria-live="assertive" aria-atomic="true" class="sr-only">{sortable.announcement}</div>
```

## 테스트

### 단위 테스트 (`tests/logic/drop-target.test.ts`, 55건)

| 함수               | 건수 | 보는 것                                                                                                                                      |
| ------------------ | ---- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `clampToRect`      | 3    | 안, 밖(변과 모서리), 넓이 없는 사각형                                                                                                        |
| `nearestList`      | 9    | 포인터 밑의 열, 열 사이, 정확히 가운데(앞의 것), 바깥, 짧은 열 아래(가로 우선), 위아래로 쌓인 목록, 격자, `x`축, 목록 없음                   |
| `insertionIndex`   | 7    | 가운데가 경계, 처음과 끝, 빈 목록, 높이가 다른 항목, `x`축, 빈자리가 열린 뒤에도 답이 같은지                                                 |
| `dropTarget`       | 10   | 카드의 위·아래 절반, 빈 열, 짧은 열의 아래, 스크롤된 열, 열의 위·아래로 벗어난 포인터, 가만히 있는 포인터 밑으로 스크롤, 보드 밖으로 나간 열 |
| `sameTarget`       | 1    |                                                                                                                                              |
| `locateItem`       | 2    |                                                                                                                                              |
| `moveItem`         | 8    | 다른 열로, 빈 열로, 같은 열 위·아래로, 제자리, 끝을 넘는 인덱스, 입력 불변, 모르는 id                                                        |
| `keyboardTarget`   | 9    | 한 칸, 끝에서 멈춤, Home·End, 옆 열(인덱스 유지·짧으면 끝), 자기 열로 돌아오기, 처음·마지막 열, 걸음의 끝이 `moveItem`과 맞는지              |
| `edgeScrollFactor` | 6    | 가운데 0, 가장자리로 갈수록 증가, 부호, 상자 밖 ±1, 짧은 상자, 크기 0                                                                        |

`pointer-drag.ts`, `auto-scroll.ts`, `sortable.svelte.ts`에는 단위 테스트가 없다. 브라우저 테스트로만 확인한다.

### 브라우저 테스트 (`spikes/drag/drag.spec.ts`)

프로젝트는 둘이다: `desktop`(Desktop Chrome, 1280×720)과 `touch`(Pixel 7, 412×839). 마우스는 `page.mouse`로, 터치는 CDP 세션의 `Input.dispatchTouchEvent`로 `touchStart` → 프레임마다 `touchMove` → `touchEnd`/`touchCancel`을 보낸다. 이 길로 보낸 터치는 Chromium의 제스처 처리를 그대로 거친다: 테스트에서 빠른 쓸기는 실제로 열과 보드를 스크롤했고, 길게 누른 뒤의 움직임은 스크롤하지 않았다. 모든 테스트는 끝에 화면의 `data-column` 안 `data-card`의 순서를 읽어 비교한다.

- **마우스 (desktop, 9건).** 다른 열의 고른 자리로(카드의 위쪽 → 그 앞, 아래쪽 → 그 뒤), 같은 열에서 아래로·위로, 제자리에 놓으면 변화 없음, 화면 밖의 열과 화면 밖의 자리로(오른쪽 가장자리에 멈춰 보드를 끝까지 스크롤 → 마지막 열의 아래 가장자리에 멈춰 열을 끝까지 스크롤, 그동안 포인터가 가만히 있어도 놓일 인덱스가 커지는 것 → 스크롤 뒤 지금 자리의 카드로), Escape(순서·스크롤 위치·눌린 버튼의 뒤처리·다음 클릭), `lostpointercapture`, 드래그 없는 클릭은 카드를 연다, 빈 열에 놓기.
- **터치 (touch, 7건).** 세로 쓸기는 열을 스크롤하고 아무것도 끌지 않는다, 가로 쓸기는 보드를 스크롤한다, 기다리는 중에 움직인 손가락은 끌지 않는다, 길게 눌러 같은 열에서 순서 바꾸기(끄는 동안 열이 스크롤되지 않는 것 포함), 보드를 자동 스크롤해 옆 열로, 열을 자동 스크롤해 긴 열의 끝으로, `touchCancel`(→ `pointercancel`)은 취소.
- **키보드 (두 프로젝트, 7건씩).** 집기·열 안과 열 사이 이동·놓기와 그때마다의 안내 문구·초점, 끝에서 멈춤과 Home·End, 화면 밖의 열로 나르면 보드가 따라오는 것, Escape, Tab이 초점을 빼앗지 않는 것, 나르는 중에 마우스로 다른 곳을 누르면 취소, 집지 않은 Enter는 카드를 연다.

터치 테스트가 찾아낸 것 하나: 열의 본문에 `overscroll-behavior: contain`을 주었더니 가로 쓸기가 보드로 넘어가지 않았다(열은 가로로 스크롤할 것이 없는데도). `overscroll-behavior-y: contain`으로 바꾸었다.

출력(2026-11-10 19:06 KST, `cd apps/web && FVOCI_SPIKES_PORT=18331 pnpm run spikes:e2e`):

```
Running 48 tests using 4 workers
  ✓   2 [desktop] › spikes/drag/drag.spec.ts:109:3 › mouse › moves a card to a chosen place in another column (1.2s)
  ✓   8 [desktop] › spikes/drag/drag.spec.ts:133:3 › mouse › the lower half of a card is the place after it (773ms)
  ✓  12 [desktop] › spikes/drag/drag.spec.ts:147:3 › mouse › reorders a card within its column, down and up (1.2s)
  ✓  15 [touch] › spikes/drag/drag.spec.ts:368:3 › touch › a swipe over the cards scrolls the column and drags nothing (882ms)
  ✓  16 [desktop] › spikes/drag/drag.spec.ts:175:3 › mouse › dropping a card where it was changes nothing (422ms)
  ✓  17 [touch] › spikes/drag/drag.spec.ts:379:3 › touch › a swipe sideways over the cards scrolls the board and drags nothing (866ms)
  ✓  19 [touch] › spikes/drag/drag.spec.ts:390:3 › touch › a finger that moves off before the wait is over does not drag (1.1s)
  ✓  20 [touch] › spikes/drag/drag.spec.ts:402:3 › touch › a long press picks a card up; it is reordered within its column (1.6s)
  ✓  18 [desktop] › spikes/drag/drag.spec.ts:181:3 › mouse › a drag to a column and a place that are both out of sight scrolls to them (5.2s)
  ✓  21 [touch] › spikes/drag/drag.spec.ts:422:3 › touch › moves a card to the next column, which the board scrolls into view (3.0s)
  ✓  22 [desktop] › spikes/drag/drag.spec.ts:221:3 › mouse › Escape cancels: order, scrolling and the pressed button are as if nothing happened (1.6s)
  ✓  24 [desktop] › spikes/drag/drag.spec.ts:249:3 › mouse › losing the pointer capture cancels (860ms)
  ✓  25 [desktop] › spikes/drag/drag.spec.ts:273:3 › mouse › a click without a drag still opens the card (368ms)
  ✓  26 [desktop] › spikes/drag/drag.spec.ts:279:3 › mouse › a drop into an empty column (576ms)
  ✓  23 [touch] › spikes/drag/drag.spec.ts:453:3 › touch › a drag to the end of a long column scrolls the column (3.0s)
  ✓  35 [desktop] › spikes/drag/drag.spec.ts:487:3 › keyboard › picks a card up, moves it within and between columns and drops it (542ms)
  ✓  31 [touch] › spikes/drag/drag.spec.ts:472:3 › touch › a touch the system takes away cancels (1.8s)
  ✓  36 [desktop] › spikes/drag/drag.spec.ts:526:3 › keyboard › stops at the ends, and Home and End go there (445ms)
  ✓  37 [touch] › spikes/drag/drag.spec.ts:487:3 › keyboard › picks a card up, moves it within and between columns and drops it (559ms)
  ✓  38 [desktop] › spikes/drag/drag.spec.ts:548:3 › keyboard › carries a card to a column that is out of sight, and the board follows (505ms)
  ✓  39 [touch] › spikes/drag/drag.spec.ts:526:3 › keyboard › stops at the ends, and Home and End go there (485ms)
  ✓  40 [desktop] › spikes/drag/drag.spec.ts:565:3 › keyboard › Escape puts the card back (514ms)
  ✓  42 [desktop] › spikes/drag/drag.spec.ts:581:3 › keyboard › Tab does not leave a carried card behind (446ms)
  ✓  41 [touch] › spikes/drag/drag.spec.ts:548:3 › keyboard › carries a card to a column that is out of sight, and the board follows (594ms)
  ✓  43 [desktop] › spikes/drag/drag.spec.ts:590:3 › keyboard › reaching for the pointer gives a carried card up (485ms)
  ✓  44 [touch] › spikes/drag/drag.spec.ts:565:3 › keyboard › Escape puts the card back (546ms)
  ✓  45 [desktop] › spikes/drag/drag.spec.ts:600:3 › keyboard › Enter on a card that is not carried opens it (398ms)
  ✓  46 [touch] › spikes/drag/drag.spec.ts:581:3 › keyboard › Tab does not leave a carried card behind (415ms)
  ✓  47 [touch] › spikes/drag/drag.spec.ts:590:3 › keyboard › reaching for the pointer gives a carried card up (430ms)
  ✓  48 [touch] › spikes/drag/drag.spec.ts:600:3 › keyboard › Enter on a card that is not carried opens it (338ms)
  18 skipped
  30 passed (19.2s)
```

건너뛴 18건의 줄(`-`로 시작)은 위에서 뺐다. 다른 프로젝트 몫의 테스트(마우스 테스트는 `touch`에서, 터치 테스트는 `desktop`에서) 16건과 측정 파일의 2건이다. 실패는 없다.

### 그 밖의 검증 (2026-11-10 19:07 KST)

```
pnpm --filter web run check    svelte-check: 1621 FILES 0 ERRORS 0 WARNINGS / eslint 통과 / prettier 통과 / licences: ok / names: ok (312 files)
pnpm --filter web run test     Test Files 19 passed (19), Tests 486 passed (486)
TZ=UTC                         Tests  486 passed (486)
TZ=Asia/Seoul                  Tests  486 passed (486)
TZ=Pacific/Kiritimati          Tests  3 failed | 483 passed (486)
```

Kiritimati에서 실패하는 3건은 `tests/logic/filter.test.ts`의 날짜 조건 테스트(isBefore, isAfter, is)로, 이 스파이크 전부터 실패하던 것이고 손대지 않았다. 새 단위 테스트 55건은 세 시간대 모두에서 통과한다.

## 다른 네 곳에 쓸 수 있는가

층별로 답이 다르다. 아래 두 층(`pointer-drag.ts`, `auto-scroll.ts`)과 기하 함수(`nearestList`, `insertionIndex`, `clampToRect`, `edgeScrollFactor`)는 네 곳 모두에서 그대로 쓸 수 있다. `Sortable`은 "목록들 + 인덱스"라는 목표를 전제로 하므로 그 모양에 맞는 곳에서만 쓸 수 있다.

| 곳                                  | `pointer-drag`·`auto-scroll` | `Sortable`                  | 새로 필요한 것                                         |
| ----------------------------------- | ---------------------------- | --------------------------- | ------------------------------------------------------ |
| 표: 열 순서                         | 그대로                       | 그대로(`axis: 'x'`), 조건부 | 고정 열, RTL, 삽입선 그리기                            |
| 표: 열 너비                         | 그대로                       | 쓰지 않는다                 | 즉시 시작하는 손잡이, 키보드 조절                      |
| 사이드바: 보드를 다른 범주로        | 그대로                       | 그대로, 조건부              | 접힌 범주, 링크인 항목의 실기기 확인                   |
| 사이드바: 범주 순서                 | 그대로                       | 고쳐야 한다                 | 한 루트 안의 두 단계 정렬(범위 구분)                   |
| 카드 내용 블록 (2차원)              | 그대로                       | 맞지 않는다                 | 새 순수 함수(블록 + 방향), 목표를 모르는 일반 컨트롤러 |
| 달력: 일정을 다른 날로, 기간 늘리기 | 그대로                       | 맞지 않는다                 | 날짜 계산, 기간 손잡이, 일반 컨트롤러                  |

**표의 열 순서.** 머리글 행이 목록 하나, 머리글 칸이 항목이고 축이 `x`다. `insertionIndex`·`dropTarget`·`nearestList`의 `x`축은 단위 테스트가 있고, `Sortable`의 `axis` 옵션은 키보드 방향까지 바꾸게 만들어 두었으나 브라우저에서 돌려 보지 않았다. 더할 것: (1) 옮길 수 없는 열(제목 열)과 그 앞에는 놓을 수 없다는 제한 — "놓을 수 있는 인덱스의 범위"나 `canDrop` 같은 옵션이 없다. (2) `insertionIndex`는 항목이 좌표가 커지는 순서로 놓였다고 가정한다. RTL 표에서는 뒤집어야 한다. (3) 표에서는 빈자리를 열기보다 열 사이에 삽입선을 그리는 편이 맞다. `active.to.index`로 그릴 수 있으므로 화면 쪽 일이다. 표의 가로 스크롤은 `data-drag-scroll="x"`로 지금 방식 그대로 된다.

**표의 열 너비.** 정렬이 아니라 "누른 곳에서 얼마나 움직였는가"만 필요하다. `pointerDrag`의 세션(`start`/`move`/`drop`/`cancel`)을 직접 쓰면 된다. 더할 것: 손잡이는 길게 누르기 없이 바로 시작해야 한다. 지금은 시작 규칙이 포인터 종류로 고정되어 있다(마우스 = 거리, 터치 = 지연). `touchDelay: 0`과 손잡이의 `touch-action: none`으로 될 것으로 보이나 해 보지 않았다. 누름마다 시작 규칙을 고르는 옵션을 넣는 것이 깔끔하다. 키보드 조절(`role="separator"`에 화살표 키)은 따로 짜야 한다.

**사이드바: 보드를 다른 범주로.** 범주가 위아래로 쌓인 목록들이고 항목도 세로로 쌓인다. `nearestList`가 이 배치를 다루고(가로 거리가 같으면 세로 거리로 고른다, 단위 테스트 있음) `Sortable`은 열이 나란한지 쌓였는지를 가정하지 않으므로 그대로 붙일 수 있다. 더할 것: (1) 접힌 범주는 목록 상자의 높이가 0이라 맞히기 어렵다. 머리글을 목록 상자에 넣거나, 머리글 위에 머무르면 펼치는 동작을 새로 만들어야 한다. (2) 항목이 링크(`<a>`)다. 항목 자신이 링크인 경우는 누름을 받게 해 두었고 `dragstart`를 막아 브라우저의 링크 드래그를 끄지만, 터치에서 링크를 길게 누를 때의 메뉴·미리보기는 실제 기기에서 봐야 한다.

**사이드바: 범주 순서.** 범주 자체가 바깥 목록의 항목이 되는 두 단계 정렬이다. 지금의 `Sortable`은 루트 아래의 모든 `data-drag-list`와 그 안의 모든 `data-drag-item`을 한 단계로 본다. 같은 루트에 인스턴스를 둘 붙이면 서로의 목록과 항목을 섞어 읽는다. 고칠 것: 인스턴스마다 범위를 주어(속성 값에 종류를 넣거나 속성 이름을 옵션으로) 자기 단계의 것만 읽게 하고, 목록 안의 항목을 모을 때 안쪽 목록으로 내려가지 않게 한다. 칸반의 **열 자체를 끄는 것**도 같은 문제다. 범주 머리글만 잡게 하는 것은 `data-drag-handle`로 된다.

**카드 내용 블록.** 한 행에 블록이 여럿 나란히 놓이고, 놓는 곳이 "어느 블록의 왼쪽·오른쪽" 또는 "어느 행의 위·아래"다. `src/lib/logic/content-order.ts`의 `dragAndDropRearrange`가 받는 것도 `dstBlockId` + `moveTo: 'left' | 'right' | 'aboveRow' | 'belowRow'`다. "목록 + 인덱스"가 아니다. 필요한 것: (1) 새 순수 함수 — 포인터와 블록 사각형들에서 `{ dstBlockId, moveTo }`를 내는 것(포인터 밑이나 가장 가까운 블록을 고르고, 블록의 좌우 가장자리 띠에 있으면 `left`/`right`, 아니면 위·아래 절반으로 `aboveRow`/`belowRow`). `nearestList`와 `clampToRect`는 재료로 쓸 수 있다. (2) `Sortable`에서 목표와 무관한 절반 — 프레임 루프, 자동 스크롤 연결, 끌리는 모습의 좌표, 놓기·취소, 초점 복귀, 안내 — 을 떼어 "포인터 → 목표"와 "키 → 목표" 두 함수를 주입받는 일반 컨트롤러로 만드는 것. `Sortable`은 그 위의 한 가지 구성이 된다. (3) 미리보기는 빈자리가 아니라 삽입선이어야 한다. 2차원 배치에서 빈자리를 열면 행이 다시 짜여 포인터 밑이 계속 바뀐다. (4) 키보드로 네 방향이 무엇을 뜻하는지는 제품이 정해야 한다.

**달력.** 일정을 다른 날로 옮기는 것은 "포인터 밑의 날짜 칸"만 알면 된다. 날짜 칸은 격자이고 `nearestList`가 격자에서 칸을 고른다(단위 테스트 있음). 그러나 여러 날에 걸친 일정은 어느 한 칸의 항목이 아니므로 `Sortable`의 모델(항목은 한 목록에 속한다)에 맞지 않는다. `pointerDrag`를 직접 쓰고 "잡은 날짜에서 포인터 밑의 날짜까지 며칠인가"로 기간을 평행 이동하는 것이 맞다. 기간 늘리기는 일정 막대 양끝의 손잡이에서 즉시 시작하는 드래그(열 너비와 같은 요구)이고 시작 ≤ 끝의 제한이 붙는다. 키보드(±1일, ±1주)와 안내는 위의 일반 컨트롤러가 있으면 같은 틀로 짤 수 있다. 달 보기는 보통 스크롤하지 않으므로 자동 스크롤은 주 보기·일 보기에서만 필요하다.

정리: 두 번째 쓰임새를 만들 때 `Sortable`을 "일반 컨트롤러 + 목록 전략"으로 나누는 것을 권한다. 지금 나누지 않은 것은 쓰임새가 하나뿐이라 경계를 추측으로 그어야 해서다.

## 한계 (측정하고 관찰한 것)

### 카드 수와 프레임 시간

측정함. `spikes/drag/measure.spec.ts`, headless Chromium(Playwright 1.65의 빌드), Vite 개발 서버(프로덕션 빌드 아님), AMD Ryzen 9 5950X, WSL2. 두 열에 같은 수의 카드를 모두 그리고(가상화 없음) 마우스로 한 장을 집어, 옆 열 위를 프레임마다 한 번씩 위아래로 120번 움직이고("이동"), 이어서 열의 아래 가장자리에 2초 동안 멈춰 자동 스크롤시켰다("스크롤"). 프레임 사이의 시간을 `requestAnimationFrame`으로 쟀다. 60Hz에서 한 프레임은 16.7ms다. "놓기"는 버튼을 뗀 때부터 새 순서가 그려진 프레임까지다.

| 열마다 카드 | 이동: 95% | 이동: 최장 | 이동: 20ms 넘은 프레임 | 스크롤: 95% | 스크롤: 최장 | 스크롤: 20ms 넘은 프레임 | 놓기   |
| ----------- | --------- | ---------- | ---------------------- | ----------- | ------------ | ------------------------ | ------ |
| 100         | 16.7ms    | 16.8ms     | 238 중 0               | 16.7ms      | 16.8ms       | 118 중 0                 | 10.5ms |
| 500         | 16.7ms    | 16.8ms     | 254 중 0               | 16.8ms      | 16.8ms       | 118 중 0                 | 4.5ms  |
| 1000        | 16.8ms    | 33.4ms     | 263 중 5               | 33.3ms      | 33.4ms       | 109 중 9                 | 11.3ms |
| 2000        | 50.0ms    | 66.7ms     | 265 중 20              | 66.6ms      | 66.7ms       | 75 중 21                 | 15.4ms |
| 5000        | 150.0ms   | 200.0ms    | 308 중 21              | 216.6ms     | 216.7ms      | 21 중 18                 | 46.3ms |

측정 시각: 2026-11-10 19:04 KST. 한 번씩만 쟀다.

읽는 법과 결론:

- 열마다 500장까지는 늦은 프레임이 하나도 없었다. 1000장에서 프레임이 가끔 하나씩 빠지고(이동 중 2%, 스크롤 중 8%), 2000장에서는 스크롤 중 프레임의 28%가 늦고 가장 긴 것이 67ms라 끊김이 보일 수준이다. 5000장은 쓸 수 없다(프레임 하나가 150~217ms).
- "이동"에는 테스트가 다음 움직임을 보내기를 기다리는 빈 프레임이 섞여 있다. 그래서 중앙값은 모든 줄에서 16.7ms이고(표에서 뺐다) 95%와 최장을 봐야 한다.
- 비용은 포인터 밑에 있는 **한 열**의 카드 수에 비례한다. 놓일 자리를 계산할 때마다 그 열의 모든 카드의 사각형을 읽고(`getBoundingClientRect` N번), 빈자리가 움직이면 브라우저가 그 열을 다시 배치하기 때문이다. 다른 열의 카드는 읽지 않게 만들었다(열의 상자만 읽는다). 어느 쪽이 더 큰지는 프로파일하지 않아 모른다.
- 줄일 길: 항목이 순서대로 놓여 있으므로 이분 탐색으로 읽는 사각형을 log N개로 줄이기, 보이는 범위의 항목만 읽기, 긴 열의 가상화. 어느 것도 하지 않았다.
- "놓기"에는 다음 프레임을 기다리는 시간이 섞여 있어 작은 값끼리는 비교할 수 없다. 5000장에서도 46ms 한 번이다.

재지 않은 것: 실제 휴대폰에서의 프레임 시간, 프로덕션 빌드에서의 값, 카드가 실제 제품처럼 무거울 때(속성 배지, 아바타)의 값.

### 그 밖에 관찰한 것

- **경로에 따라 한 칸 다를 수 있다.** "놓일 자리 계산"에 적은 대로다.
- **놓을 때 애니메이션이 없다.**
- **집자마자 스크롤될 수 있다.** 열의 보이는 영역 맨 아래 56px 안에 있는 카드를 집으면 곧바로 열이 내려간다. 포인터가 가장자리 쪽으로 움직였을 때만 스크롤하는 조건은 넣지 않았다.
- **머리글 높이로 끌면 지나가는 열이 위로 스크롤된다.** 포인터가 열 본문의 위쪽 바깥에 있으면 그 열을 최고 속도로 올리기 때문이다.
- **가상화된 목록은 지원하지 않는다.** 인덱스는 문서에 있는 `data-drag-item`을 세어 얻는다. 긴 열을 일부만 그리게 되면 "그리지 않은 앞쪽 항목의 수"를 더하는 길이 필요하다.
- **드래그 중에 데이터가 바뀌는 경우를 다루지 않는다.** 다른 사람의 변경이 웹소켓으로 들어와 순서가 바뀌면 `from`이 낡는다. 화면이 `sortable.cancel()`을 부르는 것이 지금 할 수 있는 전부다.
- **모든 카드가 Tab 정지점이다.** 카드 사이를 화살표로 다니는 것(roving tabindex)은 만들지 않았다.
- **창이 초점을 잃어도 취소하지 않는다.** 키보드로 나르는 중에 다른 창으로 갔다 오면 나르는 상태 그대로다.
- **한 번에 한 장만.** 여러 장 선택은 없다.
- **두 번째 손가락은 무시한다.** 끄는 중의 두 손가락 확대가 어떻게 되는지는 보지 않았다.

### 스파이크를 하며 걸린 것 (다음 스파이크를 위해)

- `src/styles/app.css`가 `@source not '../../spikes'`로 스파이크 폴더를 빼므로, 스파이크의 스타일시트에서 `@source './'`(폴더)로는 클래스가 다시 들어오지 않는다. `@source './*.svelte'`처럼 **파일 글롭**으로 적어야 들어온다(빌드한 CSS에서 확인).
- `releasePointerCapture` 뒤의 `lostpointercapture`는 바로 오지 않고 그 포인터의 다음 이벤트와 함께 온다.
- Playwright의 추적(`trace: 'retain-on-failure'`)은 동작마다 문서 전체를 복사한다. 카드 수천 장을 재는 테스트에서는 꺼야 한다(`test.use({ trace: 'off' })`).
- Playwright 실행을 밖에서 `timeout`으로 죽이면 Vite 개발 서버가 남는다. 남은 것은 찾아서 껐다.

## 확인하지 못한 것

- **실제 터치 기기.** Android Chrome과 iOS Safari 어느 쪽도 손으로 만져 보지 않았다. 터치 테스트는 데스크톱 headless Chromium에 Pixel 7의 화면 크기와 터치 입력을 흉내 낸 것이다. 특히 보지 못한 것: 길게 누를 때의 OS 메뉴·글자 선택·진동, iOS에서 길게 누른 뒤의 `touchmove` 취소가 스크롤을 막는지, 300ms가 OS의 길게 누르기(대략 400~500ms)보다 충분히 짧은지, 손가락이 끌리는 카드를 가리는 정도.
- **Safari와 Firefox.** 이 기기에 설치되어 있지 않다. Playwright 설정에도 Chromium 프로젝트 둘뿐이다.
- **화면 낭독기.** 안내 영역의 글자가 맞게 바뀌는 것과 초점의 위치만 테스트로 확인했다. NVDA·VoiceOver·TalkBack이 실제로 무엇을 읽는지, 집을 때 초점이 대역 요소로 옮겨 가면서 이름을 두 번 읽지 않는지는 모른다.
- **사람 손으로 한 조작.** 화면은 Playwright로만 움직였다. 끄는 느낌(속도, 빈자리가 움직이는 시점, 자동 스크롤의 빠르기)은 사람이 해 봐야 판단할 수 있다.
- **펜.** 터치와 같은 길로 가게만 해 두었다.
- **RTL.** 키보드의 좌우를 뒤집는 코드는 있으나 돌려 보지 않았다.
- **`axis: 'x'`, `data-drag-handle`, 링크인 항목.** 코드는 있고 브라우저 테스트는 없다.
- **드래그 중의 휠 스크롤, 창 밖으로 나간 포인터.** 코드는 있고 테스트는 없다.
- **확대된 화면(브라우저 확대, 두 손가락 확대), iframe 안, 세 겹 이상의 스크롤 상자.**
- **어두운 화면에서의 모습.** 토큰만 썼으므로 따라올 것으로 보이나 찍어 보지 않았다.

## 제품 책임자에게 묻는 것

1. **터치의 시작 방법.** 길게 누르기(지금, 300ms)로 갈지, 손잡이로 갈지. 길게 누르기라면 시간은 얼마로 할지. 실제 기기에서 250·300·400ms를 비교해 정하는 것이 좋다.
2. **열 자체를 끌 수 있어야 하는가.** 그렇다면 두 단계 정렬이 필요하다("범주 순서"와 같은 작업).
3. **키보드의 키.** 지금은 Space가 집기, Enter가 열기다. 카드가 링크가 될지 버튼이 될지에 따라 달라진다.
4. **보드 밖에 놓으면** 가장 가까운 자리에 놓을지(지금), 취소할지.
5. **미리보기의 모양.** 빈자리가 따라다니는 방식(지금)인지 삽입선인지. 놓을 때 애니메이션이 필요한지.
6. **정렬이 걸린 보기에서의 드래그.** 열 안의 순서가 정렬로 정해질 때는 열만 바꾸고 자리는 무시할지, 자리 이동을 막을지.
7. **여러 장을 한꺼번에 옮기기**가 필요한가.
8. **취소 때 스크롤 위치를 되돌리는 것**(지금)이 맞는가.
9. **자동 스크롤의 빠르기**(가장자리 56px, 최고 초당 900px). 사람이 해 보고 정해야 한다.
10. **접힌 열·숨긴 열에 놓기**가 필요한가(열 머리글에 놓으면 맨 위나 맨 아래로).
11. **진동 피드백**(`navigator.vibrate`)을 길게 누르기가 성립하는 순간에 줄 것인가.

## 참고한 것

코드는 모두 새로 썼다. `github.com/fvoci/groundzero`의 `board.ts`와 `e2e/board-drag.spec.ts`는 열어 보지 않았다.

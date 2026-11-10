# WebSocket `/ws`의 메시지

적은 때: 2026-11-11 04:14 KST. 서버가 **지금** 주고받는 것을 모양을 바꾸지 않고 적었다(`crates/server/src/ws.rs`, 핸드셰이크는 `crates/server/src/lib.rs`의 `handle_websocket`). OpenAPI는 WebSocket을 적지 못하므로 계약 파일 `contracts/openapi.json` 밖에 둔다.

- 메시지마다의 멤버와 타입은 기계가 읽는 `contracts/websocket.json`에 있다. 서버의 테스트(`crates/server/tests/api/websocket.rs`의 `messages_match_the_websocket_contract`)가 서버가 실제로 보낸 메시지를 그 파일과 견준다: 맨 위 멤버가 정확히 그 이름들이고, 실린 객체(`board`, `block` 등)의 멤버는 계약 파일의 같은 이름의 스키마에 있는 것들이다.
- 메시지가 싣는 보드, 블록, 멤버, 카테고리는 계약의 `Board`, `Block`, `BoardMember`, `Category`와 같은 모양이다. 본문 저장(`docs/contract-pending.md` 1절)이 들어가면 `Block`의 모양이 함께 바뀌고, 그 변경은 계약 파일의 차이로 보인다.
- 이 문서의 모양을 바꾸는 일은 계약을 바꾸는 일이다. 바꿀 때는 이 문서, `websocket.json`, `docs/contract-pending.md`를 같은 커밋에서 고친다.

## 1. 핸드셰이크와 인증

주소는 `{base}/ws`이고 `GET`의 WebSocket 업그레이드만 받는다. 그 밖의 메서드는 405(`Allow: GET`), 업그레이드가 아닌 `GET`은 400이다. 둘 다 API의 오류 본문(`ErrorBody`)이다.

누구의 연결인지는 **핸드셰이크에서 한 번** 정해진다. 연결한 뒤에 로그인하는 메시지는 없다(옛 `AUTH`는 모르는 명령으로 무시된다).

| 핸드셰이크 | 결과 |
|---|---|
| `Origin`이 있고 서버의 출처(`serverRoot`의 출처나 설정 `allowed_origins`)가 아니다 | 403 `forbidden`, `reason: csrf`. 연결되지 않는다 |
| `Authorization: Bearer <세션 토큰>`이 있다 | 세션이면 그 사용자의 연결. 세션이 아니면 401 |
| `Origin`이 있고 세션 쿠키(`fvoci_session`, HTTPS에서 `__Host-fvoci_session`)가 있다 | 세션이면 그 사용자의 연결. 끝난 세션의 쿠키면 **익명 연결**(401이 아니다. 로그아웃된 브라우저도 공유 보드를 따라간다) |
| `Origin`이 있고 자격이 없다 | 익명 연결 |
| `Origin`이 없고 Bearer도 없다(브라우저가 아닌 클라이언트) | 401 |

- 둘 다 오면 Bearer를 쓴다.
- 브라우저는 같은 출처의 페이지에서 `new WebSocket(…)`으로 열면 쿠키를 저절로 싣는다. 스크립트가 토큰을 알 필요가 없다.
- 익명 연결은 공유 보드의 블록 구독(`SUBSCRIBE_BLOCKS`)만 할 수 있다.

## 2. 클라이언트가 보내는 명령

명령은 텍스트 프레임의 JSON 객체 하나다. 멤버는 camelCase이고 없는 멤버는 빈 값으로 읽는다. **명령에는 답이 없다.** 받아들였는지, 거절했는지, 모르는 명령인지 알리는 메시지가 없다(서버의 로그에만 남는다). 그래서 구독이 됐는지는 그 뒤에 오는 변경으로만 안다.

| `action` | 세션 | 멤버 | 하는 일 |
|---|---|---|---|
| `SUBSCRIBE_TEAM` | 필요 | `teamId` | 그 팀의 변경을 받는다(3절). 같은 팀을 다시 구독해도 한 번이다 |
| `UNSUBSCRIBE_TEAM` | 필요 | `teamId` | 그만 받는다 |
| `SUBSCRIBE_BLOCKS` | 없어도 된다 | `teamId`, `readToken`, `blockIds` | 공유 보드의 블록들을 읽기 토큰으로 따라간다. `blockIds`가 모두 한 보드의 것이고 `readToken`이 그 보드의 공유 토큰이며 `teamId`가 비어 있지 않아야 한다. 하나라도 어긋나면 명령 전체를 무시한다 |
| `UNSUBSCRIBE_BLOCKS` | 없어도 된다 | 같다 | 그 블록들을 그만 따라간다. 같은 검사를 한다 |

- 세션이 필요한 명령을 익명 연결이 보내면 무시한다.
- JSON이 아니거나, 멤버의 타입이 틀리거나(`teamId`가 수, `blockIds`가 목록이 아님 등), `action`을 모르면 그 명령만 무시하고 연결은 그대로다.
- `SUBSCRIBE_TEAM`의 팀 권한 검사는 지금 늘 참이다(팀이 하나뿐이다). 어느 `teamId`든 받아들이지만, 변경은 그 보드의 팀 ID로 나가므로 맞는 팀 ID(`GET /api/v1/teams`)를 보내야 받는다.
- `blockIds`가 빈 목록이면 보드를 정할 수 없어 읽기 토큰이 맞지 않는 것으로 친다.

## 3. 서버가 보내는 메시지

메시지는 텍스트 프레임의 JSON 객체 하나이고 `action`이 종류다. 그 밖에 서버는 설정 `websocket_ping_seconds`(기본 30초, 0이면 보내지 않음)마다 Ping 프레임을 보낸다. 클라이언트(브라우저)가 Pong으로 답한다.

| `action` | 싣는 것 | 누가 받나 |
|---|---|---|
| `UPDATE_BOARD` | `teamId`, `board`(`Board` 전체) | 그 팀을 구독한 연결 가운데 사용자가 보드의 멤버인 것 |
| `UPDATE_BLOCK` | `teamId`, `block`(`Block` 전체) | 위와 같은 연결, 그리고 그 블록이나 그 블록의 부모를 `SUBSCRIBE_BLOCKS`로 따라가는 연결 |
| `UPDATE_MEMBER` | `teamId`, `member`(`BoardMember`) | 그 팀을 구독한 연결 가운데 사용자가 보드의 멤버인 것 |
| `DELETE_MEMBER` | `teamId`, `member`(`boardId`, `userId`만 뜻이 있고 역할은 모두 `false`) | 위와 같은 연결, 그리고 빠진 사용자의 연결 |
| `UPDATE_CATEGORY` | `teamId`, `category`(`Category`) | 카테고리 주인의 연결 **하나**(아래) |
| `UPDATE_BOARD_CATEGORY` | `teamId`, `boardCategories`(`{boardId, categoryId, hidden}`의 목록) | 그 사용자의 연결 하나 |
| `REORDER_CATEGORIES` | `teamId`, `categoryOrder`(카테고리 ID의 새 순서) | 그 사용자의 연결 하나 |
| `REORDER_CATEGORY_BOARDS` | `teamId`, `categoryId`, `boardOrder`(그 카테고리의 보드 ID의 새 순서) | 그 사용자의 연결 하나 |
| `UPDATE_CLIENT_CONFIG` | `clientConfig` | 모든 연결. **지금은 보내는 곳이 없다**(서버 설정이 실행 중에 바뀌지 않는다) |

### 받는 사람의 규칙

- "보드의 멤버"는 메시지를 보낼 때 저장소에서 읽은 멤버 목록이다. 팀을 구독하지 않은 연결은 멤버여도 받지 않는다.
- 공개 보드라도 멤버가 아닌 사용자는 보드와 블록의 변경을 받지 않는다.
- **카테고리의 네 메시지는 그 사용자의 연결 가운데 그 팀을 먼저 구독한 하나에만 간다.** 같은 사용자의 다른 탭은 받지 않는다(원본에서 이어 온 동작. 7절 G1의 답에 권고가 있다).
- 읽기 토큰으로만 따라가는 연결이 받는 `UPDATE_BLOCK`에서는 `createdBy`, `modifiedBy`가 빈 문자열이다(HTTP의 공유 응답과 같은 까닭: 사용자 ID가 가입 시각을 드러낸다).
- 한 연결이 팀 구독과 블록 구독을 함께 해서 두 길로 다 해당하면 같은 메시지를 두 번 받는다.
- 쓴 사람 자신의 연결도 받는다. 서버는 보낸 사람을 빼지 않는다.

### 메시지를 내는 조작

| 조작(계약의 `operationId`) | 메시지 |
|---|---|
| `createBoard`, `createBoardsAndBlocks`, `duplicateBoard`(템플릿에서 만들기 포함), `onboard` | 새 보드마다 `UPDATE_BOARD`, 블록마다 `UPDATE_BLOCK`, 멤버마다 `UPDATE_MEMBER` |
| `patchBoard` | `UPDATE_BOARD`. 보드의 종류(`type`)가 바뀌면 멤버가 아닌 팀 사용자마다: 비공개가 되면 `DELETE_MEMBER`, 공개가 되면 보기 역할의 `UPDATE_MEMBER` |
| `patchBoardsAndBlocks` | 바뀐 블록마다 `UPDATE_BLOCK`, 바뀐 보드마다 `UPDATE_BOARD` |
| `deleteBoard`, `deleteBoardsAndBlocks` | `UPDATE_BOARD`, `board`는 `id`, `teamId`, `updateAt`, `deleteAt`(0이 아님)만 뜻이 있고 나머지는 빈 값. 받는 사람은 지우기 직전의 멤버. `deleteBoardsAndBlocks`는 그 앞에 블록마다 아래의 삭제 `UPDATE_BLOCK`도 낸다. 보드의 공유 토큰으로 따라가던 연결은 닫힌다 |
| `undeleteBoard` | 돌아온 보드의 `UPDATE_BOARD` 하나. **함께 돌아온 블록과 멤버의 메시지는 없다** |
| `createBlocks`, `patchBlock`, `patchBlocks`, `createCard`, `patchCard`, `duplicateBlock`, `moveContentBlock`, `undeleteBlock` | 바뀌거나 생긴 블록마다 `UPDATE_BLOCK`. `undeleteBlock`은 되살린 블록 하나만 알린다(함께 돌아온 아래 블록은 알리지 않는다) |
| `deleteBlock` | `UPDATE_BLOCK`, `block`은 `id`, `boardId`, `updateAt`, `deleteAt`(0이 아님)만 뜻이 있고 나머지는 빈 값. **지운 블록 하나만** 알린다. 함께 휴지통에 간 아래 블록(카드의 댓글·첨부)은 따로 알리지 않는다 |
| `addMember`, `joinBoard`, `updateMember` | `UPDATE_MEMBER` |
| `removeMember`, `leaveBoard` | `DELETE_MEMBER` |
| `createCategory`, `updateCategory`, `deleteCategory` | `UPDATE_CATEGORY`. 지운 카테고리는 `deleteAt`이 0이 아니다 |
| `reorderCategories` | `REORDER_CATEGORIES` |
| `setBoardCategory`, `hideBoard`, `unhideBoard` | `UPDATE_BOARD_CATEGORY` |
| `reorderCategoryBoards` | `REORDER_CATEGORY_BOARDS` |
| `setSharing`(끄기, 새 토큰) | 메시지 없음. 옛 토큰으로 따라가던 연결이 닫힌다 |
| `patchUserSettings`, `patchClientState` | 메시지 없음 |

메시지는 쓰기가 끝난 뒤 따로 돌며 보낸다. HTTP의 답이 먼저 갈 수도 메시지가 먼저 갈 수도 있다.

### 서버가 연결을 닫을 때

닫을 때 서버는 닫는 프레임을 보내고 이유를 싣지 않는다. 클라이언트는 다시 연결하고 보드를 다시 읽는다.

| 언제 | 닫히는 연결 |
|---|---|
| 로그아웃 | 그 세션으로 연 연결 |
| 관리자가 비밀번호를 재설정 | 그 사용자의 모든 연결 |
| 자기 비밀번호를 바꿈 | 그 사용자의, 바꾼 세션이 아닌 세션의 연결 |
| 세션이 한가함 한도나 절대 한도로 끝남 | 그 세션의 연결. 끝난 세션을 지우는 정리가 한 시간마다 돌 때 닫힌다(`docs/contract-pending.md` 8절) |
| 공유를 끄거나 새 토큰을 내거나 보드를 지움 | 그 보드의 옛 읽기 토큰으로 블록을 따라가던 연결 |
| 보내지 못한 메시지가 512개 쌓임 | 그 연결(읽지 않는 클라이언트) |
| 받은 메시지가 64 KiB를 넘음 | 그 연결 |
| 서버가 멈춤 | 모든 연결 |

## 4. 알아 둘 것

- **놓친 변경을 다시 보내지 않는다.** 순번도 판 번호도 없다. 다시 연결한 클라이언트는 필요한 것을 HTTP로 다시 읽는다.
- **`updateAt`은 서버의 밀리초 시각이다.** 같은 블록에 같은 밀리초의 쓰기가 둘이면 같은 값이 될 수 있다. 시각이 뒤로 가는 것도 막지 않는다(서버의 시계를 따른다).
- 메시지는 보낸 순서대로 한 연결에 도착한다. 다른 쓰기의 메시지 사이의 순서는 쓰기가 끝난 순서와 다를 수 있다(메시지를 따로 돌며 보낸다).
- 명령과 메시지 모두 64 KiB를 넘는 프레임은 받지 않는다(서버가 받는 쪽).

## 5. 웹앱의 질문 G1–G6에 대한 답

`apps/web/docs/contract-assumptions.md`의 "실시간 변경" 표의 질문에, 지금 서버가 하는 일로 답한다. 바꾸자는 권고는 `docs/contract-pending.md`의 "웹앱의 질문에 대한 답" 절에 모았고, 메시지의 모양이나 뜻을 바꾸는 것은 "사용자 결정 필요"로 남겼다.

| # | 웹앱의 가정 | 지금 서버 |
|---|---|---|
| G1 | 구독은 둘: 보드 하나(`watchBoard`), 나 자신(`watchSelf`) | 구독의 단위는 **팀**(`SUBSCRIBE_TEAM`)과 **블록**(`SUBSCRIBE_BLOCKS`, 읽기 토큰으로)이다. 보드 하나만 구독하는 명령은 없다. 팀을 구독하면 내가 멤버인 모든 보드의 보드·블록·멤버 변경과 내 카테고리의 변경이 온다. 그러니 `watchBoard`와 `watchSelf`는 연결 하나의 팀 구독에서 화면이 보드 ID로 갈라 쓴다. 설정(`patchUserSettings`)의 변경은 메시지가 없다. 카테고리 메시지는 내 연결 하나에만 간다(3절) |
| G2 | 변경의 종류 일곱 | 보드: `UPDATE_BOARD`(전체). 보드 삭제: `deleteAt`이 0이 아닌 `UPDATE_BOARD`(따로 된 종류가 없다). 블록: `UPDATE_BLOCK`이 **블록 하나씩**, `Block` 전체를 싣는다. 카드도 요약이 아니라 블록 전체다(지금은 본문이 없으니 크기 문제는 없다). 블록 삭제: `deleteAt`이 0이 아닌 `UPDATE_BLOCK`, 지운 블록 하나만(아래 것은 싣지 않는다). 구성원: 전체 목록이 아니라 멤버 한 명씩(`UPDATE_MEMBER`, `DELETE_MEMBER`). 카테고리: 전체 목록이 아니라 카테고리 하나(`UPDATE_CATEGORY`), 보드의 자리(`UPDATE_BOARD_CATEGORY`), 순서(`REORDER_CATEGORIES`, `REORDER_CATEGORY_BOARDS`). 설정: 메시지 없음 |
| G3 | 내 쓰기도 내게 온다 | 맞다. 보낸 연결도 받는다(카테고리 메시지가 다른 탭으로 갔을 때는 그 탭만) |
| G4 | `updateAt`이 블록마다 단조 증가 | **보장하지 않는다.** `updateAt`은 쓸 때의 서버 시각(밀리초)이다. 같은 밀리초의 두 쓰기는 같은 값이고, 판을 가르는 다른 값(카운터)은 없다 |
| G5 | 남이 본문을 고치면 카드의 요약이 온다 | 지금은 본문 저장이 없다(다음 단계). 지금의 `UPDATE_BLOCK`은 바뀐 블록 전체를 싣는다. 본문이 들어간 뒤의 모양은 `docs/contract-pending.md` 1절에 적는다 |
| G6 | 재연결 때 놓친 변경은 화면이 보드를 다시 읽는다 | 맞다. 서버는 놓친 구간을 다시 보내지 않는다. 순번이 없어 무엇을 놓쳤는지도 알 수 없다 |

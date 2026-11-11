# WebSocket `/ws`의 메시지

적은 때: 2026-11-11 04:14 KST. 서버가 **지금** 주고받는 것을 모양을 바꾸지 않고 적었다(`crates/server/src/ws.rs`, 핸드셰이크는 `crates/server/src/lib.rs`의 `handle_websocket`). OpenAPI는 WebSocket을 적지 못하므로 계약 파일 `contracts/openapi.json` 밖에 둔다.

- 메시지마다의 멤버와 타입은 기계가 읽는 `contracts/websocket.json`에 있다. 서버의 테스트(`crates/server/tests/api/websocket.rs`의 `messages_match_the_websocket_contract`)가 서버가 실제로 보낸 메시지를 그 파일과 견준다: 맨 위 멤버가 정확히 그 이름들이고, 실린 객체(`board`, `block` 등)의 멤버는 계약 파일의 같은 이름의 스키마에 있는 것들이다. 테스트가 보는 것은 **메시지**뿐이다. 명령(2절)과 `session`·`sent` 표시는 이 문서와 코드를 사람이 맞춰 둔다. 실린 객체의 멤버는 이름만 견주고 타입은 계약 파일의 스키마를 따른다.
- 메시지가 싣는 보드, 블록, 멤버, 카테고리는 계약의 `Board`, `Block`, `BoardMember`, `Category`와 같은 모양이다. `Block`은 본문(`body`, `docs/contract-pending.md` 1절)을 싣는다.
- 이 문서의 모양을 바꾸는 일은 계약을 바꾸는 일이다. 바꿀 때는 이 문서, `websocket.json`, `docs/contract-pending.md`를 같은 커밋에서 고친다.

## 1. 핸드셰이크와 인증

주소는 `{base}/ws`이고 `GET`의 WebSocket 업그레이드만 받는다. 그 밖의 메서드는 405(`Allow`에 `GET`이 있다. `HEAD`가 함께 있을 수 있다), 업그레이드가 아닌 `GET`은 400이다. 둘 다 API의 오류 본문(`application/problem+json`의 `Problem`)이다.

누구의 연결인지는 **핸드셰이크에서 한 번** 정해진다. 연결한 뒤에 로그인하는 메시지는 없다(옛 `AUTH`는 모르는 명령으로 무시된다).

| 핸드셰이크 | 결과 |
|---|---|
| `Origin`이 있고 서버의 출처(`serverRoot`의 출처나 설정 `allowed_origins`)가 아니다 | 403 `forbidden`, `reason: foreign_origin`. 연결되지 않는다 |
| `Authorization: Bearer <세션 토큰>`이 있다 | 세션이면 그 사용자의 연결. 세션이 아니면 401 |
| `Origin`이 있고 세션 쿠키(`fvoci_session`, HTTPS에서 `__Host-fvoci_session`)가 있다 | 세션이면 그 사용자의 연결. 끝난 세션의 쿠키면 **익명 연결**(401이 아니다. 로그아웃된 브라우저도 공유 보드를 따라간다) |
| `Origin`이 있고 자격이 없다 | 익명 연결 |
| `Origin`이 없고 Bearer도 없다(브라우저가 아닌 클라이언트) | 401 |

- 둘 다 오면 Bearer를 쓴다.
- 브라우저는 같은 출처의 페이지에서 `new WebSocket(…)`으로 열면 쿠키를 저절로 싣는다. 스크립트가 토큰을 알 필요가 없다.
- 익명 연결은 공유 보드의 블록 구독(`SUBSCRIBE_BLOCKS`)만 할 수 있다.
- **공유 보드는 설정 `enable_public_shared_boards`가 켜져 있을 때만 있다. 기본값은 꺼짐이고, 꺼져 있으면 어느 읽기 토큰도 맞지 않아 `SUBSCRIBE_BLOCKS`는 늘 무시된다.**

## 2. 클라이언트가 보내는 명령

명령은 텍스트 프레임의 JSON 객체 하나다. 멤버는 camelCase이고 없는 멤버는 빈 값으로 읽는다. **명령에는 답이 없다.** 받아들였는지, 거절했는지, 모르는 명령인지 알리는 메시지가 없다(서버의 로그에만 남는다). 그래서 구독이 됐는지는 그 뒤에 오는 변경으로만 안다.

| `action` | 세션 | 멤버 | 하는 일 |
|---|---|---|---|
| `SUBSCRIBE_WORKSPACE` | 필요 | `workspaceId` | 그 워크스페이스의 변경을 받는다(3절). 같은 워크스페이스를 다시 구독해도 한 번이다 |
| `UNSUBSCRIBE_WORKSPACE` | 필요 | `workspaceId` | 그만 받는다 |
| `SUBSCRIBE_BLOCKS` | 없어도 된다 | `workspaceId`, `readToken`, `blockIds` | 공유 보드의 블록들을 읽기 토큰으로 따라간다. `blockIds`가 모두 한 보드의 것이고 `readToken`이 그 보드의 공유 토큰이며 `workspaceId`가 비어 있지 않아야 한다. 하나라도 어긋나면 명령 전체를 무시한다 |
| `UNSUBSCRIBE_BLOCKS` | 없어도 된다 | 같다 | 그 블록들을 그만 따라간다. 같은 검사를 한다 |

- 세션이 필요한 명령을 익명 연결이 보내면 무시한다.
- JSON이 아니거나, 멤버의 타입이 틀리거나(`workspaceId`가 수, `blockIds`가 목록이 아님 등), `action`을 모르면 그 명령만 무시하고 연결은 그대로다.
- `SUBSCRIBE_WORKSPACE`는 그 워크스페이스에서 일하는 사용자(손님이 아닌 구성원)의 것만 받아들이고, 그 밖의 것은 무시한다. 워크스페이스 ID는 `GET /api/v1/users/me/workspaces`에 있다.
- `blockIds`가 빈 목록이면 보드를 정할 수 없어 읽기 토큰이 맞지 않는 것으로 친다.

## 3. 서버가 보내는 메시지

메시지는 텍스트 프레임의 JSON 객체 하나이고 `action`이 종류다. 그 밖에 서버는 설정 `websocket_ping_seconds`(기본 30초, 0이면 보내지 않음)마다 Ping 프레임을 보낸다. 첫 Ping은 연결하자마자 간다. 클라이언트(브라우저)가 Pong으로 답하지만 서버는 Pong을 확인하지 않는다.

| `action` | 싣는 것 | 누가 받나 |
|---|---|---|
| `UPDATE_BOARD` | `workspaceId`, `board`(`Board` 전체) | 그 워크스페이스를 구독한 연결 가운데 사용자가 보드의 멤버인 것 |
| `UPDATE_BLOCK` | `workspaceId`, `block`(`Block` 전체) | 위와 같은 연결, 그리고 그 블록이나 그 블록의 부모를 `SUBSCRIBE_BLOCKS`로 따라가는 연결 |
| `UPDATE_MEMBER` | `workspaceId`, `member`(`BoardMember`) | 그 워크스페이스를 구독한 연결 가운데 사용자가 보드의 멤버인 것 |
| `DELETE_MEMBER` | `workspaceId`, `member`(`boardId`, `userId`만 뜻이 있고 역할은 모두 `false`) | 위와 같은 연결, 그리고 빠진 사용자의 연결 |
| `UPDATE_CATEGORY` | `workspaceId`, `category`(`Category`) | 카테고리 주인의, 그 워크스페이스를 구독한 연결 모두(아래) |
| `UPDATE_BOARD_CATEGORY` | `workspaceId`, `boardCategories`(`{boardId, categoryId, hidden}`의 목록). **목록은 바꾼 항목들이지 전체가 아니다**: `setBoardCategory`와 보드를 기본 카테고리에 넣는 조작은 그 카테고리의 보드 전부를, `hideBoard`·`unhideBoard`·`undeleteBoard`는 항목 하나를 싣는다. 받는 쪽은 항목마다 덮어쓴다(목록으로 갈아 끼우면 보드를 잃는다) | 그 사용자의 연결 모두 |
| `REORDER_CATEGORIES` | `workspaceId`, `categoryOrder`(카테고리 ID의 새 순서) | 그 사용자의 연결 모두 |
| `REORDER_CATEGORY_BOARDS` | `workspaceId`, `categoryId`, `boardOrder`(그 카테고리의 보드 ID의 새 순서) | 그 사용자의 연결 모두 |
| `UPDATE_CLIENT_CONFIG` | `clientConfig` | 모든 연결. **지금은 보내는 곳이 없다**(서버 설정이 실행 중에 바뀌지 않는다) |

### 받는 사람의 규칙

- "보드의 멤버"는 메시지를 보낼 때 저장소에서 읽은 멤버 목록이다. 워크스페이스를 구독하지 않은 연결은 멤버여도 받지 않는다.
- 공개 보드라도 멤버가 아닌 사용자는 보드와 블록의 변경을 받지 않는다.
- **카테고리의 네 메시지는 그 사용자의 연결 가운데 그 워크스페이스를 구독한 것 모두에 간다.** 같은 사용자의 다른 탭도 받는다(원본은 먼저 구독한 하나에만 보냈다).
- `SUBSCRIBE_BLOCKS`로 따라가는 연결 가운데 세션이 없거나 사용자가 보드의 멤버(보기 역할부터)가 아닌 것이 받는 `UPDATE_BLOCK`은 HTTP의 `getBlocks`가 그 독자에게 주는 모양이다(`docs/contract-pending.md` 1절): `createdBy`, `modifiedBy`, 본문의 모든 멘션의 `userId`, 사람 속성이 가리키는 사람이 비어 있다. 사용자 ID가 가입 시각을 드러낸다. 멤버의 연결은 읽기 토큰으로 따라가도 그대로 받는다.
- 한 연결이 워크스페이스 구독과 블록 구독을 함께 해서 두 길로 다 해당하면 같은 메시지를 두 번 받는다.
- 쓴 사람 자신의 연결도 받는다. 서버는 보낸 사람을 빼지 않는다.

### 메시지를 내는 조작

| 조작(계약의 `operationId`) | 메시지 |
|---|---|
| `createBoard`, `createBoardsAndBlocks`, `duplicateBoard`(템플릿에서 만들기 포함), `onboard` | 새 보드마다 `UPDATE_BOARD`, 블록마다 `UPDATE_BLOCK`, 멤버마다 `UPDATE_MEMBER`. 템플릿이 아닌 새 보드는 만든 사람의 기본 카테고리에 들어가며 `UPDATE_BOARD_CATEGORY`를 낸다. `onboard`는 보드를 고친 뒤 `UPDATE_BOARD`를 한 번 더 낸다 |
| `importArchive` | 보드·블록의 `UPDATE_BOARD`·`UPDATE_BLOCK`이 멤버가 생기기 전에 나가 **아무도 받지 않는다**. 그 뒤 `UPDATE_MEMBER`, `UPDATE_BOARD_CATEGORY`, 이미지를 고친 블록의 `UPDATE_BLOCK` |
| `patchBoard` | `UPDATE_BOARD`. **템플릿**의 패치에 `type`이 있으면(바뀌지 않았어도) 멤버가 아닌 워크스페이스 사용자마다: 비공개면 `DELETE_MEMBER`, 공개면 보기 역할의 `UPDATE_MEMBER`. 템플릿이 아닌 보드는 종류가 바뀌어도 `UPDATE_BOARD`뿐이다 |
| `patchBoardsAndBlocks` | 바뀐 블록마다 `UPDATE_BLOCK`, 바뀐 보드마다 `UPDATE_BOARD` |
| `reorderCardProperties` | 새 순서의 `cardProperties`를 실은 `UPDATE_BOARD` |
| `deleteBoard`, `deleteBoardsAndBlocks` | `UPDATE_BOARD`, `board`는 `id`, `workspaceId`, `updateAt`, `deleteAt`(0이 아님)만 뜻이 있고 나머지는 빈 값. 받는 사람은 지우기 직전의 멤버. `deleteBoardsAndBlocks`는 그 앞에 블록마다 삭제 `UPDATE_BLOCK`(모양은 아래 `deleteBlock`)을 내는데, 이것도 지우기 직전의 멤버에게만 가고 `SUBSCRIBE_BLOCKS`로 따라가던 연결은 받지 않는다. 둘 다 보드의 공유 토큰으로 따라가던 연결을 닫는다 |
| `undeleteBoard` | 돌아온 보드의 `UPDATE_BOARD` 하나, 그리고 보드를 사이드바에 다시 둔 사용자마다 그 사람의 연결에 `UPDATE_BOARD_CATEGORY`(그 보드의 항목 하나: 원래 카테고리, 그사이 지워졌으면 기본 카테고리)와 그 카테고리의 `REORDER_CATEGORY_BOARDS`(원래 자리로 들어가며 뒤의 보드가 밀린 순서). 비공개 보드는 다시 구성원이 된 사람에게만. **함께 돌아온 블록과 멤버의 메시지는 없다** |
| `createBlocks`, `patchBlock`, `patchBlocks`, `createCard`, `patchCard`, `duplicateBlock`, `undeleteBlock` | 바뀌거나 생긴 블록마다 `UPDATE_BLOCK`. `undeleteBlock`은 되살린 블록 하나만 알린다(함께 돌아온 아래 블록은 알리지 않는다) |
| `deleteBlock` | `UPDATE_BLOCK`, `block`은 `id`, `boardId`, `updateAt`, `deleteAt`(0이 아님)만 뜻이 있고 나머지는 빈 값. **지운 블록 하나만** 알린다. 함께 휴지통에 간 아래 블록(카드의 댓글·첨부)은 따로 알리지 않는다 |
| `addMember`, `joinBoard`, `updateMember` | `UPDATE_MEMBER`. 이미 멤버인 사용자의 `addMember`·`joinBoard`, 멤버가 아닌 사용자의 `updateMember`는 아무것도 내지 않는다. 템플릿이 아닌 보드에 새로 든 멤버는 기본 카테고리에 들어가며 `UPDATE_BOARD_CATEGORY`도 받는다 |
| `removeMember`, `leaveBoard` | `DELETE_MEMBER` |
| `createCategory`, `updateCategory`, `deleteCategory` | `UPDATE_CATEGORY`. 지운 카테고리는 `deleteAt`이 0이 아니다. `deleteCategory`는 그 보드들을 기본 카테고리로 옮기며 `UPDATE_BOARD_CATEGORY`도 낸다 |
| `getCategories`와 기본 카테고리를 쓰는 위의 조작 | 사용자에게 기본 카테고리가 아직 없으면 만들면서 `UPDATE_CATEGORY`와 `UPDATE_BOARD_CATEGORY`를 낸다(읽기인 `GET`도) |
| `reorderCategories` | `REORDER_CATEGORIES` |
| `setBoardCategory`, `hideBoard`, `unhideBoard` | `UPDATE_BOARD_CATEGORY` |
| `reorderCategoryBoards` | `REORDER_CATEGORY_BOARDS` |
| `setSharing`(끄기, 새 토큰) | 메시지 없음. 옛 토큰으로 따라가던 연결이 닫힌다 |
| `patchUserSettings`, `patchClientState` | 메시지 없음 |

메시지는 쓰기가 끝난 뒤 보낸다. 어떤 조작은 답하기 전에 보내고(`createBoardsAndBlocks`, `createBlocks`, `createCard`, `deleteBoard`, `deleteBoardsAndBlocks`, 카테고리의 메시지 모두), 나머지는 따로 돌며 보낸다. 클라이언트는 HTTP의 답과 메시지의 순서에 기대지 않는다.

### 서버가 연결을 닫을 때

닫을 때 서버는 닫는 프레임을 보내고 이유를 싣지 않는다. 다만 두 경우에는 닫는 프레임 없이 끊는다: 핸드셰이크와 연결 등록 사이에 세션이 끝난 때, 그리고 읽지 않는 상대에게 남은 메시지를 5초 안에 보내지 못한 때. 클라이언트는 어느 쪽이든 다시 연결하고 보드를 다시 읽는다.

| 언제 | 닫히는 연결 |
|---|---|
| 로그아웃 | 그 세션으로 연 연결 |
| 관리자가 비밀번호를 재설정 | 그 사용자의 모든 연결 |
| 자기 비밀번호를 바꿈 | 그 사용자의, 바꾼 세션이 아닌 세션의 연결 |
| 핸드셰이크 직후, 연결을 등록하기 전에 세션이 끝나 있음 | 그 연결(닫는 프레임 없이) |
| 세션이 한가함 한도나 절대 한도로 끝나거나 사용자가 지워짐 | 그 세션의 연결. 서버가 Ping마다(`websocket_ping_seconds`, Ping이 꺼져 있으면 1분마다) 세션을 다시 보고 닫는다 |
| 공유를 끄거나 새 토큰을 내거나 `deleteBoard`, `deleteBoardsAndBlocks`로 보드를 지움 | 그 보드의 옛 읽기 토큰으로 블록을 따라가던 연결 |
| 보내지 못한 메시지가 512개 쌓임 | 그 연결(읽지 않는 클라이언트) |
| 받은 메시지가 64 KiB를 넘음 | 그 연결 |
| 서버가 멈춤 | 모든 연결 |

## 4. 알아 둘 것

- **놓친 변경을 다시 보내지 않는다.** 순번도 판 번호도 없다. 다시 연결한 클라이언트는 필요한 것을 HTTP로 다시 읽는다.
- **`updateAt`은 서버의 밀리초 시각이다.** 있는 블록을 고칠 때마다 블록의 `updateAt`은 앞의 값보다 커진다(같은 밀리초의 쓰기나 시계가 뒤로 간 때에는 앞의 값에 1을 더한다). 그래서 `patchCard`의 `readAt`이 그 사이의 변경을 놓치지 않는다. 휴지통에서 돌아온 블록은 돌아온 때의 서버 시각을 받는다. 보드, 멤버, 카테고리의 `updateAt`에는 이런 보장이 없다.
- 메시지는 보낸 순서대로 한 연결에 도착한다. 다른 쓰기의 메시지 사이의 순서는 쓰기가 끝난 순서와 다를 수 있다(메시지를 따로 돌며 보낸다).
- 서버는 64 KiB를 넘는 프레임과 메시지를 **받지** 않는다. 서버가 보내는 메시지에는 한도가 없다(큰 블록의 `UPDATE_BLOCK`도 그대로 간다).

## 5. 웹앱의 질문 G1–G6에 대한 답

`apps/web/docs/contract-assumptions.md`의 "실시간 변경" 표의 질문에, 지금 서버가 하는 일로 답한다. 바꾸자는 권고는 웹앱의 질문 전체에 답하는 일(작업 묶음의 7단계)에서 `docs/contract-pending.md`에 적고, 메시지의 모양이나 뜻을 바꾸는 것은 "사용자 결정 필요"로 남긴다.

| # | 웹앱의 가정 | 지금 서버 |
|---|---|---|
| G1 | 구독은 둘: 보드 하나(`watchBoard`), 나 자신(`watchSelf`) | 구독의 단위는 **워크스페이스**(`SUBSCRIBE_WORKSPACE`)와 **블록**(`SUBSCRIBE_BLOCKS`, 읽기 토큰으로)이다. 보드 하나만 구독하는 명령은 없다. 워크스페이스를 구독하면 내가 멤버인 모든 보드의 보드·블록·멤버 변경과 내 카테고리의 변경이 온다. 그러니 `watchBoard`와 `watchSelf`는 연결 하나의 워크스페이스 구독에서 화면이 보드 ID로 갈라 쓴다. 설정(`patchUserSettings`)의 변경은 메시지가 없다. 카테고리 메시지는 그 워크스페이스를 구독한 내 연결 모두에 간다(3절) |
| G2 | 변경의 종류 일곱 | 보드: `UPDATE_BOARD`(전체). 보드 삭제: `deleteAt`이 0이 아닌 `UPDATE_BOARD`(따로 된 종류가 없다). 블록: `UPDATE_BLOCK`이 **블록 하나씩**, `Block` 전체를 싣는다. 카드도 요약이 아니라 블록 전체이고 본문(최대 1 MiB)을 함께 싣는다. 블록 삭제: `deleteAt`이 0이 아닌 `UPDATE_BLOCK`, 지운 블록 하나만(아래 것은 싣지 않는다). 구성원: 전체 목록이 아니라 멤버 한 명씩(`UPDATE_MEMBER`, `DELETE_MEMBER`). 카테고리: 전체 목록이 아니라 카테고리 하나(`UPDATE_CATEGORY`), 보드의 자리(`UPDATE_BOARD_CATEGORY`), 순서(`REORDER_CATEGORIES`, `REORDER_CATEGORY_BOARDS`). 설정: 메시지 없음 |
| G3 | 내 쓰기도 내게 온다 | 맞다. 보낸 연결도 받는다 |
| G4 | `updateAt`이 블록마다 단조 증가 | **있는 블록을 고칠 때는 그렇다**: 고칠 때마다 앞의 값보다 커진다(4절). 휴지통에서 돌아온 블록은 그때의 서버 시각을 받으므로, 시계가 뒤로 갔다면 지우기 전보다 작을 수 있다. 판을 가르는 다른 값(카운터)은 없다 |
| G5 | 남이 본문을 고치면 카드의 요약이 온다 | **요약이 아니라 블록 전체가 온다.** 카드의 본문이 저장된 뒤로(`docs/contract-pending.md` 1절) `UPDATE_BLOCK`의 `block`은 `body`(문서 하나, 최대 1 MiB)를 싣는다. 할 일 수는 싣지 않는다(`Block`에 없다. 받는 쪽이 본문에서 센다). 요약만 보내자는 것은 메시지의 모양을 바꾸는 일이라 사용자 결정 필요(`docs/contract-pending.md` 9절의 G5) |
| G6 | 재연결 때 놓친 변경은 화면이 보드를 다시 읽는다 | 맞다. 서버는 놓친 구간을 다시 보내지 않는다. 순번이 없어 무엇을 놓쳤는지도 알 수 없다 |

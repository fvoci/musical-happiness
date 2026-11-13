# WebSocket `/ws`의 메시지

적은 때: 2026-11-11 04:14 KST(본문의 공동 편집, 6절은 2026-11-13 04:41 KST). 서버가 **지금** 주고받는 것을 모양을 바꾸지 않고 적었다(`crates/server/src/ws.rs`, 핸드셰이크는 `crates/server/src/lib.rs`의 `handle_websocket`). OpenAPI는 WebSocket을 적지 못하므로 계약 파일 `contracts/openapi.json` 밖에 둔다.

- 메시지마다의 멤버와 타입은 기계가 읽는 `contracts/websocket.json`에 있다. 서버의 테스트(`crates/server/tests/api/websocket.rs`의 `messages_match_the_websocket_contract`, 본문의 공동 편집(6절)의 `DOC_…`은 `rooms_contract.rs`의 `the_messages_of_the_rooms_match_the_websocket_contract`)가 서버가 실제로 보낸 메시지를 그 파일과 견준다: 맨 위 멤버가 정확히 그 이름들이고, 실린 객체(`board`, `block` 등)의 멤버는 계약 파일의 같은 이름의 스키마에 있는 것들이다. 테스트가 보는 것은 **메시지**뿐이다. 명령(2절)과 `session`·`sent` 표시는 이 문서와 코드를 사람이 맞춰 둔다. 실린 객체의 멤버는 이름만 견주고 타입은 계약 파일의 스키마를 따른다. Binary 프레임(6절)의 바이트는 `crates/server/src/collab/frame.rs`의 테스트와 `rooms.rs`의 테스트가 따로 쓴 부호화로 견준다.
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

명령은 텍스트 프레임의 JSON 객체 하나다. 멤버는 camelCase이고 없는 멤버는 빈 값으로 읽는다. **구독 명령에는 답이 없다.** 받아들였는지, 거절했는지, 모르는 명령인지 알리는 메시지가 없다(서버의 로그에만 남는다). 그래서 구독이 됐는지는 그 뒤에 오는 변경으로만 안다. 답이 있는 것은 `JOIN_DOC`뿐이다(6절).

| `action` | 세션 | 멤버 | 하는 일 |
|---|---|---|---|
| `SUBSCRIBE_WORKSPACE` | 필요 | `workspaceId` | 그 워크스페이스의 변경을 받는다(3절). 같은 워크스페이스를 다시 구독해도 한 번이다 |
| `UNSUBSCRIBE_WORKSPACE` | 필요 | `workspaceId` | 그만 받는다 |
| `SUBSCRIBE_BLOCKS` | 없어도 된다 | `workspaceId`, `readToken`, `blockIds` | 공유 보드의 블록들을 읽기 토큰으로 따라간다. `blockIds`가 모두 한 보드의 것이고 `readToken`이 그 보드의 공유 토큰이며 `workspaceId`가 비어 있지 않아야 한다. 하나라도 어긋나면 명령 전체를 무시한다 |
| `UNSUBSCRIBE_BLOCKS` | 없어도 된다 | 같다 | 그 블록들을 그만 따라간다. 같은 검사를 한다 |
| `JOIN_DOC` | 필요 | `workspaceId`, `cardId` | 카드 본문을 함께 고치는 방에 들어간다(6절). 답은 `DOC_JOINED`나 `DOC_REFUSED` |
| `LEAVE_DOC` | 필요 | `doc` | `DOC_JOINED`가 알려 준 번호의 방에서 나온다. 답이 없다 |

- 세션이 필요한 명령을 익명 연결이 보내면 무시한다(`JOIN_DOC`은 `DOC_REFUSED`로 답한다, 6절).
- JSON이 아니거나, 멤버의 타입이 틀리거나(`workspaceId`가 수, `blockIds`가 목록이 아님 등), `action`을 모르면 그 명령만 무시하고 연결은 그대로다.
- `SUBSCRIBE_WORKSPACE`는 그 워크스페이스에서 일하는 사용자(손님이 아닌 구성원)의 것만 받아들이고, 그 밖의 것은 무시한다. 워크스페이스 ID는 `GET /api/v1/users/me/workspaces`에 있다.
- `blockIds`가 빈 목록이면 보드를 정할 수 없어 읽기 토큰이 맞지 않는 것으로 친다.
- 비밀번호를 받지 않는 조직(공급자의 `ssoRequired`)의 워크스페이스는 그 공급자로 만든 세션의 연결만 구독한다. 다른 세션의 `SUBSCRIBE_WORKSPACE`는 무시한다(HTTP의 401 `sso_required`에 해당한다. `getMyWorkspaces`의 `ssoRequired`가 미리 알려 준다). 조직이 `ssoRequired`를 켜면, 그 조직의 워크스페이스를 구독하던 연결 가운데 그 공급자로 만들지 않은 세션의 것을 서버가 닫는다. 조직이 다시 비밀번호를 받게 돼도 무시된 구독이 저절로 살아나지는 않는다: 다시 구독한다.

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
- `SUBSCRIBE_BLOCKS`로 따라가는 연결 가운데 세션이 없거나 사용자가 보드의 멤버(보기 역할부터)가 아닌 것이 받는 `UPDATE_BLOCK`은 HTTP의 `getBlocks`가 그 독자에게 주는 모양이다(`docs/contract-pending.md` 1절): `createdBy`, `modifiedBy`, 본문의 모든 멘션의 `userId`, 사람 속성이 가리키는 사람이 비어 있다. 사용자 ID가 가입 시각을 드러낸다. 멤버의 연결은 읽기 토큰으로 따라가도 그대로 받는다. 다만 그 세션이 보드의 조직을 열지 못하면(조직이 `ssoRequired`이고 세션이 그 공급자의 것이 아님) 독자의 모양으로 받는다: 메시지를 보낼 때마다 본다.
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
| `addTeamMember`, `removeTeamMember`, `deleteTeam`, `setBoardTeam`, `removeBoardTeam` | 그 팀이 걸린 보드와 그 팀의 사람마다 `DELETE_MEMBER`(역할이 생기거나 오른 때에도): 받은 클라이언트는 보드의 멤버를 다시 읽는다. 팀이 걸린 보드가 없으면 아무것도 내지 않는다 |
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
| 받은 텍스트 메시지가 64 KiB를 넘거나, Binary 프레임이 64 KiB(설정 `collab_update_bytes`에 32바이트를 더한 것이 더 크면 그것)를 넘음 | 그 연결 |
| 방(6절)이 보낸 것을 전혀 읽지 않아, 방에서 내보냈다는 알림만으로 기다리는 메시지가 `collab_queue_messages`의 두 배(기본 1,024개)를 넘음 | 그 연결 |
| 서버가 멈춤 | 모든 연결 |

## 4. 알아 둘 것

- **놓친 변경을 다시 보내지 않는다.** 순번도 판 번호도 없다. 다시 연결한 클라이언트는 필요한 것을 HTTP로 다시 읽는다.
- **`updateAt`은 서버의 밀리초 시각이다.** 있는 블록을 고칠 때마다 블록의 `updateAt`은 앞의 값보다 커진다(같은 밀리초의 쓰기나 시계가 뒤로 간 때에는 앞의 값에 1을 더한다). 그래서 `patchCard`의 `readAt`이 그 사이의 변경을 놓치지 않는다. 휴지통에서 돌아온 블록은 돌아온 때의 서버 시각을 받는다. 보드, 멤버, 카테고리의 `updateAt`에는 이런 보장이 없다.
- 메시지는 보낸 순서대로 한 연결에 도착한다. 다른 쓰기의 메시지 사이의 순서는 쓰기가 끝난 순서와 다를 수 있다(메시지를 따로 돌며 보낸다).
- 서버는 64 KiB를 넘는 프레임과 메시지를 **받지** 않는다(Binary 프레임은 `collab_update_bytes`를 64 KiB 가까이나 그보다 크게 잡았으면 그 값에 32바이트를 더한 것까지 받는다). 서버가 보내는 메시지에는 한도가 없다(큰 블록의 `UPDATE_BLOCK`도, 문서 전체를 싣는 Binary 프레임도 그대로 간다).

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


## 6. 본문의 공동 편집: 방

카드의 본문을 함께 고치는 연결들이 그 카드의 **방**에 든다. 서버는 본문을 Y 문서 하나로 쥐고(`docs/configuration.md`의 "본문의 공동 편집"), 방의 연결들과 그 문서의 변경을 주고받는다. 제어는 텍스트 메시지, 문서의 내용은 Binary 프레임이다. 구독(2–3절)과 같은 연결을 쓰고, 한 연결이 방 여럿에 들 수 있다. 본문이 카드에 쓰이면 `UPDATE_BLOCK`(3절)이 지금처럼 나간다: 방에 든 연결도 워크스페이스를 구독했으면 그것을 함께 받는다.

### 들어오기와 나가기

`JOIN_DOC`(`workspaceId`, `cardId`)에 서버는 하나로 답한다.

| 답 | 멤버 | 뜻 |
|---|---|---|
| `DOC_JOINED` | `workspaceId`, `cardId`, `doc`, `epoch`, `mode` | 방에 들었다. `doc`은 이 연결에서 그 방을 가리키는 번호(1부터, 들어올 때마다 새 번호)이고 뒤의 메시지와 프레임이 모두 이 번호로 방을 가리킨다. `epoch`은 문서의 세대(아래), `mode`는 `write`나 `read`. **그 방의 다음 Binary 프레임이 문서 전체다**(sync step 2. 그 사이에 다른 방이나 구독의 메시지가 낄 수 있다) |
| `DOC_REFUSED` | `workspaceId`, `cardId`, `reason` | 들지 못했다. `reason`은 아래 표 |

- **누가 드나.** 카드의 보드에서 카드를 고칠 수 있으면(편집 역할부터) `write`, 보드를 읽을 수만 있으면(보기·댓글 역할, 열린 템플릿을 읽는 워크스페이스 구성원) `read`로 든다. 그 밖의 사용자, 세션 없는 연결(공유 보드를 읽기 토큰으로 따라가는 연결도), 비밀번호를 받지 않는 조직(`ssoRequired`)의 카드에 그 공급자로 만들지 않은 세션의 연결(HTTP의 401 `sso_required`에 해당한다), 없는 카드, 카드가 아닌 블록, `workspaceId`가 카드의 보드가 있는 워크스페이스가 아닌 것은 모두 `reason`이 `not_found`인 같은 `DOC_REFUSED`다. `workspaceId`나 `cardId`가 64바이트를 넘는 `JOIN_DOC`은 답 없이 버린다. 들 수 있는지는 카드가 어느 보드의 것인지만 읽어 가른다: 본문은 들인 뒤에 읽으므로 거절은 본문의 크기와 상관없이 같은 일을 한다.
- **클라이언트는 빈 문서로 든다.** 문서는 서버가 카드의 본문에서 만든다. 클라이언트가 본문에서 스스로 만든 문서는 서버의 것과 다른 문서여서 합치면 내용이 두 번 든다.
- 이미 든 카드에 다시 `JOIN_DOC`을 보내면 새 번호로 새로 들고(문서 전체를 다시 받는다) 앞의 번호는 더 방이 아니다(그 번호의 기다리던 Binary 프레임은 버려진다). 거절되면 앞의 것이 그대로다.
- `LEAVE_DOC`(`doc`)으로 나온다. 연결이 끊겨도 나온다. 나온 뒤에도 그 번호로 이미 보낸 것이 조금 더 올 수 있다: 모르는 번호의 메시지와 프레임은 버린다.
- 마지막 연결이 나가면 서버가 문서를 닫으며 본문을 카드에 쓴다. 그 전에도 마지막 변경 2초 뒤(늦어도 첫 변경 10초 뒤)에 쓴다.

### Binary 프레임

양쪽이 같은 모양으로 보낸다. 수는 lib0의 varuint(바이트마다 7비트, 낮은 자리부터, 마지막이 아닌 바이트는 맨 윗비트가 1)다.

| 차례 | 무엇 |
|---|---|
| varuint `doc` | 어느 방인가(`DOC_JOINED`의 `doc`) |
| varuint 종류 | `0`: sync 메시지. 지금은 이것뿐이고 다른 값의 프레임은 버린다 |
| varuint `epoch` | 이 메시지가 문서의 어느 세대의 것인가 |
| varuint sync 종류 | `0`: step 1(상태 벡터), `1`: step 2(받는 쪽에 없는 것), `2`: update |
| varuint 길이, 그만큼의 바이트 | 상태 벡터(step 1)나 업데이트(step 2, update). Yjs의 version 1 부호화. 프레임은 여기서 끝난다 |

sync 종류부터 끝까지는 y-protocols의 sync 메시지 그대로다(`writeSyncStep1`, `writeSyncStep2`, `writeUpdate`). 예: `doc` 3, `epoch` 300, 두 바이트 `aa bb`의 update는 `03 00 ac 02 02 02 aa bb`.

| 누가 | 프레임 | 서버가 하는 일 |
|---|---|---|
| 서버 | step 2 | `DOC_JOINED`와 `DOC_RESET` 뒤 그 방의 첫 프레임: 문서 전체. step 1의 답: 그 상태 벡터에 없는 것 |
| 서버 | update | 다른 연결의 변경, 그리고 서버가 고친 것(아래) |
| 클라이언트 | step 1 | 그 연결에만 step 2로 답한다. 세대가 다르거나 상태 벡터가 아니면 답하지 않는다. `read`도 보낼 수 있다 |
| 클라이언트 | update, step 2 | 둘 다 변경으로 받는다(아래). 답은 `DOC_ACK`나 `DOC_NACK` |

서버는 step 1을 보내지 않는다: 클라이언트는 서버에 없는 것을 갖고 들어오지 않는다(오프라인 편집이 없다). 이 모양이 아닌 프레임(끝나지 않는 수, 길이가 남은 바이트와 다름)과 들지 않은 방의 프레임은 답 없이 버린다.

### 변경의 순서와 답

| 메시지 | 멤버 | 뜻 |
|---|---|---|
| `DOC_ACK` | `doc`, `epoch`, `seq` | 보낸 변경을 받아들였다. `seq`는 문서의 기록에서 그 변경의 순번 |
| `DOC_NACK` | `doc`, `epoch`, `reason` | 보낸 변경을 받아들이지 않았다. 문서는 그대로이고 누구에게도 가지 않았다 |
| `DOC_RESET` | `doc`, `epoch` | 문서를 다시 보낸다. **그 방의 다음 Binary 프레임이 문서 전체다**(step 2. 세대, 아래) |
| `DOC_MODE` | `doc`, `mode` | 이 연결이 이제부터 `write`나 `read`다(아래 "방에 있는 동안") |
| `DOC_LEFT` | `doc`, `reason` | 서버가 이 연결을 방에서 내보냈다. 그 번호는 더 방이 아니다. 그 방에서 이미 정해진 답(`DOC_JOINED`, `DOC_ACK`, `DOC_NACK`, `DOC_MODE`)은 이보다 먼저 오고, 기다리던 그 방의 Binary 프레임은 버려진다. 보낼 줄이 차서 내보내는 때에도 그렇다: 기록에 적힌 변경의 `DOC_ACK`는 줄이 찼어도 실리고 그 뒤에 `DOC_LEFT`가 온다 |

- **기록이 먼저다.** 변경은 서버가 문서에 넣고 기록(DB)에 적은 뒤에만 나간다: 보낸 연결에 `DOC_ACK`, 방의 다른 연결에 update 프레임. 보낸 연결에는 자기 변경이 돌아오지 않는다. 방의 연결들은 변경을 기록의 순서대로 받는다.
- **변경 프레임 하나에 답 하나.** 방에 든 연결이 보낸 update·step 2 프레임마다 `DOC_ACK`나 `DOC_NACK`가 보낸 순서대로 하나씩 온다. `epoch`은 그 프레임이 실은 값 그대로다. `DOC_LEFT`를 받은 방의 것은 더 오지 않는다.
- **서버가 고친 것.** 받아들인 변경이 문서를 본문이 아닌 것으로 만들면 서버가 그만큼 지우고, 그 지움을 update 프레임으로 **보낸 연결을 포함한 모두에게** 보낸다(`DOC_ACK` 뒤에).
- **`DOC_NACK` 뒤.** 받아들여지지 않은 변경은 보낸 편집기의 문서에만 남아 있다. 그 편집기는 방에 **한 번** 다시 들어(`JOIN_DOC`) 문서를 새로 받는다(`DOC_NACK`마다가 아니다: `JOIN_DOC`은 프레임 다섯으로 센다). 그대로 두면 그 위에 얹은 다음 변경도 `update_out_of_step`으로 거절된다. 다시 들지 않아도 되는 때: `epoch`이 이미 받은 `DOC_RESET`의 것보다 옛것인 `DOC_NACK`는 버린 문서의 답이므로 무시하고, 쥐고 있는 `epoch`의 `update_out_of_step`은 문서가 방금 새로 만들어진 것일 수 있으니 곧 올 `DOC_RESET`을 잠깐 기다린 뒤에 든다.

### 방에 있는 동안

구성원·역할·보드 공개 범위·조직 관리자 참여·워크스페이스의 사용 여부·팀이 바뀌는 조작은 답하기 전에 해당 방들의 권한을 다시 묻는다. 팀의 조작(`addTeamMember`, `removeTeamMember`, `deleteTeam`, `setBoardTeam`, `removeBoardTeam`)은 그 팀이 걸린 보드들에서 그 팀의 사람만(사람을 넣고 빼면 그 사람만, 보드의 팀 역할을 바꾸면 그 보드에서) 묻고, `undeleteBoard`는 사람과 남은 팀의 역할이 다시 걸린 그 보드를 묻는다. 한 사람의 사실이 바뀐 조작(구성원·역할, 참여)은 그 사람의 연결만(워크스페이스에서 빠지거나 손님이 된 사람이 혼자 관리하던 보드가 그 조작을 한 관리자에게 넘어가면 그 보드에서는 그 관리자도), 보드나 워크스페이스 전체가 바뀐 조작은 모두를 묻고, 같은 보드의 같은 사용자·세션 방식은 여러 카드 방에 있어도 한 번만 묻고, 여럿은 여덟까지 함께 묻는다. 보드 여럿을 한꺼번에 바꾸는 조작(묶음, 가져오기)은 보드마다가 아니라 한 번에 다시 묻는다. 다른 조직의 조작은 기다리지도 다시 묻게 하지도 않는다. 서버는 그와 별개로 설정 `collab_recheck_seconds`(기본 30초)마다 방의 연결마다 들어올 때와 같은 것을 다시 묻는다(같은 사용자가 같은 방식(비밀번호, 어느 공급자)으로 만든 세션의 연결들은 한 번에).

- 더 읽을 수 없으면(보드에서 빠졌다, 워크스페이스에서 빠졌다, 카드가 지워졌거나 다른 워크스페이스의 보드로 갔다, 조직이 비밀번호를 받지 않게 됐는데(`ssoRequired`) 연결의 세션이 그 공급자로 만든 것이 아니다) `reason`이 `not_found`인 `DOC_LEFT`로 내보낸다. 물을 수 없었을 때(저장소의 오류, 한 간격 안에 답이 없음)는 2초 뒤에 한 번 더 묻고, 그래도 답이 없으면 그 오류의 `reason`(답이 없었으면 `document_unavailable`)으로 내보낸다: 확인되지 않은 연결은 방에 남지 않는다.
- 쓰던 연결이 읽을 수만 있게 됐으면 `DOC_MODE`(`read`)를 보내고 그 뒤의 변경은 `DOC_NACK`(`document_read_only`)이다. 반대로 쓸 수 있게 됐으면 `DOC_MODE`(`write`)다.
- 즉시 재확인은 방을 쥐지 않고 저장소에 묻는다. 묻는 동안 기다리는 것은 그 조작이 해당하는 연결뿐이다: 그 연결이 보낸 변경은 답이 적용되기 전에는 받아들여지지 않고, 연결이 그 뒤에 보낸 것(다른 방의 것도)은 그 뒤에 처리된다. 방의 다른 연결, 다른 방의 연결, 같은 조직의 다른 조작은 기다리지 않는다. 묻기 시작할 때 이미 진행 중이던 변경은 조작의 답보다 먼저 끝난다. 그 사이에 같은 사람에 대한 더 새 조작이 오면 앞 재확인의 답은 버려지고 새 조작의 답이 적용된다. 그 전에 들어오던 연결은 다시 묻고, 주기 재확인의 오래된 답은 즉시 재확인의 결과를 덮지 않는다. 둘 다 그 방과 그 사람에 닿는 조작만 본다: 다른 보드나 다른 사람의 조작은 들어오던 연결을 다시 묻게 하지도 주기 재확인의 답을 버리게 하지도 않는다. 저장소가 즉시 재확인에 답하지 못하면 그 오류의 `reason`으로 내보낸다. 조회 기한은 읽기마다 한 간격(느린 읽기 하나가 다른 사람의 읽기 시간을 쓰지 않는다), 재확인 전체로 두 간격이며, 기한까지 답이 없으면 그 사람만 `document_unavailable`로 내보낸다(기다리던 변경은 답 없이 버려진다). 또한 조직이 비밀번호를 받지 않게 되면 그 공급자로 만들지 않은 세션의 연결은 그 조직의 방에서 저장소에 묻지 않고 곧바로 `not_found`의 `DOC_LEFT`로 나간다(그것을 켠 요청은 그 뒤에 답한다). 그 전에 들어오던 연결은 다시 물은 뒤에 든다. 세션이 끝난 연결은 연결째 닫힌다(3절).

카드나 보드를 지우기 전에는 방의 마지막 변경까지 카드 본문에 쓴다. 쓰지 못해도 삭제는 이루어지고, 쓰지 못한 변경은 기록에 남아 되살린 카드를 열 때 반영된다. 삭제와 그 조직의 문서 작업은 엇갈리지 않는다. 삭제된 방의 모두는 `DOC_LEFT`(`not_found`)로 나가며, 그 방의 뒤늦은 입력은 카드를 되살린 뒤에도 받아들이지 않는다. 되살린 카드의 새 방은 삭제 때 저장한 본문에서 열린다.

카드·보드 복제, 템플릿 만들기, 아카이브 내보내기는 방의 문서에 있고 카드 본문에는 아직 없는 변경을 카드에 먼저 쓴 뒤 읽는다. 열린 문서가 없더라도 남은 기록은 같은 순서로 반영한다: 그 기록만 쓰려고 여는 문서는 서버가 쥘 수 있는 문서의 수와 따로 세므로(`docs/configuration.md`의 `collab_documents_per_worker`), 서버의 문서가 다 찼어도 그 때문에 실패하지 않는다. 먼저 쓰기에 실패하면 복제나 내보내기도 실패한다.

### 세대

`epoch`은 문서가 몇 번째로 만들어진 것인가다(1부터). 서버는 문서가 한도 가까이 자랐을 때와 카드의 본문이 문서를 거치지 않고 통째로 쓰였을 때(HTTP의 `patchCard` 등, 가져오기, 되살리기) 카드의 본문에서 문서를 **새로 만든다**. 내용이 같아도 다른 문서다: 앞 세대의 변경은 새 문서에 얹히지 않는다.

- 서버는 프레임마다 `epoch`을 싣고, 클라이언트도 자기가 쥔 세대를 싣는다. 방의 세대와 다른 변경은 `DOC_NACK`(`update_out_of_step`)이다.
- 문서가 새로 만들어지면 방의 모두에게 새 `epoch`의 `DOC_RESET`과 문서 전체가 간다. 클라이언트는 **쥐고 있던 문서를 버리고** 온 것으로 새 문서를 만든다. 아직 답을 받지 못한 변경은 버려진 문서의 것이다(늦게 오는 그 답은 옛 `epoch`을 싣고 있다). 자기 `epoch`과 다른 프레임은 버린다. 새로 만들어진 때와 `DOC_RESET` 사이에 보낸 변경은 `DOC_NACK`(`update_out_of_step`)이다.
- `epoch`이 그대로인 `DOC_RESET`은 같은 문서를 다시 보낸 것이다(문서를 쥔 작업자 프로세스가 죽어 서버가 저장된 것에서 다시 열었을 때 등. 까닭은 싣지 않는다). 뒤따르는 전체를 쥔 문서에 그대로 합친다(버리지 않는다). 받아들여진 변경은 모두 그 안에 있고, 그 `epoch`의 변경은 앞뒤로 그대로 받아들여진다. 작업자가 어떤 변경을 받다가 죽은 것이면 그 변경만 `DOC_NACK`(`document_unavailable`)이고 방에는 아무것도 가지 않는다.
- 방에 문서를 다시 보내려는데 열 수 없으면: 지금 다룰 수 없는 것(`document_unavailable`)이면 방은 그대로 두고 2초마다 다시 해 본다(그동안의 변경은 그 `reason`으로 거절될 수 있다). 그 밖의 까닭(서버가 문서를 더 쥘 수 없다, 카드가 없다)이면 방의 모두가 그 `reason`의 `DOC_LEFT`를 받는다. 카드를 지우는 순간에 방을 비우는 것은 아직 없다: 지운 뒤의 변경은 보낸 연결에 `DOC_NACK`(`not_found`)이다.

### `reason`의 값

값은 HTTP의 문제 이름과 같다: `reason`이 있는 문제는 그 값, 없는 문제는 `type`의 끝(`/problems/not-found`)을 밑줄로 적은 것(`not_found`). 설명은 `/problems/<이름>`에 있다.

| `reason` | 어디서 | 뜻 |
|---|---|---|
| `not_found` | `DOC_REFUSED`, `DOC_NACK`, `DOC_LEFT` | 그 카드가 이 사용자에게 없다(위 "누가 드나"). 방에 든 뒤에는 카드가 없어졌거나 이 사용자가 더 읽을 수 없게 된 것이다("방에 있는 동안") |
| `update_refused` | `DOC_NACK` | 업데이트가 아니거나, 본문 밖을 쓰거나, 문서를 본문이 아닌 것으로 만들거나, 보드에 없는 파일의 이미지를 보인다 |
| `update_out_of_step` | `DOC_NACK` | 방의 세대와 다른 `epoch`, 또는 문서에 없는 변경 위에 얹은 변경 |
| `update_too_large` | `DOC_NACK` | 업데이트가 `collab_update_bytes`를 넘는다 |
| `document_too_large` | `DOC_NACK` | 받아들이면 문서나 본문이 한도를 넘는다 |
| `document_read_only` | `DOC_NACK` | `read`로 든 연결의 변경 |
| `too_many_documents` | `DOC_REFUSED`, `DOC_NACK`, `DOC_LEFT` | 서버가 쥘 수 있는 문서가 다 찼다 |
| `document_unavailable` | `DOC_REFUSED`, `DOC_NACK`, `DOC_LEFT` | 지금 그 문서를 다룰 수 없다(작업자가 죽었거나 답하지 않았다, 문서가 잠시 닫혔다, 서버가 멈추는 중이다). 그 변경은 문서에 없다 |
| `too_many_documents_joined` | `DOC_REFUSED` | 이 연결이 이미 `collab_documents_per_connection`(기본 8)개의 방에 들어 있다. 이미 든 카드에 다시 드는 것은 세지 않는다 |
| `too_many_connections_joined` | `DOC_REFUSED` | 그 방에 이 사용자의 연결이 이미 `collab_connections_per_user`(기본 5)개 들어 있다(같은 카드를 연 탭과 기기). 이미 든 연결이 다시 드는 것은 세지 않는다 |
| `document_full` | `DOC_REFUSED` | 그 방에 이미 `collab_connections_per_document`(기본 50)개의 연결이 들어 있다 |
| `too_many_frames` | `DOC_REFUSED`, `DOC_LEFT` | 이 연결이 `collab_frames_per_second`(기본 50)보다 빨리 보냈다. 쉬고 난 뒤에는 그만큼을 한꺼번에, 그 뒤로 1초에 그만큼씩 보낼 수 있다. Binary 프레임은 모두 하나로(들지 않은 방의 것도, 프레임이 아닌 것도), 문서 전체로 답하는 `JOIN_DOC`과 step 1은 다섯으로, `LEAVE_DOC`은 하나로 센다(나가기는 넘어도 된다). 서버가 이 연결의 일을 하느라 읽지 못한 시간의 몫은 그대로 받는다. 넘은 `JOIN_DOC`은 거절되고, 넘은 프레임은 그 프레임의 방에서 이 연결을 내보낸다: 그 프레임이 실은 것을 방이 받지 않았으므로 다시 들어 문서를 새로 받는다. |
| `connection_too_slow` | `DOC_LEFT`, `DOC_REFUSED` | 이 연결에 보낼 방의 메시지가 `collab_queue_messages`(기본 512)개, 또는 `collab_state_bytes`의 두 배(바이트)를 넘게 쌓였다(읽지 않는 연결. step 1을 답을 읽지 않고 거듭 보내도 그렇다). 방의 다른 연결은 그대로 받는다. 다시 들면 문서 전체를 받는다 |
| `internal` 등 | 모두 | 서버의 오류(저장소 등). 로그에 남는다 |


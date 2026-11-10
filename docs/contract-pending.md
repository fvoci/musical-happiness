# 계약에서 곧 바뀔 부분

`contracts/openapi.json`은 지금 서버가 하는 일을 적은 것이다. 그 가운데 다음 묶음(설계 `docs/s6-design.md`의 구현 순서 8, 9단계)에서 바뀌기로 결정된 부분을 여기에 모은다. 웹앱은 이 문서에 든 조작과 멤버를 나중에 쓰거나, 쓰더라도 얇게 감싸 둔다. 여기에 없는 것은 바꿀 계획이 없다.

각 절의 "지금"은 계약 파일에 있는 것이고 "바뀐 뒤"는 설계의 결정이다. 조작은 계약의 `operationId`로 가리킨다. 경로 매개변수는 계약의 이름(`{boardId}`처럼 camelCase)으로 적는다.

계약 파일은 서버의 라우트 정의에서 만든다(`cargo xtask openapi --write`). 손으로 고치지 않는다. 파일이 서버가 내는 것과 다르면 `cargo xtask openapi`와 `cargo xtask check`가 실패한다.

## 1. 카드와 댓글의 본문 (설계 5.7)

**지금.** 카드의 본문은 카드의 자식 블록들이다(`type`이 `text`, `image`, `divider`, `checkbox`, `h1`–`h3` 등). 순서는 카드의 `fields.contentOrder`에 있고 `moveContentBlock`이 그것을 고친다. 댓글의 글은 댓글 블록의 `title`에 마크다운으로 있다.

**바뀐 뒤.**

| 대상 | 바뀌는 것 |
|---|---|
| `Block`, `Card` | 멤버 `body`가 생긴다: 허용 목록(`body_version` 1)을 따르는 문서 하나이거나 `null`. 문서의 모양은 설계 5.7의 표다(노드 `doc`, `paragraph`, `heading`, 목록들, `blockquote`, `codeBlock`, `horizontalRule`, `image`, `mention`, `text`, `hardBreak`. 마크 `bold`, `italic`, `strike`, `code`, `link`) |
| 블록의 `type` | 남는 종류는 `card`, `view`, `comment`, `attachment`다. 콘텐츠 블록 종류(`text`, `image`, `divider`, `checkbox`, 제목들)는 없어진다 |
| 카드의 `fields.contentOrder` | 없어진다 |
| `moveContentBlock` (`POST /content-blocks/{blockId}/moveto/{where}/{dstBlockId}`) | 없어진다 |
| `getCards` | 목록의 카드는 `body`를 싣지 않고, 본문이 있는지와 할 일의 수(전체, 끝낸 것)를 싣는다. 본문은 `getCard`로 읽는다 |
| 댓글 | 글이 `title`의 마크다운에서 `body` 문서로 간다 |
| 첨부 | 카드의 자식 블록으로 남는다. 순서는 만든 시각이다 |
| 블록의 `fields` 한도 | 카드·댓글·첨부는 64,000자, 뷰는 800,000자 |
| `createBlocks`, `patchBlock`, `patchBlocks`, `createCard`, `patchCard`, `createBoardsAndBlocks`, `patchBoardsAndBlocks` | `body`를 받고 검증한다. 어긋나면 400이고 `field`가 `body`다. 한도는 직렬화한 JSON 1 MiB, 깊이 32, 노드 20,000개 |
| `image` 노드의 `fileId` | 같은 보드의 파일이어야 한다. 아니면 400 |

클라이언트의 검증기와 서버의 검증은 공유 테스트 벡터 `contracts/body-vectors.json`으로 맞춘다. 서버 쪽이 처음 만든다. 아직 없다.

**웹앱이 지금 해도 되는 것.** 카드의 속성, 제목, 아이콘, 뷰, 카드 목록의 쪽 나누기는 그대로다. 콘텐츠 블록을 읽고 쓰는 코드와 `contentOrder`를 다루는 코드는 만들지 않는다.

## 2. 날짜 속성의 값 (설계 5.7의 B)

**지금.** 카드의 `properties`(계약에서는 `object`)에서 날짜 속성의 값은 JSON을 담은 문자열이다: `"{\"from\":1772712000000}"`. 날짜만 있는 값도 "그날 정오의 시각"(밀리초)으로 적고 `includeTime`, `timeZone` 멤버가 붙을 수 있다.

**바뀐 뒤.** 값이 객체다.

| 값 | 모양 |
|---|---|
| 달력 날짜 | `{"from": "2026-03-05"}` |
| 날짜 범위 | `{"from": "2026-03-05", "to": "2026-03-09"}`. `to`는 그날을 포함한다 |
| 시각 | `{"from": 1772703000000}` (UTC 밀리초 정수) |

- `from`은 필수, `to`는 선택이고 둘은 같은 종류다. `includeTime`과 `timeZone`은 없어진다. 시간대는 보는 사람의 설정(`getUserSettings`의 `timeZone`)에서 온다.
- 서버가 검증한다: 있는 날짜인지, 연도 1–9999, 정수의 범위, 모르는 멤버. 날짜 속성에는 객체나 `null`만 받는다.
- 날짜 필터의 값도 밀리초 문자열 대신 달력 날짜 문자열이다.

## 3. 속성 정의와 속성 값의 타입

**지금.** 보드의 `cardProperties`의 항목과 카드의 `properties`, 블록의 `fields`는 계약에서 멤버를 정하지 않은 `object`(`additionalProperties: true`)다. 생성한 타입에서는 `{[key: string]: unknown}`이 된다. 서버는 속성 값이 문자열, 문자열의 목록, `null` 가운데 하나인지와 속성 정의의 모양을 검증하지만(결정 대기 S14의 정리) 그 모양을 계약에 타입으로 적지 않았다.

**바뀐 뒤.** 2절의 날짜 값과 함께 속성 정의(`id`, `name`, `type`, `options`)와 속성 값의 타입을 계약에 적는다. 그때까지 웹앱은 이 둘의 타입을 자기 쪽에서 둔다.

## 4. 파일 올리기 (설계 5.6, 4.3)

**지금.** `uploadFile`(`POST /boards/{boardId}/files`)이 `multipart/form-data` 요청 하나로 파일 전체를 받는다. 한도는 설정 `maxfilesize`이고 기본값은 한도 없음이다.

**바뀐 뒤.**

| 조작 | 하는 일 | 답 |
|---|---|---|
| `POST /boards/{boardId}/uploads` `{"name", "size", "contentType"?}` | 올리기 세션을 만든다 | 201 `{"uploadId", "partSize", "partCount", "expiresAt"}` |
| `PUT /uploads/{uploadId}/parts/{n}` (본문은 바이트, 부분의 SHA-256 헤더) | 부분 하나 | 204 |
| `GET /uploads/{uploadId}` | 받은 부분의 번호와 크기 | 200 |
| `POST /uploads/{uploadId}/complete` | 확인하고 파일로 만든다 | 201 `{"fileId"}` |
| `DELETE /uploads/{uploadId}` | 버린다 | 204 |

- `uploadFile`은 남되 부분 하나의 크기(8 MiB, 설정 `upload_part_bytes`)까지만 받는다. 넘으면 413이고 `reason`이 청크로 올리라고 알린다. 붙여 넣은 이미지처럼 작은 파일의 길이다.
- 파일 하나의 한도는 100 MiB가 된다.
- 열린 세션의 수와 대기 용량을 넘으면 429다.
- `getFile`과 `getFileInfo`는 그대로다.

## 5. 아카이브와 템플릿 (설계 3.10, 5.7)

**지금.** `exportBoardArchive`, `exportTeamArchive`가 원본의 형식(판 2의 ZIP, 안에 줄마다 JSON)을 내고 `importArchive`가 판 1(JSONL)과 판 2를 받는다. 계약은 이 셋의 본문을 `application/octet-stream`과 `multipart/form-data`로만 적는다. 내장 템플릿은 원본의 아카이브에서 온다.

**바뀐 뒤.**

- 아카이브 형식이 판 3이 된다. 계약의 타입(보드, 블록, 본문)을 그대로 쓴다. 옛 형식(판 1, 판 2)의 가져오기는 없어진다.
- `importArchive`는 4절의 올리기 세션을 쓴다(용도 `import`, 한도 256 MiB). 요청의 모양이 바뀐다.
- 내장 템플릿은 웹앱 트랙의 변환기(`apps/web/tools/templates/convert.ts`)가 낸 산출물을 서버 저장소가 받아 넣는다. `getTemplates`의 모양은 그대로이고 템플릿의 내용(본문이 문서가 된다, 원본 이름과 바깥 링크가 빠진다)이 바뀐다.
- 온보딩(`onboard`)이 복사하는 보드의 내용도 같이 바뀐다.

## 6. 계약 밖에 있는 것

- **WebSocket(`/ws`)의 메시지.** OpenAPI는 WebSocket을 적지 못하고, 메시지의 모양도 계약 파일에 넣지 않았다. 어느 조작이 어느 메시지를 내는지와 메시지의 모양은 서버의 `crates/server/src/ws.rs`와 `crates/server/PORTING.md`가 기준이다. 메시지가 싣는 보드, 블록, 카테고리, 멤버는 계약의 `Board`, `Block`, `Category`, `BoardMember`와 같은 모양이다. 1절이 끝나면 `UPDATE_BLOCK`이 싣는 블록의 모양이 함께 바뀐다. 메시지를 계약처럼 고정하는 일은 남은 문제다.
- **오류의 상태 코드.** 조작마다 4xx를 하나씩 적지 않았다. 모든 조작의 `default` 응답이 `ErrorBody`이고, 어느 상태가 무슨 뜻인지는 계약의 머리말과 `default`의 설명에 있다. 조작에 고유한 거절(`reason`)은 그 조작의 설명에 있다(`invalid_credentials`, `signup_token_required`, `signup_token_invalid`, `username_taken`, `email_taken`, `admin_required`, `last_admin`, `sharing_disabled`). 429와 `Retry-After`는 `login`, `register`, `changePassword`에 따로 적었다. `getFile`의 412와 416만 `ErrorBody`가 아니다(412는 본문이 없고 416은 `text/plain`이다).
- **관리자 소켓의 라우트**(`/api/v1/admin/...`). 로컬 유닉스 소켓에서만 받는다. 계약에 없다.

## 7. 그대로 둔 불규칙

바꿀 결정이 난 것은 아니지만 계약에 그대로 적힌 모양이다. 고치게 되면 계약을 고치는 변경으로 한다.

| 무엇 | 지금 |
|---|---|
| ID 목록으로 묻는 조회가 `POST`다 | `getUsers`(`POST /users`), `getTeamUsersById`(`POST /teams/{teamId}/users`). 없는 ID나 두 번 적은 ID가 있으면 요청 전체가 거절된다 |
| 검색어 없는 `getTeamUsers` | 모든 사용자가 아니라 가장 오래된 열 명이다(검색과 같은 한도) |
| `searchTeamBoards` | 경로의 팀으로 좁히지 않는다. 사용자가 찾을 수 있는 모든 팀의 보드를 찾는다 |
| 동작을 경로 끝에 붙인 라우트 | `duplicate`, `undelete`, `join`, `leave`, `hide`, `unhide`, `onboard`, `moveto` |
| 구독의 경로 | `DELETE /subscriptions/{blockId}/{subscriberId}`, `GET /subscriptions/{subscriberId}`. 라우터에서는 첫 자리의 이름이 하나(`blockOrSubscriberId`)이고 계약에서만 갈라 적는다 |
| 구독자의 종류 `channel` | 스키마 `SubscriberType`에 있으나 서버는 `user`만 받는다 |
| 오류 본문의 `retry_after`, `request_id` | snake_case다(설계 1.2가 정한 모양) |
| 오류 본문의 선택 멤버 | 스키마에는 `null`도 되는 것으로 적혀 있으나 서버는 `null`을 보내지 않고 멤버를 뺀다 |
| `deleteAt` | 있는 것에서는 늘 0이다(지운 것은 휴지통에 있다). WebSocket의 삭제 알림이 쓴다 |
| 팀의 `title`, `settings` | 값을 정하는 라우트가 없다 |
| `/hello`, `/ping` | `/api/v1` 밖에 있다. `/ping`의 멤버 이름은 `db_type` 등 snake_case다 |
| 요청 스키마와 서버의 해석 | 만들기와 고치기의 요청은 `NewBoard`, `NewBlock`, `NewCard`, `NewCategory`, `CategoryUpdate`, `NewMember`, `MemberRoles`, `SharingRequest`, `NewSubscription`, `AdminChange`, `SettingsChange`로 따로 적었다. 서버는 이보다 너그럽게 읽는다(모르는 멤버를 대부분 버린다). 예외: `patchUserSettings`는 모르는 멤버를 400으로 거절하고, `createBoard`와 `createCategory`는 `id`를, `setSharing`은 `token`을 400으로 거절한다. 스키마가 필수로 적은 멤버가 빠졌을 때 거절하지 않고 빈 값으로 읽는 곳도 있다(`login`의 `password`가 없으면 401, `patchBlocks`의 목록이 없으면 204). 계약에 없는 멤버는 보내지 않는다 |
| `NewBlock`의 `createAt`, `updateAt` | 필수이고 1 이상이어야 하지만 서버는 그 값을 쓰지 않는다 |
| `NewCategory`, `CategoryUpdate`의 `userId`, `teamId`(와 고칠 때의 `id`) | 세션과 경로가 이미 말하는 값인데 본문에도 있어야 하고 같아야 한다. 세션과 다른 `userId`는 만들 때 403, 고칠 때 400이다 |
| `createCategory`에 `createAt`, `updateAt`을 보내면 | 계약의 `NewCategory`에는 없지만 0이 아닌 값이 오면 서버가 그 값을 쓴다 |
| `NewBoard`의 `templateVersion` | 클라이언트가 정할 수 있다 |
| `disableNotify` | 받지만 API로 보이는 효과가 없다(알림을 보내는 쪽이 아직 없다) |
| 빈 목록을 넣는 `createBlocks` | 201과 `[]` |
| 이미 있는 구독을 다시 만드는 `createSubscription` | 201 |
| 이미 멤버인 사용자를 넣는 `addMember` | 200과 지금의 멤버십. 역할은 바뀌지 않는다 |
| `patchCard`의 `updatedProperties` | 이름과 달리 합치지 않는다. 비어 있지 않으면 카드의 속성 값 전체를 그것으로 바꾼다. 비어 있으면 아무것도 바꾸지 않아서 마지막 값을 이 길로는 지울 수 없다. 합치도록 바꿀 후보다 |
| `getFile`의 `Content-Type` | 모든 파일이 `application/octet-stream`과 `Content-Disposition: attachment`로 나간다. 파일 이름은 올린 이름이 아니라 `fileId`다. `Last-Modified`가 응답 시각이라 재검증으로 내려받기를 아끼지 못한다 |
| `searchTeamBoards`의 `field=property_name` | 카드 속성의 이름이 아니라 보드 자신의 `properties` 객체에 그 이름의 멤버가 있는지를 본다. 정확히 같은 이름만 찾는다 |
| 검색어의 `%`, `_` | `searchBoards`, `searchTeamBoards`, `getTeamUsers`의 검색어에서 와일드카드로 읽힌다. 빈칸 둘을 이어 쓰면 모든 제목이 맞는다 |
| 역할이 하나도 없는 멤버 | `addMember`, `updateMember`에 역할을 모두 빼고 보내면 보기 권한도 없는 멤버가 된다 |
| `removeMember`와 `updateMember` | 멤버가 아닌 사용자를 빼면 204, 고치면 404다 |
| `createCategory`의 `sortOrder` | 보낸 값에 10을 더해 저장하고, 그 사용자의 다른 카테고리도 모두 10씩 민다 |
| `type: system`인 `createCategory` | 받는다. 그렇게 만든 카테고리는 이름을 바꾸거나 지울 수 없다 |
| `hideBoard`, `unhideBoard` | 그 카테고리에 없는 보드여도 204다 |
| `deleteBoard` 뒤의 `undeleteBoard` | 공유(읽기 토큰), 사이드바의 자리, 구독은 돌아오지 않는다 |
| `deleteBoardsAndBlocks`의 `blocks` | 검사만 한다. 보드가 자기 블록을 모두 데리고 휴지통에 가고, `undeleteBoard`로만 돌아온다 |
| `exportTeamArchive` | 없는 팀도 200이다(보드 없는 아카이브) |
| `importArchive` | 보드를 하나씩 만든다. 중간에 실패하면 앞의 보드는 남는다. 요청 크기의 한도가 아직 없다(4절에서 생긴다) |
| `onboard` | 부를 때마다 보드를 하나 더 복사한다. 답이 200이다(만든 것을 201로 답하는 규칙의 예외, PORTING N) |
| `createSubscription`의 `blockType` | 블록의 실제 종류와 견주지 않고 보낸 대로 저장한다 |
| 세션 없는 큰 요청 | JSON 본문이 64 KiB를 넘으면 401보다 먼저 413이다 |

## 8. 알려진 결함

계약이 약속하지 않는 동작이다. 클라이언트는 여기에 기대지 않는다. 다음 묶음에서 고칠 후보이고, 고치면 이 절에서 뺀다.

| 무엇 | 지금 |
|---|---|
| 카드가 아닌 모양의 카드 블록 | `createBlocks`와 `patchBlock`은 `fields`의 `properties`만 검사한다. 종류가 `card`인 블록의 `contentOrder`가 문자열 목록이 아니거나 `icon`이 문자열이 아니거나 `isTemplate`이 불리언이 아니면 `getCards`와 `getCard`가 500이다. 편집자 한 명이 그 보드의 카드 목록을 막을 수 있다 |
| 파일 없는 이미지·첨부 블록 | `fileId`도 `attachmentId`도 문자열로 갖지 않은 `image`, `attachment` 블록이 있으면 `exportBoardArchive`, `exportTeamArchive`가 500이다 |
| SQLite에서 `field=property_name` | 검색어가 JSON 경로의 일부가 된다. `a.b`는 안쪽 멤버를 찾고, 경로로 읽을 수 없는 검색어는 DB 오류다. 값이 `null`인 멤버는 PostgreSQL에서만 찾는다 |
| DB에 따라 다른 대소문자 | 제목 검색은 SQLite에서 ASCII 글자만 대소문자를 접는다. `getTeamUsers`의 `search`는 SQLite에서 ASCII 대소문자를 가리지 않고 PostgreSQL에서 가린다 |
| 기본 카테고리를 처음 만드는 `getCategories` | 카테고리를 이미 가진 사용자의 기본 카테고리가 만들어지는 그 한 번의 답은, 새 카테고리가 `sortOrder`와 상관없이 맨 끝에 오고 다른 카테고리는 10씩 밀리기 전의 `sortOrder`를 싣는다. 다음 요청부터는 맞다 |
| 내장 환영 보드가 없을 때의 `onboard` | 500이다 |
| `setSharing`의 `token: null` | 거절하지 않는다(`null`인 멤버를 먼저 버린다) |

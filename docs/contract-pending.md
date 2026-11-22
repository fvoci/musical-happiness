# 계약에서 곧 바뀔 부분

`contracts/openapi.json`은 지금 서버가 하는 일을 적은 것이다. 이 문서는 그 가운데 바뀌기로 결정된 부분과 구현된 결과를 모은다. 웹앱은 이 문서에 든 조작과 멤버를 나중에 쓰거나, 쓰더라도 얇게 감싸 둔다. 여기에 없는 것은 바꿀 계획이 없다.

1, 2, 4, 5절은 작업 묶음 `fvoci/server-next`가 구현했다(설계 `docs/s6-design.md`의 "구현 순서" 8, 9단계).

읽는 법:

- 이 문서는 어느 조작과 멤버가 바뀌는지, 웹앱이 지금 무엇을 해도 되는지를 적는다. 바뀐 뒤의 모양과 검증 규칙의 전부는 절마다 가리키는 설계의 절에 있다.
- 조작은 계약의 `operationId`로 가리킨다. 경로 매개변수는 계약의 이름(`{boardId}`처럼 camelCase)으로 적는다.

계약 파일은 손으로 고치지 않는다. 서버의 라우트 정의에서 만든다(`cargo xtask openapi --write`). 파일이 서버가 내는 것과 다르면 `cargo xtask openapi`와 `cargo xtask check`가 실패한다. 고치는 절차는 `docs/handoff.md` 1절의 "계약을 고치는 절차"에 있다.

| 절 | 무엇이 바뀌나 | 설계 |
|---|---|---|
| 1 | 카드와 댓글의 본문이 문서 하나가 된다 | 5.7 |
| 2 | 날짜 속성의 값이 객체가 된다 | 5.7 |
| 3 | 속성 정의와 속성 값이 계약에 타입으로 적힌다 | 5.7 |
| 4 | 파일을 부분으로 나눠 올린다 | 5.6, 4.3 |
| 5 | 아카이브 형식과 내장 템플릿이 바뀐다 | 3.10, 5.7 |
| 6 | (바뀌지 않음) 계약 밖에 있는 것 | |
| 7 | (바뀌지 않음) 그대로 둔 불규칙 | |
| 8 | (고칠 후보) 알려진 결함 | |
| 9 | (바뀌지 않음) 웹앱의 질문에 대한 답 | |

## 1. 카드와 댓글의 본문 (설계 5.7)

**바뀐 뒤.** 본문은 허용 목록을 따르는 문서 하나다. 허용하는 노드와 마크, 속성, 포함 관계는 설계 5.7의 허용 목록 표에 있다.

| 대상 | 바뀌는 것 |
|---|---|
| `Block`, `Card` | 멤버 `body`가 생긴다. 값은 문서 하나이거나 `null`이다 |
| 블록의 `type` | 남는 종류는 `card`, `view`, `comment`, `attachment`다. 콘텐츠 블록 종류(`text`, `image`, `divider`, `checkbox`, 제목들)는 없어진다 |
| 카드의 `fields.contentOrder` | 없어진다 |
| `moveContentBlock` (`POST /content-blocks/{blockId}/moveto/{where}/{dstBlockId}`) | 없어진다 |
| `getCards` | 목록의 카드는 `body`를 싣지 않는다. 본문이 있는지와 할 일의 수(전체, 끝낸 것)를 싣는다. 본문은 `getCard`로 읽는다 |
| 댓글 | 글이 `title`의 마크다운에서 `body` 문서로 간다 |
| 첨부 | 카드의 자식 블록으로 남는다. 순서는 만든 시각이다 |
| 블록의 `fields` 한도 | 카드·댓글·첨부는 64,000자, 뷰는 800,000자 |
| `createBlocks`, `patchBlock`, `patchBlocks`, `createCard`, `patchCard`, `createBoardsAndBlocks`, `patchBoardsAndBlocks` | `body`를 받고 검증한다. 어긋나면 400이고 `errors`가 `body`의 자리를 가리킨다 |
| `body`의 한도 | 직렬화한 JSON 4 MiB, 깊이 60, 노드 50,000개 |
| `image` 노드의 `fileId` | 같은 보드의 파일이어야 한다. 아니면 400 |

**들어간 것(tracer-bullet 1, 2026-11-11 04:45 KST).** 아래는 계약 파일에 이미 있다. 표의 나머지는 아직이다.

- `Card`에 `body`, `hasBody`, `tasksTotal`, `tasksDone`가 늘 있다. `getCard`와 `createCard`·`patchCard`의 답은 `body`에 문서를, `getCards`는 `body`에 `null`을 싣고 나머지 셋으로 본문이 있는지와 할 일 수를 알린다. 목록은 본문을 읽지 않고 저장할 때 센 열을 읽는다.
- `Block`에 `body`가 늘 있다(없으면 `null`). WebSocket의 `UPDATE_BLOCK`도 `Block` 전체이므로 본문을 싣는다(`contracts/websocket.md`).
- `createCard`가 `body`를 받는다. 빠졌거나 `null`이거나 빈 문서(빈 문단 하나)면 본문이 없다.
- `patchCard`가 `body`와 `readAt`을 받는다. `body`가 빠지면 본문은 그대로, `null`이나 빈 문서면 지운다. **`body`를 보내려면 `readAt`(그 카드를 읽었을 때의 `updateAt`)이 있어야 한다.** 없으면 400이고 `errors`가 `readAt`의 자리를 가리킨다. `readAt`이 지금의 `updateAt`과 다르면 409(`conflict`)이고 아무것도 바뀌지 않는다. `readAt`은 `body` 없이 보내도 검사한다. 패치를 적용한 블록이 블록의 한도(제목, `fields`)를 넘으면 `readAt`을 보기 전에 400이다. 부모의 검사는 그 뒤라서, 부모가 틀리고 `readAt`도 낡았으면 409다.
- 검증은 허용 목록 판 2 전체다(tracer-bullet 2, 2026-11-11 05:29 KST): 노드와 마크의 종류, 속성과 그 값, 어느 노드가 어디에 올 수 있는지, 한도(4 MiB, 깊이 60, 노드 50,000). 길이는 UTF-16 코드 단위로 센다. 어느 문자열과 멤버 이름에도 U+0000이 있으면 거절한다(PostgreSQL의 `jsonb`가 담지 못한다. 웹앱의 검증기에는 아직 없는 규칙이다). 허용 목록을 통과한 문서가 빈 문단 하나뿐이면(`attrs`가 있어도) 본문이 없는 것이다. 어긋나면 400이고 `errors`가 `body`의 자리를 가리킨다. 다만 JSON이 128단보다 깊게 겹친 요청은 디코더가 먼저 거절해 `errors`가 없는 400이다(한도 60단의 본문은 JSON으로 122단까지라 늘 디코더를 지나고, 한도를 조금 넘는 본문은 어느 쪽에서든 400이다).
- 서버가 저장할 때 `body_version`(2)과 검색용 평문 `body_text`를 함께 적는다. 평문은 문단·제목·코드 블록·수식 블록마다 한 줄이고 글자가 없는 블록의 줄은 뺀다. 줄바꿈(`hardBreak`)은 그 블록의 줄 안의 줄바꿈으로 남는다. 인라인 수식은 제자리에 원문으로 들어가고, 멘션은 아무것도 남기지 않는다(이름을 저장하지 않는다). 웹앱의 `plainText`는 멘션을 `@이름`으로 쓰므로 다르다. 평문의 앞부분은 `getCards?preview=true`의 `preview.text`로 나간다.
- 블록의 경로(`createBlocks`, `patchBlock`, `patchBlocks`, `createBoardsAndBlocks`, `patchBoardsAndBlocks`)도 카드의 본문을 받는다(tracer-bullet 2b, 2026-11-11 05:55 KST). 검증과 거절(400, `errors`: `body`의 자리)은 카드의 경로와 같다. 패치가 `body`를 바꾸려면(`null`도) `readAt`이 있어야 하고, 없으면 400이고 `errors`가 `readAt`의 자리를 가리킨다. **본문은 카드와 댓글만 갖는다.** 다른 블록에 본문이 있으면 400이고 `errors`가 `body`의 자리를 가리킨다.
- **본문의 이미지는 그 블록의 보드의 파일이다**(설계 5.7). `image`의 `fileId`가 그 보드가 가진 파일의 저장 이름이 아니면 400이고 `errors`가 `body`의 자리를 가리킨다. 본문을 받는 경로가 모두 같다(아카이브 가져오기는 아직 아니다. 8절). `createBlocks`에 `sourceBoardId`를 주면 그 보드의 파일이어야 하고(받는 보드에 이미 있는 파일이어도 거절한다), 블록과 함께 파일이 복사된다. `createBoardsAndBlocks`의 보드는 새것이라 파일이 없으므로 이미지가 있는 본문을 무엇도 찾아보지 않고 거절한다. 본문이 있는 카드나 댓글을 그 둘이 아닌 `type`으로 바꾸는 패치는 본문을 보내지 않았어도 400이고 `errors`가 `body`의 자리를 가리킨다.
- **댓글의 본문**(tracer-bullet 3). 댓글 블록도 `createBlocks`, `patchBlock`, `patchBlocks`, `createBoardsAndBlocks`, `patchBoardsAndBlocks`로 `body`를 받는다. 검증, `readAt`, 이미지가 그 보드의 파일이어야 하는 규칙, 저장되는 평문까지 카드와 같다. 댓글을 고치는 권한은 9절의 B1이다. 댓글만 쓸 수 있는 멤버(commenter)는 파일을 올리지 못해 댓글에 새 그림을 넣지 못하고, `sourceBoardId`로 파일을 복사하게 되는 요청은 편집 권한을 요구한다(없으면 403). 댓글의 `title`은 아직 받고 돌려준다(웹앱이 `body`로 옮겨 간 뒤에 걷어 낼지는 5절의 콘텐츠 블록과 함께 본다). 카드·보드 복제는 지금처럼 댓글을 복사하지 않는다. `sourceBoardId`로 다른 보드의 댓글을 만들면 그 본문의 파일도 복사된다.
- **댓글의 이력(사용자 결정 2026-11-11 14:46 KST).** 댓글의 본문이 바뀔 때마다(어느 경로든, 본문이 같은 패치는 빼고) 서버가 그 전의 판을 남긴다. 본문이 바뀐 적 있는 댓글의 `Block`에는 `editedAt`(마지막으로 바뀐 시각)이 있고, 없으면 바뀐 적이 없다(클라이언트가 보낸 값은 무시한다). 판은 `getCommentVersions`(`GET /boards/{boardId}/blocks/{blockId}/versions`)로 읽는다: 지금의 것을 뺀 판들이 처음 것부터 `{version, body, writtenBy, writtenAt}`로 온다. 첫 판은 처음 쓴 글이고 쓴 사람은 댓글을 만든 사람이다. 보드의 구성원(보기 역할부터)만 읽고, 읽기 토큰으로는 읽지 못한다. 댓글이 휴지통에 있어도 읽는다. 댓글이 아닌 블록은 404다. 댓글마다 처음 판과 가장 새 판들을 합해 설정 `max_comment_versions`(기본 50)판까지 남긴다. 남은 판의 본문이 보이는 파일은 수거되지 않고, 댓글이 영구히 지워질 때 판도 지워진다. 내보내기·가져오기·복제는 판을 싣지 않는다(사용자가 확정, 2026-11-11 21:00 KST).
- 카드를 복제하면(블록·보드 복제, 템플릿에서 보드 만들기) 본문의 이미지가 가리키는 파일도 복사되고, 복사본의 본문은 새 파일을 가리킨다.
- `BlockPatch.readAt`은 지금 쓰인다: `patchBlock`, `patchBlocks`, `patchBoardsAndBlocks`의 패치에 있으면 `patchCard`처럼 검사하고 어긋나면 409다.
- 있는 블록을 고칠 때마다 `updateAt`이 앞의 값보다 커진다(같은 밀리초여도). `readAt`이 그 사이의 변경을 놓치지 않는다.
- 휴지통에서 되살린 카드와 복제한 카드는 본문과 할 일 수를 그대로 갖는다.
- **읽기 토큰의 독자와 사용자 ID(사용자 결정 2026-11-11 11:05, 14:25 KST).** 읽기 토큰으로 읽고 보드의 구성원(보기 역할부터)이 아닌 독자(세션이 없거나, 로그인했어도. 열린 보드의 워크스페이스 사용자도 토큰으로 읽으면 그렇다)에게 나가는 보드와 블록은 `createdBy`·`modifiedBy`가 비어 있고(결정 7), 카드와 댓글의 본문에서 모든 멘션의 `userId`가 빈 문자열이고, 보드의 사람 속성(`type`이 `person`, `multiPerson`, `createdBy`, `updatedBy`)이 가리키는 사람이 카드의 `fields.properties`와 뷰의 `fields.filter` 절에서 사람마다 빈 문자열이다(목록의 길이는 그대로). 카드의 값 가운데 보드가 정의하지 않은 속성의 것(지운 사람 속성이 남긴 것)은 빠지고, 사람 속성으로 묶은 뷰의 `visibleOptionIds`, `hiddenOptionIds`, `collapsedOptionIds`, `kanbanCalculations`에는 값 없는 묶음(`""`)만 남는다. HTTP(`getContainer`, `getBlocks`, `getCards`의 카드도 같다)와 WebSocket(`UPDATE_BLOCK`) 모두. 그 독자가 받는 응답 가운데 사용자 ID를 싣는 자리는 이것뿐이다(`getFile`, `getFileInfo`는 싣지 않는다). 사람 속성을 `text` 같은 다른 종류로 바꾸면 남은 값은 그 종류의 값으로 그대로 나간다. 이것은 나가는 모양일 뿐 저장되는 모양이 아니다. 허용 목록은 `userId`가 사용자 ID가 아닌 멘션을 받지 않으므로, 그 본문을 그대로 되돌려 쓰면 400이다(익명 독자는 쓰지 못하니 생기지 않는다). 화면은 빈 `userId`의 멘션을 "알 수 없는 사용자"로 그리면 된다. 보드의 있음은 읽기 토큰의 연결을 수로만 세고 그 연결에는 명단을 보내지 않는다(`contracts/websocket.md` 7절).

**들어간 것(tracer-bullet 6, 2026-11-11 08:42 KST).** 표의 나머지 넷(블록의 `type`, `contentOrder`, `moveContentBlock`, `fields` 한도)이 들어갔다. 이로써 이 표는 모두 계약 파일에 있다.

- `moveContentBlock`이 없다. 그 경로는 404다.
- `Card`, `CardPatch`, `createCard`의 몸에서 `contentOrder`가 없어졌다. 보내도 무시하고 저장하지 않는다. 전에 저장된 카드 블록의 `fields.contentOrder`는 그대로 남고 읽지 않는다. 그래서 `contentOrder`가 중첩 목록인 카드도 이제 카드로 읽힌다(8절에서 그 경우를 뺐다).
- 새 블록의 `type`은 `card`, `view`, `comment`, `attachment`만 된다. 다른 종류는 `createBlocks`와 `createBoardsAndBlocks`에서 400이고 `errors`가 `type`의 자리를 가리킨다. 메시지는 클라이언트가 적은 종류를 64자까지만 옮긴다. 아카이브 가져오기도 400이다(`errors` 없이, 메시지가 블록과 종류를 적는다). 어느 쪽이든 아무것도 저장하지 않는다. 패치(`patchBlock`, `patchBlocks`, `patchBoardsAndBlocks`)가 블록을 그런 종류로 바꾸는 것도 400이고 `errors`가 `type`의 자리를 가리킨다.
- `fields`의 한도는 직렬화한 JSON의 글자 수로 카드·댓글·첨부 64,000자, 뷰 800,000자다. 만들 때와 패치할 때 모두 본다. 세는 단위는 지금까지의 800,000자와 같은 유니코드 글자다(본문의 UTF-16과 다르다). 그 밖의 종류(옛 보드 블록의 `board` 등)는 800,000자다. 전에 저장된 카드·댓글·첨부가 64,000자를 넘으면 그 블록을 고치는 패치(제목만 바꾸는 것도)와 그 블록이나 그것이 든 보드의 복제가 400이다. `fields`를 64,000자 아래로 줄이는 패치는 받는다. 그런 카드가 든 보드는 내보낼 수 있지만, 그 아카이브의 가져오기는 400이다.
- **저장된 콘텐츠 블록은 지운다(사용자 결정 2026-11-11 11:05 KST, tracer-bullet 16).** 마이그레이션 8의 데이터 단계가 서버가 시작할 때 한 번, 콘텐츠 블록(`text`, `image`, `checkbox`, `divider`, 제목들과 카드·뷰·댓글·첨부·옛 보드 블록이 아닌 모든 종류)의 행을 보드와 휴지통에서 지운다. 그 블록만 보이던 파일은 다른 파일처럼 유예 기간 뒤에 수거된다. 본문으로 바꾸지 않는다. 그 뒤로 서버에는 그런 블록을 읽고, 고치고, 복제하고, 내보내는 길이 없다. 그런 블록이 든 아카이브는 가져오기가 거절한다(전과 같다).

**허용 목록의 판.** 판 2가 들어갔다(설계 5.7의 표). 판 1에 수식 노드 둘을 더한 것이다(웹앱 쪽의 제안을 사용자가 받아들였다. 2026-11-11 03:13 KST의 작업 지시에 적힌 결정이다).

- 새 노드는 `inlineMath`(인라인)와 `blockMath`(블록)다. 둘 다 속성은 `latex` 하나이고 1–4,000자의 문자열이다.
- 다이어그램은 언어가 `mermaid`인 `codeBlock`이다. 허용 목록도 서버도 바뀌지 않는다.
- 자세한 것(새 노드가 올 수 있는 자리, 테스트 벡터에 더할 경우)은 웹앱의 문서 `apps/web/docs/contract-assumptions.md` 4절에 있다.

**공유 테스트 벡터.** 클라이언트의 검증기와 서버의 검증은 `contracts/body-vectors.json`(판 2, 166개 경우)으로 맞춘다. 경우마다 문서와 기대(`accept`, `empty`, `reject`와 그 까닭)를 적고, 받는 경우에는 서버가 뽑을 평문(`text`)을 적기도 한다. 한도의 경계(깊이 60과 61, 노드 50,000과 50,001, 4 MiB와 1바이트 더)는 문서 대신 만드는 법(`build`)을 적었다. 설계 5.7이 정한 경계, 웹앱 문서 4.4의 1–13번(UTF-16으로 세는 6번과 10번 포함)이 들어 있다. 서버의 테스트 `crates/model/tests/body_vectors.rs`가 모두 읽는다.

**웹앱에 알릴 것(벡터와 웹앱 검증기의 차이).** 서버 쪽에서 보기에 웹앱의 검증기가 벡터와 다르게 답할 경우다. 웹앱의 벡터 테스트가 잡을 것이다. (1) U+0000을 담은 문자열을 서버는 거절한다. (2) `mention.userId`를 서버는 사용자 ID 형식(UUID 소문자 표기)으로만 받는다(설계 5.7). 웹앱은 1–100자의 문자열이면 받는다. (3) `content: null`과 `text` 노드의 `marks: null`을 서버는 거절한다(설계 5.7의 "`null`은 거절"). 웹앱은 빈 것으로 본다. (4) `build`로 적은 경우는 웹앱의 테스트가 같은 법으로 문서를 만들어야 한다.

**웹앱이 지금 해도 되는 것.** 카드의 속성, 제목, 아이콘, 뷰, 카드 목록의 쪽 나누기는 그대로다. 콘텐츠 블록을 읽고 쓰는 코드와 `contentOrder`를 다루는 코드는 만들지 않는다(tracer-bullet 6부터 서버에 없다).

## 2. 날짜 속성의 값 (설계 5.7의 B)

**바뀐 뒤.** 값이 객체다. 달력 날짜는 문자열이고(`{"from": "2026-03-05"}`), 시각은 UTC 밀리초 정수다(`{"from": 1772703000000}`). 범위는 `to`를 더한다. 세 모양의 표와 서버의 검증 규칙은 설계 5.7의 "날짜 속성 값의 와이어 모양"에 있다.

웹앱이 알아 둘 것:

- `includeTime`과 `timeZone`은 없어진다. 시간대는 보는 사람의 설정(`getUserSettings`의 `timeZone`)에서 온다.
- 날짜 필터의 값도 밀리초 문자열이 아니라 달력 날짜 문자열이다.

**들어간 것(tracer-bullet 4, 2026-11-11).** 서버가 설계 5.7의 규칙대로 받는다. 카드를 쓰는 모든 경로(`createCard`, `patchCard`, `createBlocks`, 블록 패치 셋, boards-and-blocks 둘, 가져오기)에서 같다.

- 날짜 값은 객체이고 `from`과 `to` 밖의 멤버(`includeTime`, `timeZone`)가 있으면 거절한다. 달력에 없는 날, 연도 1–9999 밖, 0보다 작거나 9999년을 넘는 밀리초, 종류가 섞인 `from`과 `to`, `from`보다 앞선 `to`, `from`과 같은 `to`도 거절한다. 모두 400이다(`errors`는 없고 메시지가 속성의 ID와 까닭을 말한다).
- 보드의 속성 정의에서 `type`이 `date`인 속성은 날짜 값이나 `null`만 받는다. 다른 속성(정의가 없는 ID 포함)은 날짜 값을 받지 않는다. 그래서 날짜 속성에 예전 모양(JSON을 담은 문자열)을 보내면 400이다.
- 속성 정의의 `type`을 바꾸거나 정의를 지워도 카드의 값은 그대로 둔다. 검사는 쓰기가 바꾸는 값에만 하므로, 맞지 않게 된 값이 있는 카드도 제목·본문 같은 다른 것은 고칠 수 있고, 그 값을 다시 쓸 때 새 규칙으로 검사한다. 카드와 보드를 복제하거나 아카이브를 가져올 때는 맞지 않는 종류의 값(날짜가 아닌 속성의 날짜 값, 날짜 속성의 문자열이나 목록)을 `null`로 바꿔 넣는다.
- 밀리초는 JSON의 정수로 적는다. `1772703000000.0`처럼 소수점을 단 수는 받지 않는다(JavaScript는 그렇게 적지 않는다).
- 이미 저장된 값은 마이그레이션 4가 서버를 처음 띄울 때 한 번 바꾼다(휴지통의 것까지). 시각이 없던 값(`includeTime`이 없거나 거짓)은 그 밀리초의 UTC 날짜가 된다. 예전 웹앱이 그날 정오로 적었으므로 UTC-12부터 UTC+11까지는 적은 날 그대로다. 시각이 있던 값은 밀리초 정수로 남는다. 읽을 수 없는 값은 `null`이 된다. 뷰의 필터 가운데 날짜 속성의 것은 밀리초 문자열을 UTC 날짜로 바꾼다.
- 서버는 뷰의 필터 값을 검사하지 않는다(필터는 클라이언트가 쓴다). 계약의 `properties`는 아직 `object`이고 설명에 날짜 값을 적었다. 값의 타입을 계약에 적는 일(3절)은 남았다.

## 3. 속성 정의와 속성 값의 타입

**지금.** 다음 셋은 계약에서 멤버를 정하지 않은 `object`(`additionalProperties: true`)다: 보드의 `cardProperties`의 항목, 카드의 `properties`, 블록의 `fields`. 생성한 타입에서는 `{[key: string]: unknown}`이 된다.

서버는 이미 검증한다. 속성 값은 문자열, 문자열의 목록, `null` 가운데 하나여야 하고, 속성 정의의 모양도 본다. 다만 그 모양을 계약에 타입으로 적지 않았다.

**바뀐 뒤.** 2절의 날짜 값과 함께 속성 정의(`id`, `name`, `type`, `options`)와 속성 값의 타입을 계약에 적는다. 그때까지 웹앱은 이 둘의 타입을 자기 쪽에 둔다.

## 4. 파일 올리기 (설계 5.6, 4.3)

**바뀐 뒤.** 올리기 세션의 조작 다섯이 생긴다: 세션 만들기, 부분 하나 올리기, 받은 부분 묻기, 완료, 버리기. 경로와 요청·답의 모양은 설계 5.6의 "라우트" 표에 있다. 조작 이름(`operationId`)은 계약에 넣을 때 정한다.

이미 있는 조작은 이렇게 된다.

- `uploadFile`은 남는다. 다만 부분 하나의 크기(8 MiB, 설정 `upload_part_bytes`)까지만 받는다. 넘으면 413이고 `reason`이 청크로 올리라고 알린다. 붙여 넣은 이미지처럼 작은 파일을 위한 길이다.
- 파일 하나의 한도는 100 MiB가 된다.
- 열린 세션의 수와 대기 용량을 넘으면 429다.
- `getFile`과 `getFileInfo`는 그대로다.

권한, 크기 검사, 체크섬, 만료의 규칙은 설계 5.6의 "세부 결정" 표(1–11번)에 있다.

**들어간 것(tracer-bullet 7, 2026-11-11 09:20 KST).** 올리기 세션의 조작 다섯이 계약에 들어갔다. 모두 태그 `files`이고 세션이 있어야 한다.

| 조작 | 경로 | 요청 | 답 |
|---|---|---|---|
| `createUpload` | `POST /containers/{containerId}/uploads` | `NewUpload` `{name, size, contentType?}` | 201 `UploadOpened` `{uploadId, partSize, partCount, expiresAt}` |
| `putUploadPart` | `PUT /uploads/{uploadId}/parts/{partNumber}` | 부분의 바이트(`application/octet-stream`), 헤더 `Content-Digest: sha-256=:<base64>:` | 204 |
| `getUpload` | `GET /uploads/{uploadId}` | 없음 | 200 `UploadState` `{uploadId, containerId, name, size, partSize, partCount, expiresAt, parts: [{number, size}]}` |
| `completeUpload` | `POST /uploads/{uploadId}/complete` | 없음 | 201 `UploadedFile` `{fileId}`(`uploadFile`의 답과 같다) |
| `deleteUpload` | `DELETE /uploads/{uploadId}` | 없음 | 204 |

규칙은 이렇다.

- **부분.** 부분은 1부터 `partCount`까지 번호가 붙는다. 마지막을 뺀 모든 부분이 `partSize`(설정 `upload_part_bytes`, 기본 8 MiB) 바이트이고, 마지막이 나머지다. 빈 파일은 부분이 없고(`partCount` 0) 바로 완료한다.
- **부분 받기.** `Content-Length`가 그 부분의 크기와 다르면 본문을 보기 전에 400이다. 거절한 부분의 본문은 클라이언트가 답을 읽을 수 있게 8 MiB까지, 5초까지 읽어 버리고, 그보다 길거나 느리면 끊는다. 본문이 그보다 길거나 짧아도, `Content-Digest`의 SHA-256이 다르거나 헤더가 없어도 400이고, 받던 것은 디스크에 남기지 않는다. 같은 번호를 다시 보내면 앞의 것을 바꾼다. 순서는 상관없다.
- **완료.** 빠진 부분이 있으면 400이고 메시지가 번호를 든다(`parts missing: 1, 3`). 세션은 열린 채 남는다. 완료하면 세션과 부분이 없어진다. 완료하는 동안 그 세션에 오는 부분, 버리기, 두 번째 완료는 409다.
- **세션.** 세션은 연 사용자의 것이다. 다른 사용자의 세션과 없는 세션은 똑같이 404다. `uploadId`는 256비트 난수(URL-safe base64 43자)다. 세션은 연 때부터 24시간 뒤에 끝나고, 끝난 세션은 404다.
- **권한.** 열 때와 완료할 때 그 보드에 `uploadFile`과 같은 권한(카드를 고칠 수 있는 역할)이 있어야 한다. 완료할 때 권한이 없으면(보드에서 빠졌거나 보드가 없어졌으면) 403이나 404이고 세션을 버린다.
- **크기.** `size`가 `max_file_bytes`를 넘으면 열 때 413이다. `max_file_bytes`는 1 이상이고 `partSize` × 10,000(S3 멀티파트의 부분 수 한도)을 넘지 않는다(그 밖의 설정은 시작 오류다). 이름은 1–255자, `contentType`은 255자까지다. 저장된 파일의 `fileId`는 `uploadFile`처럼 이름의 확장자로 끝난다.
- **부분을 두는 곳.** 끝나지 않은 부분은 설정 `tmp_path`(기본 `file_storage_path` 아래 `.tmp`)의 `uploads/<uploadId>/`에 둔다.

**들어간 것(tracer-bullet 8, 2026-11-11 09:53 KST).** 조작과 모양은 그대로이고, 한도와 정리가 들어갔다.

- **열린 업로드의 한도.** 사용자 한 명은 업로드를 8개까지 열어 둔다. 사용자의 열린 업로드가 걸어 두는 크기(선언한 `size`의 합)는 가장 큰 파일 한도의 두 배까지(9b부터는 파일과 아카이브 한도 가운데 큰 것의 두 배), 서버 전체는 설정 `max_pending_upload_bytes`(기본 2 GiB)까지다. 넘는 `createUpload`는 429이고 `reason`이 `too_many_uploads`, `uploads_pending`, `server_uploads_pending` 가운데 하나다. `Retry-After`는 없다: 자리는 업로드를 끝내거나 버려야 난다. 완료하는 중인(닫힌) 업로드는 세지 않는다. 같은 순간에 연 세션들도 한도를 함께 넘지 못한다.
- **파일 하나의 한도.** `maxFileBytes`(설정 `max_file_bytes`)의 기본이 0(한도 없음)에서 100 MiB가 됐다(설계 결정 16).
- **`uploadFile`.** 파일을 메모리에 모으지 않고 저장소로 흘려 쓴다. 부분 하나의 크기(`partSize`, 기본 8 MiB)까지만 받고, 그보다 크면 413이고 `reason`이 `upload_in_parts`다. 그 파일이 `maxFileBytes` 안이면 `createUpload`로 보낼 수 있다(흘려 받으며 거절하므로 서버는 전체 크기를 모른다). `maxFileBytes`가 부분보다 작으면 `maxFileBytes`까지이고 413의 `reason`이 `file_too_large`다. `maxFileBytes`는 이제 0(한도 없음)일 수 없다. 요청이 선언한 길이가 한도를 크게 넘으면 읽기 전에, 아니면 읽다가 거절한다. 거절하거나 끊긴 파일은 남지 않는다.
- **정리.** 끝난(24시간이 지난) 열린 업로드와, 닫힌 채 한 시간이 지난 업로드(서버가 완료 중에 멈춘 것. 끝나기 직전에 시작한 완료도 이 한 시간을 받는다)는 서버가 한 시간마다, 그리고 시작할 때 부분과 함께 버린다. 시작할 때는 어느 업로드의 것도 아닌 부분과, 로컬 저장소가 쓰다 만 파일도 지운다.

들어간 것(tracer-bullet 8b, 2026-11-11 10:23 KST): S3 멀티파트. 연산의 모양과 뜻은 그대로다.

- `file_storage`가 `s3`이면 크기가 0보다 큰 세션은 열 때 버킷의 최종 자리에 멀티파트 업로드를 시작한다. 부분은 로컬과 같이 임시 파일로 받아 길이와 SHA-256을 확인한 뒤 버킷에 보내고 지운다(같은 번호는 바꿔 쓴다). 버킷에 보내는 동안의 메모리는 부분 크기의 8개분까지라 그보다 많이 동시에 보내면 앞의 것이 끝날 때까지 답이 늦다. 한 세션의 부분은 버킷에 하나씩 보낸다(완료·버리기와 차례를 나눠, 닫힌 세션의 부분은 버킷에 닿지 않고 409다). 버킷에 보내다 실패하면 500이고 그 번호는 받지 않은 것이 된다(다시 보낸다). 완료하면 버킷이 부분을 이어 객체를 만들고, 버리거나 끝난 세션은 중단한다.
- 크기가 0인 파일은 보낼 부분이 없어 로컬과 같이 완료할 때 빈 객체를 쓴다.
- 버킷이 세션 열기를 받지 못하면 500이고 세션은 남지 않는다. 버킷이 부분을 받지 못하면 500, 그사이 세션이 닫혔으면 409다. 버킷이 완료를 거절하면 500이고 세션과 기록 없는 객체가 남지 않는다.
- 서버가 멈춰 중단하지 못한 멀티파트 업로드는 버킷의 `AbortIncompleteMultipartUpload` 규칙이 지운다(`docs/configuration.md`의 "S3의 예").

**들어간 것(tracer-bullet 2c).** 올린 파일을 어느 블록도 가리키지 않으면(이미지·첨부 블록의 `fileId`, 카드 본문의 이미지. 휴지통의 블록까지 센다) 그렇게 된 지 `trash_retention_days`(기본 30일)가 지난 뒤 서버가 지운다. 그 뒤 `getFile`은 404다. 올리고 아직 어디에도 넣지 않은 파일도 올린 때부터 센다. 조작과 모양은 바뀌지 않았다.

## 5. 아카이브와 템플릿 (설계 3.10, 5.7)

**바뀐 뒤.**

- 아카이브 형식이 판 3이 된다. 계약의 타입(보드, 블록, 본문)을 그대로 쓴다. 옛 형식(판 1, 판 2)의 가져오기는 없어진다.
- `importArchive`는 4절의 올리기 세션을 쓴다(용도 `import`, 한도 256 MiB). 요청의 모양이 바뀐다.
- 내장 템플릿은 `crates/server/assets/templates/`의 파일이다. `getTemplates`의 모양은 그대로다.
- `onboard`가 복사하는 환영 보드의 내용도 같이 바뀐다.

**들어간 것.** 내장 템플릿 7개는 서버 크레이트의 파일이고(`crates/server/assets/templates/`, 한국어·영어로 한 파일씩) 바이너리에 들어간다. `getTemplates`와 `onboard`는 `Accept-Language`를 읽고, 그 보드의 블록이 담는 것이 바뀌었다.

- 본문이 보이는 그림과 문서는 직접 만든 작은 파일이고(설계 결정 18번), 템플릿을 넣을 때 시스템 조직의 파일로 저장된다. 템플릿에서 만든 보드는 제 사본을 갖는다. 시작할 때 파일 저장소가 이 파일을 받지 않아도 서버는 뜬다: 그동안 `getTemplates`는 DB에 있는 내장 템플릿만 내고(새 DB면 없다), 서버는 목록을 달라는 요청이 올 때와 1분마다 다시 넣어 본다. `ready`는 이 일로 달라지지 않는다.
- 카드의 내용은 본문(`body`)에 있다. 내장 템플릿에는 콘텐츠 블록(`text`, `checkbox`, `image` 등)이 없다.
- 날짜 속성의 값은 2절의 새 모양이다.
- `templateVersion`이 9가 된다. 8 이하의 내장 템플릿을 가진 DB는 다음 시작 때 템플릿을 새로 넣는다. 그 템플릿에서 이미 만든 보드(계정의 환영 보드 포함)는 그대로다.
- 내장 템플릿의 `properties`에 `templateKey`, `templateLanguage`, `templateBaseDate`가 있다. 템플릿마다의 이름(`welcome`, `issue-tracker` 등), 쓰인 언어(`ko`, `en`), 템플릿의 날짜가 기준으로 삼는 날(`YYYY-MM-DD`)이고, 제목이 바뀌어도 그대로다. 아무 클라이언트나 제 보드에 적을 수 있으므로, 내장 템플릿을 가려낼 때는 `isBuiltin`이 `true`인 보드에서만 이것을 본다.
- 내장 템플릿에서 만든 보드(`duplicateContainer`, `onboard`)는 이 셋과 `onboarding`을 갖지 않고, 날짜 속성의 값과 뷰가 날짜 속성을 거르는 날이 기준일에서 만든 날까지의 날수만큼 옮겨진다. 만든 날은 그 계정 설정의 시간대의 오늘이고, 설정이 기기를 따르면 UTC의 오늘이다.
- `getTemplates`는 내장 템플릿을 한 언어로만 낸다: 사용자 설정의 언어, 설정이 기기를 따르거나 템플릿이 없는 언어면 `Accept-Language`가 가장 앞세우는 서버의 언어(`duplicateContainer`가 사본의 제목을 짓는 언어와 같은 규칙), 둘 다 아니면 영어.
- `onboard`는 내장 템플릿 가운데 `properties.onboarding`이 `true`이고 같은 규칙으로 고른 언어의 보드를 복사하고 `{boardId}`만 답한다. 그런 보드가 없으면(템플릿을 지웠거나 시작할 때 넣지 못한 경우) 템플릿을 다시 넣고 복사한다. 템플릿을 넣을지 정하고 지우고 넣는 일은 한 트랜잭션이라, 여러 서버나 요청이 함께 해도 한 번만 넣는다. 조직을 시작한 계정은 `onboard`로 환영 보드를 받고, 구성원으로 들어온 계정(초대 링크, 링크로 한 가입의 메일·운영자 확인, 조직의 OIDC 제공자로 한 첫 로그인)은 묻지 않아도 들어온 워크스페이스에 자기만 보는 환영 보드 하나를 받는다. 계정마다 한 번이다. 이미 받았거나 `onboard`로 만든 계정은 다른 조직이나 워크스페이스에 들어가도 다시 받지 않고, 손님은 받지 않는다. 이 기능 전에 있던 계정은 받은 것으로 친다. 템플릿이 없는 서버에서는 주지 않고 들어오는 일은 그대로 된다.
- 템플릿을 새로 넣을 때 옛 내장 템플릿의 파일을 저장소에서도 지운다. 새 파일은 먼저 쓰고 같은 트랜잭션에서 기록한다.

**들어간 것(tracer-bullet 5b).** 아카이브는 판 3이다. 조작의 모양은 그대로이고, 계약의 설명에 형식을 적었다.

- 판 3은 판 2와 같은 ZIP이다: `version.json`(`{"version": 3, "date": <ms>}`), 보드마다 디렉터리 하나에 `board.jsonl`과 파일. `board.jsonl`의 줄은 `{"type": "board"|"block"|"boardMember", "data": …}`이고 `data`는 계약의 `Container`, `Block`(본문 포함), `ContainerMember` 모양이되, 블록과 멤버가 그릇을 가리키는 멤버는 계약의 `containerId`가 아니라 형식의 이름 `boardId`다(옛 아카이브와 같다). 보드 줄의 `data`에는 `fileNames`(저장 이름마다 파일의 이름)도 있어 가져오기가 저장하는 파일에 그 이름을 준다. 내보낼 때 찾지 못한 파일은 항목 없이 `missingFiles`(저장 이름의 목록)에 적히고, 가져오기는 그 이름의 항목이 있어도 저장하지 않으며 그것을 보이던 블록은 파일을 보이지 않는다. 올릴 때 받지 않을 이름(빈 이름, U+0000이 든 이름, 255자를 넘는 이름, 저장 이름과 확장자가 다른 이름)은 넘기고 그 파일은 항목의 이름을 지니며, 이 멤버가 없는 아카이브는 전처럼 읽는다.
- 블록이 보이는 파일이 모두 실린다. 카드와 댓글 본문의 이미지도 실린다. 파일이 없는 이미지·첨부 블록도 내보낼 수 있다(전에는 500. 8절에서 뺐다). 이미지 블록의 `fileId`는 클라이언트가 적은 대로라, 저장 이름이 될 수 없는 이름(`/`가 든 것, `board.jsonl`, `version.json`)에는 항목을 쓰지 않는다. 그래서 이 서버가 쓴 아카이브는 늘 다시 읽힌다.
- `importArchive`는 판 3만 받는다. 판 1(JSONL), 판 2, `version.json`이 없는 ZIP은 400이고 아무것도 만들지 않는다. 읽을 수 없는 항목(체크섬이 틀리거나 한도를 넘는 것)이 있는 아카이브도 무엇을 만들기 전에 400이다(9b부터 한도를 넘는 것은 413, 아래).
- 가져오기는 블록이 보이는 파일을 새 보드의 파일로 바꾼다. 이미지·첨부 블록의 `fileId`와 `attachmentId`, 본문의 이미지가 모두 바뀐다(8절의 결함에서 뺐다). 아카이브에 파일이 없는 이름은 블록의 필드에서 빈 이름이 되고, 본문의 이미지는 빈 문단이 된다(남는 것이 빈 문단뿐이면 본문이 없다). 복제도 같다.
- 가져오기는 속성의 종류에 맞지 않는 값을 `null`로 바꾼다(2절, 복제와 같다). 예전 모양의 날짜 값을 새 모양으로 바꾸는 일은 하지 않는다. 판 3의 아카이브에는 예전 모양이 없다.

**들어간 것(tracer-bullet 9a, 2026-11-11 10:55 KST).** 내보내기(`exportContainerArchive`, `exportWorkspaceArchive`)는 아카이브를 메모리에 모으지 않고 서버의 임시 파일에 다 쓴 뒤 보낸다. 응답에 `Content-Length`가 생겼다. 쓰다 실패하면 200을 보내기 전의 오류 응답이다(반쯤 나간 200이 없다). 아카이브의 내용과 형식은 그대로다. 가져오기의 올리기 세션은 9b다.

**들어간 것(tracer-bullet 9b, 2026-11-11 11:19 KST).** 가져오기가 4절의 올리기 세션을 쓴다. **`importArchive`의 요청 모양이 바뀌었다**(결정된 변경이다).

- 새 조작 `createImportUpload`(`POST /w/{workspaceId}/archive/uploads`). 요청과 답은 `createUpload`와 같다(`name`, `size`, `contentType` → `uploadId`, `partSize`, `partCount`, `expiresAt`). 권한은 가져오기와 같다(그 워크스페이스에서 일하는 사용자이고 서버의 게스트가 아님). `size`가 `maxImportBytes`를 넘으면 413이다.
- 부분은 `putUploadPart`로 보낸다(파일과 같다). 아카이브의 부분은 저장소가 S3여도 서버의 임시 자리에 받는다(가져오기가 ZIP을 임의 접근으로 읽는다).
- `importArchive`(`POST /w/{workspaceId}/archive/import`)의 본문은 `multipart/form-data`가 아니라 JSON `{"uploadId": "…"}`다. 답은 그대로 204다. 설계 5.6의 9번은 "완료가 가져오기를 실행한다"였는데, `completeUpload`의 답(`fileId`)과 권한(보드)이 파일의 것이라 가져오기는 제 조작에서 업로드를 이름으로 받게 했다. 빠진 부분이 있으면 400이고 업로드는 열린 채 남는다. 그 밖에는 가져오기가 성공하든 실패하든 업로드가 끝난다. 다른 사용자의 업로드와 끝난 업로드는 404, 파일의 업로드나 다른 워크스페이스의 업로드는 400(`errors`가 `uploadId`의 자리를 가리킨다)이다. 가져올 때 권한이 없으면 403이고 업로드를 버린다(완료와 같다).
- `completeUpload`에 아카이브의 업로드를 주면 400(`errors`가 `uploadId`의 자리를 가리킨다)이고 업로드는 남는다.
- `getUpload`의 답에 `purpose`(`file` | `import`)와 `workspaceId`가 생겼다. 아카이브의 업로드는 `containerId`가 빈 문자열, 파일의 업로드는 `workspaceId`가 빈 문자열이다.
- `getClientConfig`에 `maxImportBytes`(설정 `max_import_bytes`, 기본 256 MiB)가 생겼다. 사용자 한 명의 열린 업로드가 걸어 두는 크기의 한도는 `maxFileBytes`와 `maxImportBytes` 가운데 큰 것의 두 배가 됐다(기본 512 MiB).
- 아카이브의 한도(설계 4.2): 항목 10,000개, 풀린 합계 1 GiB, `board.jsonl` 64 MiB(전에는 70 MiB이고 400), 파일 항목 하나는 `maxFileBytes`(전에는 항목마다 256 MiB이고 400). 넘으면 413이고 `limit`이 그 한도다(항목 수는 개수, 나머지는 바이트). 무엇을 만들기 전에 모든 항목을 한 번 읽어 보므로 한도를 넘는 아카이브는 아무것도 만들지 않는다.
- 맨 위의 `version.json`이 64 KiB를 넘으면 이 서버의 아카이브가 아니라서 400이다(다른 판의 것과 같다). 보드 디렉터리 안의 같은 이름은 파일 항목의 한도(`maxFileBytes`)를 받고, 가져오지는 않는다.
- 항목 수는 ZIP의 디렉터리를 읽기 전에 파일 전체에서 중앙 헤더의 서명을 세어 본다. 끝 레코드가 적은 수를 말해도 헤더가 10,000개를 넘으면 413이다. 이것은 읽어 들이는 항목의 수를 묶을 뿐, ZIP 크레이트가 디렉터리를 읽기 전에 끝 레코드가 말하는 수만큼 잡아 두는 자리(가상 메모리. 꾸미면 아카이브 크기의 약 3배)는 묶지 못한다. 보통의 메모리 과할당 설정에서는 쓰이지 않고 400으로 끝나지만, 과할당을 막은 기기에서는 프로세스가 멈출 수 있다(사용자에게 알렸고 코드는 그대로 둔다. `docs/configuration.md`의 `max_import_bytes`). 압축하지 않고 저장한 ZIP(첨부한 `.zip`, `.xlsx` 등)을 담은 아카이브는 그 안의 헤더도 세어진다. 이 서버가 내보낸 아카이브는 항목을 모두 압축하므로 해당하지 않는다.
- 서버는 아카이브를 메모리에 올리지 않는다. 부분을 임시 자리에서 차례로 한 파일에 이어 붙이며(붙인 부분은 지운다) 그 파일에서 항목을 하나씩 읽고, 파일 항목은 임시 파일로 풀어 저장소에 흘려 쓴다. 가져오는 동안 임시 자리는 아카이브 하나와 부분 하나, 파일 항목 하나만큼 쓴다. 가져오는 중인 아카이브는 열린 업로드의 한도에 들지 않는다. 동시에 하는 가져오기와 내보내기는 한 사람에 하나, 서버 전체에 설정값(기본 2)까지이고 넘으면 429다(사용자 결정 2026-11-11 14:25 KST).

## 6. 계약 밖에 있는 것

- **WebSocket(`/ws`)의 메시지.** OpenAPI는 WebSocket을 적지 못한다. 메시지는 계약 파일 밖의 `contracts/websocket.md`(사람이 읽는 것)와 `contracts/websocket.json`(멤버와 타입, 서버의 테스트가 실제 메시지를 견준다)에 적었다(2026-11-11 04:14 KST). 카드 본문을 함께 고치는 방의 명령·메시지·Binary 프레임(커서, 쥔 문서로 다시 들기까지)은 그 6절이다(2026-11-15 01:40 KST). 죽은 연결을 알아채는 `PING`·`PONG`과 90초 동안 조용한 연결의 닫힘은 3절이다(2026-11-15 19:02 KST).
  - 핸드셰이크의 인증, 명령 여덟, 메시지 스물, 어느 조작이 어느 메시지를 내는지, 서버가 구독을 끝내고 연결을 닫는 때가 그 문서에 있다. 웹앱의 질문 G1–G6의 답도 그 문서의 5절에 있다.
  - 메시지가 싣는 보드, 블록, 카테고리, 멤버는 계약의 `Container`, `Block`, `Category`, `ContainerMember`와 같은 모양이다.
  - `UPDATE_BLOCK`이 싣는 블록은 1절의 본문(`body`)을 함께 싣는다.
- **오류의 상태 코드.** 조작마다 4xx를 하나씩 적지 않았다.
  - 모든 조작의 `default` 응답이 `application/problem+json`의 `Problem`(RFC 9457)이다. 어느 상태가 무슨 뜻인지는 계약의 머리말과 `default`의 설명에 있다. 거절된 입력은 `errors`에 자리(`pointer`, `parameter`, `header`)와 함께 온다.
  - 조작에 고유한 거절(`reason`)은 그 조작의 설명에 있고, 모두는 `Problem`의 `x-problem-types`와 `/problems`에 있다.
  - 429와 `Retry-After`는 `login`, `register`, `changePassword`에, 412와 416은 `getFile`에 따로 적었다. 모두 같은 `Problem`이다.
- **관리자 소켓의 라우트**(`/api/v1/admin/...`). 로컬 유닉스 소켓에서만 받는다. 계약에 없다.
- **Prometheus 지표**(`/metrics`). API와 다른 리스너(`metrics_address`)에서만 낸다. API의 리스너에는 없다(`docs/configuration.md`의 "키: 지표").
- **응답 압축**(tracer-bullet 10, 2026-11-11 11:51 KST). 조작마다 적지 않는다. 모양과 뜻은 바뀌지 않는다.
  - API의 JSON 응답(`Content-Type: application/json`, 32바이트 이상인 것)은 요청의 `Accept-Encoding`에 따라 `zstd`, `br`, `gzip`으로 압축한다. 같은 가중치면 zstd, brotli, gzip 순이다. `deflate`만 받는 클라이언트에는 압축하지 않는다. 압축한 응답은 `Content-Encoding`을 달고 `Content-Length`가 없으며 `Vary: Accept-Encoding`이 붙는다. 브라우저는 스스로 푼다.
  - 내보내는 아카이브와 파일(`getFile`), 그 밖에 JSON이 아닌 응답과 오류 응답(`Problem`)은 압축하지 않는다. 그래서 내보내기의 `Content-Length`와 `getFile`의 범위 요청은 그대로다.
  - `Accept-Encoding`이 압축하지 않은 응답을 거절해도(`identity;q=0`, `*;q=0`) 406으로 답하지 않는다. 그 항목은 없는 것으로 보고, 받을 수 있는 압축이 없으면 압축하지 않은 응답을 한 일의 상태 그대로 보낸다.
  - 설정 `compress_responses`(기본 참)로 끈다. 압축하는 프록시나 CDN 뒤의 배포를 위한 것이다.
  - WebSocket 메시지는 압축하지 않는다.
  - 웹앱의 정적 파일은 빌드가 만든 `.zst`, `.br`, `.gz`가 옆에 있으면 `Accept-Encoding`에 따라 그것을 보낸다(전에는 `.br`과 `.gz`만).

## 7. 그대로 둔 불규칙

바꿀 결정이 난 것은 아니지만 계약에 그대로 적힌 모양이다. 고치게 되면 계약을 고치는 변경으로 한다. 칸에 다 적지 못한 것은 표 아래에 같은 번호로 풀어 적었다.

| 무엇 | 지금 |
|---|---|
| ID 목록으로 묻는 조회가 `POST`다 | `getWorkspaceUsersById`(`POST /w/{workspaceId}/users`). 못 보는 사람의 ID는 없는 ID처럼 빠지고, 두 번 적은 ID는 한 번 나온다 |
| 검색어 없는 `getWorkspaceUsers` | 모든 사용자가 아니라 가장 오래된 열 명이다(검색과 같은 한도) |
| `searchWorkspaceBoards` | 경로의 워크스페이스로 좁히지 않는다. 사용자가 찾을 수 있는 모든 워크스페이스의 보드를 찾는다 |
| 동작을 경로 끝에 붙인 라우트 | `duplicate`, `undelete`, `join`, `leave`, `hide`, `unhide`, `onboard` |
| 구독의 경로 | `DELETE /subscriptions/{blockId}/{subscriberId}`, `GET /subscriptions/{subscriberId}`. 아래 (1) |
| 구독자의 종류 `channel` | 스키마 `SubscriberType`에 있으나 서버는 `user`만 받는다 |
| `deleteAt` | 있는 것에서는 늘 0이다(지운 것은 휴지통에 있다). WebSocket의 삭제 알림이 쓴다 |
| 워크스페이스의 `title`, `settings` | 값을 정하는 라우트가 없다. `getMyWorkspaces`는 `title`을 `name`이라 부른다 |
| `ping`의 멤버 이름 | `db_type`, `build_number` 등 snake_case다. 주소는 아래 (2) |
| 요청 스키마와 서버의 해석 | 서버는 계약의 요청 스키마보다 너그럽게 읽는다. 아래 (3) |
| `NewBlock`의 `createAt`, `updateAt` | 필수이고 1 이상이어야 하지만 서버는 그 값을 쓰지 않는다 |
| `NewCategory`, `CategoryUpdate`의 `userId`, `workspaceId`(와 고칠 때의 `id`) | 본문에도 있어야 하고 세션·경로의 값과 같아야 한다. 아래 (4) |
| `createCategory`에 `createAt`, `updateAt`을 보내면 | 계약의 `NewCategory`에는 없지만 0이 아닌 값이 오면 서버가 그 값을 쓴다 |
| `NewBoard`의 `templateVersion` | 클라이언트가 정할 수 있다 |
| `disableNotify` | 받지만 API로 보이는 효과가 없다(알림을 보내는 쪽이 아직 없다) |
| 빈 목록을 넣는 `createBlocks` | 201과 `[]` |
| 이미 있는 구독을 다시 만드는 `createSubscription` | 201 |
| 이미 멤버인 사용자를 넣는 `addMember` | 200과 지금의 멤버십. 역할은 바뀌지 않는다 |
| `patchCard`의 `updatedProperties`와 `patchBlock`의 `updatedFields.properties` | 앞의 것은 보낸 값만 합치고, 뒤의 것은 값 전체를 바꾼다. 아래 (5) |
| `getFile`의 `Content-Type` | 모든 파일이 `application/octet-stream`으로 나간다. 아래 (6) |
| `searchWorkspaceBoards`의 `field=property_name` | 카드 속성의 이름이 아니라 보드 자신의 `properties` 객체에 그 이름의 멤버가 있는지를 본다. 정확히 같은 이름만 찾는다 |
| 역할이 하나도 없는 멤버 | `addMember`, `updateMember`에 역할을 모두 빼고 보내면 보기 권한도 없는 멤버가 된다 |
| `removeMember`와 `updateMember` | 멤버가 아닌 사용자를 빼면 204, 고치면 404다 |
| `createCategory`의 `sortOrder` | 보낸 값에 10을 더해 저장하고, 그 사용자의 다른 카테고리도 모두 10씩 민다 |
| `type: system`인 `createCategory` | 받는다. 그렇게 만든 카테고리는 이름을 바꾸거나 지울 수 없다 |
| `hideContainer`, `unhideContainer` | 그 카테고리에 없는 보드여도 204다 |
| `deleteContainer` 뒤의 `undeleteContainer` | 공유(읽기 토큰), 사이드바의 자리, 구독은 돌아오지 않는다 |
| `deleteBoardsAndBlocks`의 `blocks` | 검사만 한다. 보드가 자기 블록을 모두 데리고 휴지통에 가고, `undeleteContainer`로만 돌아온다 |
| `exportWorkspaceArchive` | 없는 워크스페이스도, 사용자가 들지 않은 워크스페이스도 200이다(보드 없는 아카이브) |
| `importArchive` | 전부 아니면 전무가 아니다. 아래 (7) |
| `onboard` | 부를 때마다 보드를 하나 더 복사한다. 답이 200이다. 아래 (8) |
| `createSubscription`의 `blockType` | 블록의 실제 종류와 견주지 않고 보낸 대로 저장한다 |
| 세션 없는 큰 요청 | JSON 본문이 64 KiB를 넘으면 401보다 먼저 413이다 |

1. **구독의 경로.** 라우터에서는 첫 자리의 이름이 하나(`blockOrSubscriberId`)다. 계약에서만 `{blockId}`와 `{subscriberId}`로 갈라 적는다.
2. **`hello`와 `ping`의 주소.** 둘도 `/api/v1` 아래에 있다(`/api/v1/hello`, `/api/v1/ping`). 예전 주소 `/hello`, `/ping`은 다른 모든 주소처럼 웹앱의 화면 주소로 다뤄진다.
3. **요청 스키마와 서버의 해석.**
   - 만들기와 고치기의 요청은 계약에 따로 적었다: `NewBoard`, `NewBlock`, `NewCard`, `NewCategory`, `CategoryUpdate`, `NewMember`, `MemberRoles`, `SharingRequest`, `NewSubscription`, `AdminChange`, `SettingsChange`.
   - 서버는 이보다 너그럽게 읽는다. 모르는 멤버를 대부분 버린다.
   - 예외로 400을 답하는 곳이 있다. `patchUserSettings`와 `patchMe`는 모르는 멤버를 거절한다. `createBoard`와 `createCategory`는 `id`를, `setSharing`은 `token`을 거절한다.
   - 스키마가 필수로 적은 멤버가 빠져도 거절하지 않고 빈 값으로 읽는 곳이 있다. `login`의 `password`가 없으면 401이고, `patchBlocks`의 목록이 없으면 204다.
   - 그러니 클라이언트는 계약에 없는 멤버를 보내지 않는다.
4. **카테고리 요청의 `userId`, `workspaceId`.** 세션과 경로가 이미 말하는 값이다. 세션과 다른 `userId`는 만들 때 403, 고칠 때 400이다.
5. **카드의 속성 값을 고치는 두 길.** `patchCard`는 `updatedProperties`의 값을 저장된 값에 합치고 `deletedProperties`에 적힌 속성의 값을 지운다(둘에 다 적힌 속성은 지워진다). 보내지 않은 값은 그대로라, 같은 카드의 다른 속성을 거의 동시에 고친 두 요청이 둘 다 남고, 같은 속성이면 나중 것이 남는다. 합치기는 저장소가 보드를 잡은 한 트랜잭션에서 한다. `patchBlock`의 `updatedFields`에 `properties`를 보내면 값 전체가 그것으로 바뀐다(블록의 필드 하나를 바꾸는 규칙 그대로).
6. **`getFile`의 머리글.** 모든 파일이 `application/octet-stream`과 `Content-Disposition: attachment`로 나간다. 파일 이름은 올린 이름이 아니라 `fileId`다. `Last-Modified`가 응답 시각이라 재검증으로 내려받기를 아끼지 못한다.
7. **`importArchive`.** 모든 보드를 읽고 검사한 뒤 하나씩 만든다. 검사는 줄의 모양, 블록의 종류, 보드와 블록이 맞는지, 그리고 저장소가 보는 것(보드의 속성 정의, 블록의 제목·`fields`의 종류별 한도·속성 값·본문)이다. 그 검사에 걸리면 아무것도 만들지 않는다. 그 뒤에 실패하면(DB의 오류, 그리고 `board.jsonl` 하나에 보드가 여럿이고 블록의 부모가 자기 것이 아닌 보드인 경우. 이 서버의 아카이브에는 없다) 앞의 보드는 남는다. 크기의 한도는 5절에 있다.
8. **`onboard`의 200.** 만든 것을 201로 답하는 규칙의 예외다.

## 8. 알려진 결함

계약이 약속하지 않는 동작이다. 클라이언트는 여기에 기대지 않는다. 다음 작업 묶음에서 고칠 후보이고, 고치면 이 절에서 뺀다.

지금은 없다(2026-11-11 12:37 KST, tracer-bullet 12에서 모두 고쳤다).


## 9. 웹앱의 질문에 대한 답 (`apps/web/docs/contract-assumptions.md`, 2026-11-11 06:40 KST 판)

답은 이 브랜치의 서버가 지금 하는 것이다. "결정됨"은 사용자가 정한 것이다(2026-11-11). 실시간(G1–G6)의 답은 `contracts/websocket.md` 5절에 있다.

| 항목 | 답 |
|---|---|
| A1 | 이름은 가정대로 `hasBody`, `tasksTotal`, `tasksDone`(`Card`) |
| A2, H6 | **결정됨(11:05 KST), 구현됨.** `patchCard`의 `updatedProperties`는 보낸 값만 저장된 값에 합치고, 값을 지울 속성은 `deletedProperties`(속성 식별자의 목록)로 받는다(7절 (5)). 웹앱에 달라지는 것: (1) 읽은 뒤 값 전부를 `updatedProperties`로 보내도 저장은 되지만 그것으로 값이 지워지지 않는다. 빠진 속성은 그대로 남는다. 값을 지우려면 그 속성을 `deletedProperties`에 적는다. (2) 마지막 값을 `patchBlock`의 `updatedFields.properties`로 비우는 길은 필요 없다(그 길은 여전히 값 전체를 바꾼다). (3) 고친 속성만 보내면 남이 다른 속성을 고친 것을 덮지 않는다. 한 카드의 고치기를 차례로 보낼 까닭이 사라진다. 메시지(`UPDATE_BLOCK`)는 전처럼 합친 뒤의 블록 전체다 |
| A3 | 본문은 `patchCard`(와 `patchBlock`)의 `body`로, 같은 라우트다. `body`에는 `readAt`이 있어야 하고, 제목·속성 패치에는 없어도 되지만 보내면 늘 비교한다 |
| A4 | 409의 기준은 블록의 `updateAt` 하나다. 본문만의 판은 없다. 지금대로 둔다, 그 계획에서 다시 본다 |
| A5 | 된다. `createCard`의 `NewCard`에 `body`가 있다 |
| A8 | **결정됨.** 댓글은 카드의 `updateAt`을 올리지 않는다. 카드 목록·`getCard`·`patchCard`의 `lastCommentAt`이 마지막 댓글을 단 시각이다(없으면 0, 지운 댓글은 세지 않는다) |
| B1, H4 | 댓글은 `type=comment`, `parentId`=같은 보드의 카드인 블록이고(위키에서는 같은 위키의 페이지: `getPageComments`와 그 조작. 고치고 지우고 홀로 지운 것을 되살리는 권한(`undeletePageComment`)은 카드의 것과 같다)(서버가 지킨다: 댓글 아래 블록 없음, 어기면 400, 되살리기는 409) `getBlocks?parentId=&type=comment`로 읽는다. 댓글도 카드와 같은 `body`를 가진다(1절). 쓰기는 `createBlocks`(댓글 권한부터)와 `patchBlock`·`patchBlocks`(`readAt`과 함께). **결정됨(14:25·22:13 KST), 구현됨**: 자기 댓글은 댓글 권한으로 글(`body`)을 고치고 지우고(`deleteBlock`) 자기가 지운 것을 되살리며(`undeleteBlock`), 남의 댓글과 남이 지운 자기 댓글은 보드 관리자만 한다(403, `access denied to change`·`delete`·`undelete other users' comments`, 남이 지운 자기 댓글은 `a comment somebody else deleted is brought back by the board's admins`). 묶음 길(`patchBoardsAndBlocks`, `deleteBoardsAndBlocks`)도 같다. 댓글을 옮기거나(`parentId`) 다른 종류로 바꾸는 패치는 편집자부터, 남이 만든 블록을 댓글로 바꾸는 패치는 관리자만이다. 되살리면 함께 지워진 것이 함께 오므로 지워진 맨 위의 블록이 정한다. 고친 사람과 시각은 댓글의 `modifiedBy`·`updateAt`이다 |
| B2 | 블록에는 `fileId`뿐이다. 이름·크기·확장자는 `getFileInfo`(`FileInfo`). 종류(MIME)는 없다 |
| B3 | 아니다. `completeUpload`·`uploadFile`은 파일만 만들고(`UploadedFile`의 `fileId`) 첨부 블록은 클라이언트가 만든다 |
| B4 | 답(`UploadedFile`)은 `fileId`뿐이다. 이름·크기는 `getFileInfo` |
| B5 | 맞다. 파일은 `getFile`(`/containers/{containerId}/files/{fileId}`)로 받는다 |
| B6 | `getClientConfig`의 `maxFileBytes`, `maxImportBytes`. 조각 크기는 `createUpload`의 답(`partSize`) |
| C1, 6절 작업 공간 3 | `createBoardsAndBlocks`가 보드와 첫 뷰를 한 요청으로(한 트랜잭션) 만든다. 서버가 기본 뷰를 넣지는 않는다 |
| C5 | 맞다. 지워진 보드, 없는 보드, 볼 수 없는 보드는 모두 404다. 열린 보드(`O`)는 구성원이 아니어도 보드는 읽지만, 카드·블록·구성원은 403이다 |
| C3 | `cardOrder`는 뷰의 `fields`에 그대로 둔다 |
| D1 | 지금 워크스페이스는 조직마다 하나다. 지금대로 둔다, 그 계획에서 다시 본다 |
| D2, H1 | 검색 없이 오래된 열 명, 검색으로 열 명까지(대소문자 무시). 구성원의 사용자는 `getMembers` 뒤 `getWorkspaceUsersById`. 워크스페이스 전부를 읽는 길은 없다. 지금대로 둔다, 그 계획에서 다시 본다 |
| D4 | `getClientConfig`의 `nameDisplay` |
| E1 | 가정과 다르다: `containerMetadata: [{containerId, hidden}]` |
| E2, 6절 작업 공간 1 | 넣기(`setContainerCategory`)와 순서(`reorderCategoryContainers`, 그 카테고리의 보드 ID 전부를 새 순서로)가 따로다. 넣기에는 자리가 없다 |
| E3 | 넣어 준다. 템플릿이 아닌 새 보드와 새로 든 보드는 그 사용자의 기본 카테고리에 들어간다 |
| F2 | **결정됨.** 계정 설정의 `language`: `"auto"`(브라우저를 따른다) 또는 언어 태그. 서버는 모양만 본다 |
| 6절 작업 공간 2 | **결정됨.** 지운 보드는 사이드바에서 빠지고, 되살리면 사람마다 원래 카테고리의 원래 자리로(그사이 지운 카테고리면 기본 카테고리로) 돌아오며 `UPDATE_CONTAINER_CATEGORY`와 `REORDER_CATEGORY_CONTAINERS`가 간다 |
| 6절 작업 공간 4 | 보드 목록에 역할은 없다. 역할은 `getMyMemberships`. 지우기는 보드 관리자만 |
| 6절 작업 공간 5 | 기본 카테고리의 이름은 바뀌지 않는다(200으로 답하되 이름은 그대로). 메뉴에서 뺀다 |
| 6절 작업 공간 6 | 새 카테고리는 맨 위다(`reorderCategories` 뒤에도) |
| 6절 작업 공간 7 | 맞다. `reorderCategories`는 카테고리 ID 전부를 새 순서로 받는다 |
| 6절 작업 공간 9 | 맞다. 서버가 모르는 시간대 이름은 400(`errors`) |
| 6절 뷰 머리글 | 서버는 뷰의 `fields`를 크기(80만 자) 말고 검사하지 않는다. 뷰를 고치는 권한은 카드와 같다(보드 편집자). 보드의 마지막 뷰를 지우는 것도 막지 않는다 |
| 6절 칸반 1 | **결정됨.** `getCards?preview=true`면 카드마다 `preview`(`imageFileId`, 글 앞 200자). 읽기 토큰으로도 같다 |
| 6절 칸반 2, 카드 상세 가정 2, 카드 상세 질문 2 | 선택지나 속성을 지워도 서버는 카드의 값도 뷰의 `fields`도 건드리지 않는다. 카드를 쓸 때는 바뀌는 값만 보므로, 정의 없는 값이 그대로 남아 있는 것을 이유로 거절하지 않는다(정의 없는 속성에 날짜 값을 새로 쓰면 거절한다, 2절) |
| 6절 카드 상세 가정 1 | 속성 정의와 선택지의 ID는 클라이언트가 만든다. 정의의 ID는 비지 않았는지와 겹치지 않는지 보고, 선택지의 ID는 문자열인지만 본다 |
| 6절 카드 상세 가정 4 | 된다. `getFile`은 같은 출처에서 `Content-Disposition: attachment`로 보낸다(7절 (6)) |
| 6절 카드 상세 질문 3 | 날짜 값만, 카드를 쓸 때 바뀌는 값을 검사한다(2절): 날짜 속성에는 날짜 값이나 `null`, 다른 속성이나 정의 없는 속성에는 날짜 값이 아닌 것. 종류를 바꿔도 값은 그대로 두고, 그 값을 다시 쓸 때 새 규칙으로 본다 |
| 6절 카드 상세 질문 4 | **결정됨.** `reorderCardProperties`(`PUT /boards/{boardId}/property-order`)가 속성 ID 전부를 새 순서로 받는다 |
| 6절 카드 상세 질문 6 | 댓글만 달 수 있는 사람은 파일을 올리지 못한다(보드 편집자부터) |
| H2 | **결정됨.** `User.displayName`은 서버가 이름의 칸(성·이름, 영문 이름, 직접 쓴 이름)과 고른 것(`nameShown`)으로 짓는다(`contracts/name-vectors.json`, 칸은 `patchMe`로). 사용자 검색은 로그인 이름과 이름의 칸에서 찾는다. 조직·워크스페이스의 구성원 목록에도 이름의 칸·호칭·학위가 오고, 이름이 바뀌면 `UPDATE_USER`(`contracts/websocket.md`)가 간다 |
| H3 | **결정됨.** `getDeletedCards`(보드의 편집자부터)와 `getDeletedContainers`(관리자였던 보드만). 되살리기는 `undeleteBlock`·`undeleteContainer` |
| H5 | **결정됨(11:05 KST).** 웹앱이 "참여" 단계를 두고 서버의 `joinContainer`는 그대로다 |
| G5 | **결정됨(2026-11-22).** 남이 본문을 고치면 카드·페이지·일기 항목의 `UPDATE_BLOCK`은 본문 대신 요약(`summary`)을 싣는다(`contracts/websocket.md` 3절). 댓글과 웹훅은 그대로다 |
| H7 | 카테고리의 네 메시지는 그 사용자가 그 워크스페이스를 구독한 연결 모두에 간다(tracer-bullet 13) |

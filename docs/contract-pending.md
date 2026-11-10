# 계약에서 곧 바뀔 부분

`contracts/openapi.json`은 지금 서버가 하는 일을 적은 것이다. 이 문서는 그 가운데 바뀌기로 결정됐고 아직 구현되지 않은 부분을 모은다. 웹앱은 이 문서에 든 조작과 멤버를 나중에 쓰거나, 쓰더라도 얇게 감싸 둔다. 여기에 없는 것은 바꿀 계획이 없다.

바뀌는 때는 서버의 다음 작업 묶음이다. 그 묶음은 설계 `docs/s6-design.md`의 "구현 순서" 8단계(본문 저장, 날짜 값, 템플릿)와 9단계(청크 업로드, 스트리밍, 파일 한도)를 한다.

읽는 법:

- 각 절의 "지금"은 계약 파일에 있는 것이고 "바뀐 뒤"는 설계의 결정이다.
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

## 1. 카드와 댓글의 본문 (설계 5.7)

**지금.** 카드의 본문은 카드의 자식 블록들이다(`type`이 `text`, `image`, `divider`, `checkbox`, `h1`–`h3` 등). 순서는 카드의 `fields.contentOrder`에 있고 `moveContentBlock`이 그것을 고친다. 댓글의 글은 댓글 블록의 `title`에 마크다운으로 있다.

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
| `createBlocks`, `patchBlock`, `patchBlocks`, `createCard`, `patchCard`, `createBoardsAndBlocks`, `patchBoardsAndBlocks` | `body`를 받고 검증한다. 어긋나면 400이고 `field`가 `body`다 |
| `body`의 한도 | 직렬화한 JSON 1 MiB, 깊이 32, 노드 20,000개 |
| `image` 노드의 `fileId` | 같은 보드의 파일이어야 한다. 아니면 400 |

**들어간 것(tracer-bullet 1, 2026-11-11 04:45 KST).** 아래는 계약 파일에 이미 있다. 표의 나머지는 아직이다.

- `Card`에 `body`, `hasBody`, `tasksTotal`, `tasksDone`가 늘 있다. `getCard`와 `createCard`·`patchCard`의 답은 `body`에 문서를, `getCards`는 `body`에 `null`을 싣고 나머지 셋으로 본문이 있는지와 할 일 수를 알린다. 목록은 본문을 읽지 않고 저장할 때 센 열을 읽는다.
- `Block`에 `body`가 늘 있다(없으면 `null`). WebSocket의 `UPDATE_BLOCK`도 `Block` 전체이므로 본문을 싣는다(`contracts/websocket.md`).
- `createCard`가 `body`를 받는다. 빠졌거나 `null`이거나 빈 문서(빈 문단 하나)면 본문이 없다.
- `patchCard`가 `body`와 `readAt`을 받는다. `body`가 빠지면 본문은 그대로, `null`이나 빈 문서면 지운다. **`body`를 보내려면 `readAt`(그 카드를 읽었을 때의 `updateAt`)이 있어야 한다.** 없으면 400이고 `field`가 `readAt`이다. `readAt`이 지금의 `updateAt`과 다르면 409(`conflict`)이고 아무것도 바뀌지 않는다. `readAt`은 `body` 없이 보내도 검사한다. 패치를 적용한 블록이 블록의 한도(제목, `fields`)를 넘으면 `readAt`을 보기 전에 400이다. 부모의 검사는 그 뒤라서, 부모가 틀리고 `readAt`도 낡았으면 409다.
- 검증은 허용 목록 판 2 전체다(tracer-bullet 2, 2026-11-11 05:29 KST): 노드와 마크의 종류, 속성과 그 값, 어느 노드가 어디에 올 수 있는지, 한도(1 MiB, 깊이 32, 노드 20,000). 길이는 UTF-16 코드 단위로 센다. 어느 문자열과 멤버 이름에도 U+0000이 있으면 거절한다(PostgreSQL의 `jsonb`가 담지 못한다. 웹앱의 검증기에는 아직 없는 규칙이다). 허용 목록을 통과한 문서가 빈 문단 하나뿐이면(`attrs`가 있어도) 본문이 없는 것이다. 어긋나면 400이고 `field`가 `body`다. 다만 JSON이 128단보다 깊게 겹친 요청은 디코더가 먼저 거절해 `field`가 없는 400이다(본문 한도는 32단이라 그 앞에서 늘 걸린다).
- 서버가 저장할 때 `body_version`(2)과 검색용 평문 `body_text`를 함께 적는다. 평문은 문단·제목·코드 블록·수식 블록마다 한 줄이고 글자가 없는 블록의 줄은 뺀다. 줄바꿈(`hardBreak`)은 그 블록의 줄 안의 줄바꿈으로 남는다. 인라인 수식은 제자리에 원문으로 들어가고, 멘션은 아무것도 남기지 않는다(이름을 저장하지 않는다). 웹앱의 `plainText`는 멘션을 `@이름`으로 쓰므로 다르다. 평문은 아직 어느 응답에도 실리지 않는다.
- 블록의 경로(`createBlocks`, `patchBlock`, `patchBlocks`, `createBoardsAndBlocks`, `patchBoardsAndBlocks`)도 카드의 본문을 받는다(tracer-bullet 2b, 2026-11-11 05:55 KST). 검증과 거절(400, `field: body`)은 카드의 경로와 같다. 패치가 `body`를 바꾸려면(`null`도) `readAt`이 있어야 하고, 없으면 400이고 `field`가 `readAt`이다. **본문은 카드와 댓글만 갖는다.** 다른 블록에 본문이 있으면 400이고 `field`가 `body`다.
- **본문의 이미지는 그 블록의 보드의 파일이다**(설계 5.7). `image`의 `fileId`가 그 보드가 가진 파일의 저장 이름이 아니면 400이고 `field`가 `body`다. 본문을 받는 경로가 모두 같다(아카이브 가져오기는 아직 아니다. 8절). `createBlocks`에 `sourceBoardId`를 주면 그 보드의 파일이어야 하고(받는 보드에 이미 있는 파일이어도 거절한다), 블록과 함께 파일이 복사된다. `createBoardsAndBlocks`의 보드는 새것이라 파일이 없으므로 이미지가 있는 본문을 무엇도 찾아보지 않고 거절한다. 본문이 있는 카드나 댓글을 그 둘이 아닌 `type`으로 바꾸는 패치는 본문을 보내지 않았어도 400이고 `field`가 `body`다.
- **댓글의 본문**(tracer-bullet 3). 댓글 블록도 `createBlocks`, `patchBlock`, `patchBlocks`, `createBoardsAndBlocks`, `patchBoardsAndBlocks`로 `body`를 받는다. 검증, `readAt`, 이미지가 그 보드의 파일이어야 하는 규칙, 저장되는 평문까지 카드와 같다. 댓글의 권한(누가 만들고 고치고 지우는가)은 그대로다. 그래서 댓글만 쓸 수 있는 멤버(commenter)는 파일을 올리지 못해 댓글에 새 그림을 넣지 못하고, `sourceBoardId`로 파일을 복사하게 되는 요청은 편집 권한을 요구한다(없으면 403). 댓글의 `title`은 아직 받고 돌려준다(웹앱이 `body`로 옮겨 간 뒤에 걷어 낼지는 5절의 콘텐츠 블록과 함께 본다). 카드·보드 복제는 지금처럼 댓글을 복사하지 않는다. `sourceBoardId`로 다른 보드의 댓글을 만들면 그 본문의 파일도 복사된다.
- 카드를 복제하면(블록·보드 복제, 템플릿에서 보드 만들기) 본문의 이미지가 가리키는 파일도 복사되고, 복사본의 본문은 새 파일을 가리킨다.
- `BlockPatch.readAt`은 지금 쓰인다: `patchBlock`, `patchBlocks`, `patchBoardsAndBlocks`의 패치에 있으면 `patchCard`처럼 검사하고 어긋나면 409다.
- 있는 블록을 고칠 때마다 `updateAt`이 앞의 값보다 커진다(같은 밀리초여도). `readAt`이 그 사이의 변경을 놓치지 않는다.
- 휴지통에서 되살린 카드와 복제한 카드는 본문과 할 일 수를 그대로 갖는다.
- **사용자 결정 필요: 공유 보드의 익명 독자와 멘션.** 익명 독자(읽기 토큰)에게는 블록의 `createdBy`·`modifiedBy`를 비운다(결정 7: UUIDv7이 가입 시각을 드러낸다). 그런데 본문의 `mention.userId`도 같은 사용자 ID라 익명 독자에게 그대로 간다. 권고: 익명 독자에게 나가는 본문에서 `userId`를 비운 멘션으로 바꾼다(화면은 "알 수 없는 사용자"로 그린다). 결정 7의 범위를 넓히는 일이라 정하지 않았다.

**들어간 것(tracer-bullet 6, 2026-11-11 08:42 KST).** 표의 나머지 넷(블록의 `type`, `contentOrder`, `moveContentBlock`, `fields` 한도)이 들어갔다. 이로써 이 표는 모두 계약 파일에 있다.

- `moveContentBlock`이 없다. 그 경로는 404다.
- `Card`, `CardPatch`, `createCard`의 몸에서 `contentOrder`가 없어졌다. 보내도 무시하고 저장하지 않는다. 전에 저장된 카드 블록의 `fields.contentOrder`는 그대로 남고 읽지 않는다. 그래서 `contentOrder`가 중첩 목록인 카드(원본 웹앱의 나란히 놓기)도 이제 카드로 읽힌다(8절에서 그 경우를 뺐다).
- 새 블록의 `type`은 `card`, `view`, `comment`, `attachment`만 된다. 다른 종류는 `createBlocks`와 `createBoardsAndBlocks`에서 400이고 `field`가 `type`이다. 아카이브 가져오기도 400이다(`field` 없이, 메시지가 블록과 종류를 적는다). 어느 쪽이든 아무것도 저장하지 않는다. 패치(`patchBlock`, `patchBlocks`, `patchBoardsAndBlocks`)가 블록을 그런 종류로 바꾸는 것도 400이고 `field`가 `type`이다. 블록이 가진 종류를 그대로 적은 패치는 받는다.
- `fields`의 한도는 직렬화한 JSON의 글자 수로 카드·댓글·첨부 64,000자, 뷰 800,000자다. 만들 때와 패치할 때 모두 본다. 세는 단위는 지금까지의 800,000자와 같은 유니코드 글자다(본문의 UTF-16과 다르다). 전에 저장된 다른 종류의 블록은 800,000자 그대로다. 전에 저장된 카드·댓글·첨부가 64,000자를 넘으면 그 블록을 고치는 패치(제목만 바꾸는 것도)와 그 블록이나 그것이 든 보드의 복제가 400이다. `fields`를 64,000자 아래로 줄이는 패치는 받는다.
- **사용자 결정 필요: 저장된 콘텐츠 블록.** 이 서버가 S7 전에 저장한 콘텐츠 블록(`text`, `image`, `checkbox`, `divider`, 제목들과 서버가 모르는 종류)은 그대로 있다. 읽히고(`getBlocks`), 고쳐지고, 지워지고, 그 블록을 복제하거나(`duplicateBlock`) 그것이 든 카드나 보드를 복제하면(템플릿에서 보드 만들기도) 새 블록으로 복사되고, 내보내기에 실린다. 그런 블록이 든 보드의 아카이브는 가져오기가 거절한다. 그 행들을 어떻게 할지(지운다, 본문으로 바꾼다, 읽기만 하게 둔다, 그런 행이 있으면 서버가 시작하지 않는다)는 저장된 데이터를 지우거나 바꾸는 일이라 이 묶음에서 정하지 않았다. 권고: 데이터 단계(`data_steps`) 하나로 카드마다 콘텐츠 블록을 `contentOrder` 순서대로 본문 문서로 바꾸고(웹앱 변환기 `apps/web/tools/templates/convert.ts`의 규칙과 같게) 바꾼 블록은 지운다. 바꾸지 못하는 것은 남기고 로그한다. S7부터 만든 DB에는 그런 행이 없다.

**허용 목록의 판.** 판 2가 들어갔다(설계 5.7의 표). 판 1에 수식 노드 둘을 더한 것이다(웹앱 쪽의 제안을 사용자가 받아들였다. 2026-11-11 03:13 KST의 작업 지시에 적힌 결정이다).

- 새 노드는 `inlineMath`(인라인)와 `blockMath`(블록)다. 둘 다 속성은 `latex` 하나이고 1–4,000자의 문자열이다.
- 다이어그램은 언어가 `mermaid`인 `codeBlock`이다. 허용 목록도 서버도 바뀌지 않는다.
- 자세한 것(새 노드가 올 수 있는 자리, 테스트 벡터에 더할 경우)은 웹앱의 문서 `apps/web/docs/contract-assumptions.md` 4절에 있다.

**공유 테스트 벡터.** 클라이언트의 검증기와 서버의 검증은 `contracts/body-vectors.json`(판 2, 166개 경우)으로 맞춘다. 경우마다 문서와 기대(`accept`, `empty`, `reject`와 그 까닭)를 적고, 받는 경우에는 서버가 뽑을 평문(`text`)을 적기도 한다. 한도의 경계(깊이 32와 33, 노드 20,000과 20,001, 1 MiB와 1바이트 더)는 문서 대신 만드는 법(`build`)을 적었다. 설계 5.7이 정한 경계, 웹앱 문서 4.4의 1–13번(UTF-16으로 세는 6번과 10번 포함)이 들어 있다. 서버의 테스트 `crates/model/tests/body_vectors.rs`가 모두 읽는다.

**웹앱에 알릴 것(벡터와 웹앱 검증기의 차이).** 서버 쪽에서 보기에 웹앱의 검증기가 벡터와 다르게 답할 경우다. 웹앱의 벡터 테스트가 잡을 것이다. (1) U+0000을 담은 문자열을 서버는 거절한다. (2) `mention.userId`를 서버는 사용자 ID 형식(UUID 소문자 표기)으로만 받는다(설계 5.7). 웹앱은 1–100자의 문자열이면 받는다. (3) `content: null`과 `text` 노드의 `marks: null`을 서버는 거절한다(설계 5.7의 "`null`은 거절"). 웹앱은 빈 것으로 본다. (4) `build`로 적은 경우는 웹앱의 테스트가 같은 법으로 문서를 만들어야 한다.

**웹앱이 지금 해도 되는 것.** 카드의 속성, 제목, 아이콘, 뷰, 카드 목록의 쪽 나누기는 그대로다. 콘텐츠 블록을 읽고 쓰는 코드와 `contentOrder`를 다루는 코드는 만들지 않는다(tracer-bullet 6부터 서버에 없다).

## 2. 날짜 속성의 값 (설계 5.7의 B)

**지금.** 카드의 `properties`(계약에서는 `object`)에서 날짜 속성의 값은 JSON을 담은 문자열이다: `"{\"from\":1772712000000}"`. 날짜만 있는 값도 "그날 정오의 시각"(밀리초)으로 적는다. `includeTime`, `timeZone` 멤버가 붙을 수 있다.

**바뀐 뒤.** 값이 객체다. 달력 날짜는 문자열이고(`{"from": "2026-03-05"}`), 시각은 UTC 밀리초 정수다(`{"from": 1772703000000}`). 범위는 `to`를 더한다. 세 모양의 표와 서버의 검증 규칙은 설계 5.7의 "날짜 속성 값의 와이어 모양"에 있다.

웹앱이 알아 둘 것:

- `includeTime`과 `timeZone`은 없어진다. 시간대는 보는 사람의 설정(`getUserSettings`의 `timeZone`)에서 온다.
- 날짜 필터의 값도 밀리초 문자열이 아니라 달력 날짜 문자열이다.

**들어간 것(tracer-bullet 4, 2026-11-11).** 서버가 설계 5.7의 규칙대로 받는다. 카드를 쓰는 모든 경로(`createCard`, `patchCard`, `createBlocks`, 블록 패치 셋, boards-and-blocks 둘, 가져오기)에서 같다.

- 날짜 값은 객체이고 `from`과 `to` 밖의 멤버(`includeTime`, `timeZone`)가 있으면 거절한다. 달력에 없는 날, 연도 1–9999 밖, 0보다 작거나 9999년을 넘는 밀리초, 종류가 섞인 `from`과 `to`, `from`보다 앞선 `to`, `from`과 같은 `to`도 거절한다. 모두 400이다(`field`는 없고 메시지가 속성의 ID와 까닭을 말한다).
- 보드의 속성 정의에서 `type`이 `date`인 속성은 날짜 값이나 `null`만 받는다. 다른 속성(정의가 없는 ID 포함)은 날짜 값을 받지 않는다. 그래서 날짜 속성에 예전 모양(JSON을 담은 문자열)을 보내면 400이다.
- 속성 정의의 `type`을 바꾸거나 정의를 지워도 카드의 값은 그대로 둔다. 검사는 쓰기가 바꾸는 값에만 하므로, 맞지 않게 된 값이 있는 카드도 제목·본문 같은 다른 것은 고칠 수 있고, 그 값을 다시 쓸 때 새 규칙으로 검사한다. 카드와 보드를 복제하거나 아카이브를 가져올 때는 맞지 않는 종류의 값(날짜가 아닌 속성의 날짜 값, 날짜 속성의 문자열이나 목록)을 `null`로 바꿔 넣는다.
- 밀리초는 JSON의 정수로 적는다. `1772703000000.0`처럼 소수점을 단 수는 받지 않는다(JavaScript는 그렇게 적지 않는다).
- 이미 저장된 값은 마이그레이션 4가 서버를 처음 띄울 때 한 번 바꾼다(휴지통의 것까지). 시각이 없던 값(`includeTime`이 없거나 거짓)은 그 밀리초의 UTC 날짜가 된다. 예전 웹앱이 그날 정오로 적었으므로 UTC-12부터 UTC+11까지는 적은 날 그대로다. 시각이 있던 값은 밀리초 정수로 남는다. 읽을 수 없는 값은 `null`이 된다. 뷰의 필터 가운데 날짜 속성의 것은 밀리초 문자열을 UTC 날짜로 바꾼다. 예전 판의 아카이브를 가져올 때도 같은 규칙으로 바꾼다.
- 서버는 뷰의 필터 값을 검사하지 않는다(필터는 클라이언트가 쓴다). 계약의 `properties`는 아직 `object`이고 설명에 날짜 값을 적었다. 값의 타입을 계약에 적는 일(3절)은 남았다.

## 3. 속성 정의와 속성 값의 타입

**지금.** 다음 셋은 계약에서 멤버를 정하지 않은 `object`(`additionalProperties: true`)다: 보드의 `cardProperties`의 항목, 카드의 `properties`, 블록의 `fields`. 생성한 타입에서는 `{[key: string]: unknown}`이 된다.

서버는 이미 검증한다. 속성 값은 문자열, 문자열의 목록, `null` 가운데 하나여야 하고, 속성 정의의 모양도 본다(`crates/server/PORTING.md`의 결정 대기 항목 S14). 다만 그 모양을 계약에 타입으로 적지 않았다.

**바뀐 뒤.** 2절의 날짜 값과 함께 속성 정의(`id`, `name`, `type`, `options`)와 속성 값의 타입을 계약에 적는다. 그때까지 웹앱은 이 둘의 타입을 자기 쪽에 둔다.

## 4. 파일 올리기 (설계 5.6, 4.3)

**지금.** `uploadFile`(`POST /boards/{boardId}/files`)이 `multipart/form-data` 요청 하나로 파일 전체를 받는다. 한도는 설정 `maxfilesize`이고 기본값은 한도 없음이다.

**바뀐 뒤.** 올리기 세션의 조작 다섯이 생긴다: 세션 만들기, 부분 하나 올리기, 받은 부분 묻기, 완료, 버리기. 경로와 요청·답의 모양은 설계 5.6의 "라우트" 표에 있다. 조작 이름(`operationId`)은 계약에 넣을 때 정한다.

이미 있는 조작은 이렇게 된다.

- `uploadFile`은 남는다. 다만 부분 하나의 크기(8 MiB, 설정 `upload_part_bytes`)까지만 받는다. 넘으면 413이고 `reason`이 청크로 올리라고 알린다. 붙여 넣은 이미지처럼 작은 파일을 위한 길이다.
- 파일 하나의 한도는 100 MiB가 된다.
- 열린 세션의 수와 대기 용량을 넘으면 429다.
- `getFile`과 `getFileInfo`는 그대로다.

권한, 크기 검사, 체크섬, 만료의 규칙은 설계 5.6의 "세부 결정" 표(1–11번)에 있다.

**들어간 것(tracer-bullet 2c).** 올린 파일을 어느 블록도 가리키지 않으면(이미지·첨부 블록의 `fileId`, 카드 본문의 이미지. 휴지통의 블록까지 센다) 그렇게 된 지 `trash_retention_days`(기본 30일)가 지난 뒤 서버가 지운다. 그 뒤 `getFile`은 404다. 올리고 아직 어디에도 넣지 않은 파일도 올린 때부터 센다. 조작과 모양은 바뀌지 않았다.

## 5. 아카이브와 템플릿 (설계 3.10, 5.7)

**지금.**

- `exportBoardArchive`와 `exportTeamArchive`가 판 2의 형식(ZIP, 안에 줄마다 JSON)을 낸다(구현 전의 기록. 지금은 판 3, 아래).
- `importArchive`가 판 1(JSONL)과 판 2를 받는다(구현 전의 기록. 지금은 판 3만, 아래).
- 계약은 이 셋의 본문을 `application/octet-stream`과 `multipart/form-data`로만 적는다.
- 내장 템플릿은 저장소 밖에서 받아 온 판 2 아카이브에서 온다.

**바뀐 뒤.**

- 아카이브 형식이 판 3이 된다. 계약의 타입(보드, 블록, 본문)을 그대로 쓴다. 옛 형식(판 1, 판 2)의 가져오기는 없어진다.
- `importArchive`는 4절의 올리기 세션을 쓴다(용도 `import`, 한도 256 MiB). 요청의 모양이 바뀐다.
- 내장 템플릿은 웹앱의 변환기(`apps/web/tools/templates/convert.ts`)가 낸 산출물을 서버가 받아 넣는다. `getTemplates`의 모양은 그대로이고 템플릿의 내용이 바뀐다. 본문이 문서가 되고, 옛 제품 이름과 바깥 링크가 빠진다.
- 온보딩(`onboard`)이 복사하는 보드의 내용도 같이 바뀐다.

**들어간 것(tracer-bullet 5).** 내장 템플릿 13개는 변환기의 산출물이다. 서버 크레이트에 복사해 두고 바이너리에 넣으며, 저장소 밖의 아카이브는 더 쓰지 않는다. 조작과 모양은 바뀌지 않았고, `getTemplates`와 그 보드의 블록이 담는 것이 바뀌었다.

- 템플릿에 이미지가 없다(설계 결정 18번). 산출물에서 이미지 노드 15개가 빠졌다.
- 카드의 내용은 본문(`body`)에 있다. 내장 템플릿에는 콘텐츠 블록(`text`, `checkbox`, `image` 등)이 없다. welcome 보드의 블록은 46개에서 14개로 줄었다.
- 날짜 속성의 값은 2절의 새 모양이다.
- `templateVersion`이 7이 된다. 6 이하의 내장 템플릿을 가진 DB는 다음 시작 때 템플릿을 새로 넣는다.
- 보드의 `properties`에 `templateKey`가 생긴다. 산출물이 정한 템플릿마다의 이름(`welcome`, `roadmap` 등)이고, 제목이 바뀌어도 그대로다. 템플릿에서 만든 보드에도 복사되고 아무 클라이언트나 적을 수 있으므로, 내장 템플릿을 가려낼 때는 `isBuiltin`이 `true`인 보드에서만 이것을 본다.
- `trackingTemplateId`는 지금처럼 제목에서 만든 해시다. 제목 끝의 공백이 빠진 7개(`Meeting Agenda` 등)와 이름이 바뀐 welcome 보드(`Welcome to fvoci!`)는 값이 바뀌었다.
- 온보딩은 내장 템플릿 가운데 `properties.onboarding`이 `true`인 보드(welcome)를 복사한다. 그런 보드가 없으면(템플릿을 지웠거나 시작할 때 넣지 못한 경우) 템플릿을 다시 넣고 복사한다. 그래서 `onboard`가 500으로 답하던 일이 없어졌다(8절에서 뺐다). 템플릿을 넣을지 정하고 지우고 넣는 일은 한 트랜잭션이라, 여러 서버나 요청이 함께 해도 한 번만 넣는다.
- 템플릿을 새로 넣을 때 옛 내장 템플릿의 파일을 저장소에서도 지운다. 판 6의 템플릿을 가진 DB는 첫 시작에서 옛 이미지 14개(약 6.2 MB)를 지운다.

**들어간 것(tracer-bullet 5b).** 아카이브는 판 3이다. 조작의 모양은 그대로이고, 계약의 설명에 형식을 적었다.

- 판 3은 판 2와 같은 ZIP이다: `version.json`(`{"version": 3, "date": <ms>}`), 보드마다 디렉터리 하나에 `board.jsonl`과 파일. `board.jsonl`의 줄은 `{"type": "board"|"block"|"boardMember", "data": …}`이고 `data`는 계약의 `Board`, `Block`(본문 포함), `BoardMember` 모양이다.
- 블록이 보이는 파일이 모두 실린다. 카드와 댓글 본문의 이미지도 실린다. 파일이 없는 이미지·첨부 블록도 내보낼 수 있다(전에는 500. 8절에서 뺐다). 이미지 블록의 `fileId`는 클라이언트가 적은 대로라, 저장 이름이 될 수 없는 이름(`/`가 든 것, `board.jsonl`, `version.json`)에는 항목을 쓰지 않는다. 그래서 이 서버가 쓴 아카이브는 늘 다시 읽힌다.
- `importArchive`는 판 3만 받는다. 판 1(JSONL), 판 2, `version.json`이 없는 ZIP은 400이고 아무것도 만들지 않는다. 읽을 수 없는 항목(체크섬이 틀리거나 한도를 넘는 것)이 있는 아카이브도 무엇을 만들기 전에 400이다.
- 가져오기는 블록이 보이는 파일을 새 보드의 파일로 바꾼다. 이미지·첨부 블록의 `fileId`와 `attachmentId`, 본문의 이미지가 모두 바뀐다(8절의 결함에서 뺐다). 아카이브에 파일이 없는 이름은 블록의 필드에서 빈 이름이 되고 본문에서는 그대로 남는다.
- 가져오기는 속성의 종류에 맞지 않는 값을 `null`로 바꾼다(2절, 복제와 같다). 예전 모양의 날짜 값을 새 모양으로 바꾸는 일은 하지 않는다. 판 3의 아카이브에는 예전 모양이 없다.
- 4절의 올리기 세션과 스트리밍은 아직이다(9단계).

## 6. 계약 밖에 있는 것

- **WebSocket(`/ws`)의 메시지.** OpenAPI는 WebSocket을 적지 못한다. 메시지는 계약 파일 밖의 `contracts/websocket.md`(사람이 읽는 것)와 `contracts/websocket.json`(멤버와 타입, 서버의 테스트가 실제 메시지를 견준다)에 적었다(2026-11-11 04:14 KST).
  - 핸드셰이크의 인증, 명령 넷, 메시지 아홉, 어느 조작이 어느 메시지를 내는지, 서버가 연결을 닫는 때가 그 문서에 있다. 웹앱의 질문 G1–G6의 답도 그 문서의 5절에 있다.
  - 메시지가 싣는 보드, 블록, 카테고리, 멤버는 계약의 `Board`, `Block`, `Category`, `BoardMember`와 같은 모양이다.
  - 1절이 끝나면 `UPDATE_BLOCK`이 싣는 블록의 모양이 함께 바뀐다.
- **오류의 상태 코드.** 조작마다 4xx를 하나씩 적지 않았다.
  - 모든 조작의 `default` 응답이 `ErrorBody`다. 어느 상태가 무슨 뜻인지는 계약의 머리말과 `default`의 설명에 있다.
  - 조작에 고유한 거절(`reason`)은 그 조작의 설명에 있다: `invalid_credentials`, `signup_token_required`, `signup_token_invalid`, `username_taken`, `email_taken`, `admin_required`, `last_admin`, `sharing_disabled`.
  - 429와 `Retry-After`는 `login`, `register`, `changePassword`에 따로 적었다.
  - `getFile`의 412와 416만 `ErrorBody`가 아니다. 412는 본문이 없고 416은 `text/plain`이다.
- **관리자 소켓의 라우트**(`/api/v1/admin/...`). 로컬 유닉스 소켓에서만 받는다. 계약에 없다.

## 7. 그대로 둔 불규칙

바꿀 결정이 난 것은 아니지만 계약에 그대로 적힌 모양이다. 고치게 되면 계약을 고치는 변경으로 한다. 칸에 다 적지 못한 것은 표 아래에 같은 번호로 풀어 적었다.

| 무엇 | 지금 |
|---|---|
| ID 목록으로 묻는 조회가 `POST`다 | `getUsers`(`POST /users`), `getTeamUsersById`(`POST /teams/{teamId}/users`). 없는 ID나 두 번 적은 ID가 있으면 요청 전체가 거절된다 |
| 검색어 없는 `getTeamUsers` | 모든 사용자가 아니라 가장 오래된 열 명이다(검색과 같은 한도) |
| `searchTeamBoards` | 경로의 팀으로 좁히지 않는다. 사용자가 찾을 수 있는 모든 팀의 보드를 찾는다 |
| 동작을 경로 끝에 붙인 라우트 | `duplicate`, `undelete`, `join`, `leave`, `hide`, `unhide`, `onboard` |
| 구독의 경로 | `DELETE /subscriptions/{blockId}/{subscriberId}`, `GET /subscriptions/{subscriberId}`. 아래 (1) |
| 구독자의 종류 `channel` | 스키마 `SubscriberType`에 있으나 서버는 `user`만 받는다 |
| 오류 본문의 `retry_after`, `request_id` | snake_case다(설계 1.2가 정한 모양) |
| 오류 본문의 선택 멤버 | 스키마에는 `null`도 되는 것으로 적혀 있으나 서버는 `null`을 보내지 않고 멤버를 뺀다 |
| `deleteAt` | 있는 것에서는 늘 0이다(지운 것은 휴지통에 있다). WebSocket의 삭제 알림이 쓴다 |
| 팀의 `title`, `settings` | 값을 정하는 라우트가 없다 |
| `ping`의 멤버 이름 | `db_type`, `build_number` 등 snake_case다. 주소는 아래 (2) |
| 요청 스키마와 서버의 해석 | 서버는 계약의 요청 스키마보다 너그럽게 읽는다. 아래 (3) |
| `NewBlock`의 `createAt`, `updateAt` | 필수이고 1 이상이어야 하지만 서버는 그 값을 쓰지 않는다 |
| `NewCategory`, `CategoryUpdate`의 `userId`, `teamId`(와 고칠 때의 `id`) | 본문에도 있어야 하고 세션·경로의 값과 같아야 한다. 아래 (4) |
| `createCategory`에 `createAt`, `updateAt`을 보내면 | 계약의 `NewCategory`에는 없지만 0이 아닌 값이 오면 서버가 그 값을 쓴다 |
| `NewBoard`의 `templateVersion` | 클라이언트가 정할 수 있다 |
| `disableNotify` | 받지만 API로 보이는 효과가 없다(알림을 보내는 쪽이 아직 없다) |
| 빈 목록을 넣는 `createBlocks` | 201과 `[]` |
| 이미 있는 구독을 다시 만드는 `createSubscription` | 201 |
| 이미 멤버인 사용자를 넣는 `addMember` | 200과 지금의 멤버십. 역할은 바뀌지 않는다 |
| `patchCard`의 `updatedProperties` | 이름과 달리 합치지 않고 전체를 바꾼다. 아래 (5) |
| `getFile`의 `Content-Type` | 모든 파일이 `application/octet-stream`으로 나간다. 아래 (6) |
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
| `importArchive` | 전부 아니면 전무가 아니고, 요청 크기의 한도가 없다. 아래 (7) |
| `onboard` | 부를 때마다 보드를 하나 더 복사한다. 답이 200이다. 아래 (8) |
| `createSubscription`의 `blockType` | 블록의 실제 종류와 견주지 않고 보낸 대로 저장한다 |
| 세션 없는 큰 요청 | JSON 본문이 64 KiB를 넘으면 401보다 먼저 413이다 |

1. **구독의 경로.** 라우터에서는 첫 자리의 이름이 하나(`blockOrSubscriberId`)다. 계약에서만 `{blockId}`와 `{subscriberId}`로 갈라 적는다.
2. **`hello`와 `ping`의 주소.** 둘도 `/api/v1` 아래에 있다(`/api/v1/hello`, `/api/v1/ping`). 예전 주소 `/hello`, `/ping`은 다른 모든 주소처럼 웹앱의 화면 주소로 다뤄진다.
3. **요청 스키마와 서버의 해석.**
   - 만들기와 고치기의 요청은 계약에 따로 적었다: `NewBoard`, `NewBlock`, `NewCard`, `NewCategory`, `CategoryUpdate`, `NewMember`, `MemberRoles`, `SharingRequest`, `NewSubscription`, `AdminChange`, `SettingsChange`.
   - 서버는 이보다 너그럽게 읽는다. 모르는 멤버를 대부분 버린다.
   - 예외로 400을 답하는 곳이 있다. `patchUserSettings`는 모르는 멤버를 거절한다. `createBoard`와 `createCategory`는 `id`를, `setSharing`은 `token`을 거절한다.
   - 스키마가 필수로 적은 멤버가 빠져도 거절하지 않고 빈 값으로 읽는 곳이 있다. `login`의 `password`가 없으면 401이고, `patchBlocks`의 목록이 없으면 204다.
   - 그러니 클라이언트는 계약에 없는 멤버를 보내지 않는다.
4. **카테고리 요청의 `userId`, `teamId`.** 세션과 경로가 이미 말하는 값이다. 세션과 다른 `userId`는 만들 때 403, 고칠 때 400이다.
5. **`patchCard`의 `updatedProperties`.** 비어 있지 않으면 카드의 속성 값 전체를 그것으로 바꾼다. 비어 있으면 아무것도 바꾸지 않는다. 그래서 마지막 값을 이 길로는 지울 수 없다. 합치도록 바꿀 후보다.
6. **`getFile`의 머리글.** 모든 파일이 `application/octet-stream`과 `Content-Disposition: attachment`로 나간다. 파일 이름은 올린 이름이 아니라 `fileId`다. `Last-Modified`가 응답 시각이라 재검증으로 내려받기를 아끼지 못한다.
7. **`importArchive`.** 모든 보드를 읽고 검사한 뒤(줄의 모양, 블록의 종류, 보드와 블록이 맞는지) 하나씩 만든다. 그 검사에 걸리면 아무것도 만들지 않지만, 저장하는 중에 실패하면 앞의 보드는 남는다. 요청 크기의 한도가 아직 없다(4절에서 생긴다).
8. **`onboard`의 200.** 만든 것을 201로 답하는 규칙의 예외다(`PORTING.md` N절).

## 8. 알려진 결함

계약이 약속하지 않는 동작이다. 클라이언트는 여기에 기대지 않는다. 다음 작업 묶음에서 고칠 후보이고, 고치면 이 절에서 뺀다.

- **카드가 아닌 모양의 카드 블록.** `createBlocks`와 `patchBlock`은 `fields`의 `properties`만 검사한다. 종류가 `card`인 블록에서 `icon`이 문자열이 아니거나, `isTemplate`이 불리언이 아니면 `getCards`와 `getCard`가 500이다. 편집자 한 명이 그 보드의 카드 목록을 막을 수 있다.
- **SQLite에서 `field=property_name`.** 검색어가 JSON 경로의 일부가 된다. `a.b`는 안쪽 멤버를 찾고, 경로로 읽을 수 없는 검색어는 DB 오류다. 값이 `null`인 멤버는 PostgreSQL에서만 찾는다.
- **DB에 따라 다른 대소문자.** 제목 검색은 SQLite에서 ASCII 글자만 대소문자를 접는다. `getTeamUsers`의 `search`는 SQLite에서 ASCII 대소문자를 가리지 않고 PostgreSQL에서 가린다.
- **기본 카테고리를 처음 만드는 `getCategories`.** 카테고리를 이미 가진 사용자의 기본 카테고리가 만들어지는 그 한 번의 답만 틀린다. 새 카테고리가 `sortOrder`와 상관없이 맨 끝에 오고, 다른 카테고리는 10씩 밀리기 전의 `sortOrder`를 싣는다. 다음 요청부터는 맞다.
- **끝난 세션의 WebSocket.** 세션이 한가함 한도나 절대 한도로 끝나도, 그 세션으로 연 연결은 끝난 세션을 지우는 정리(한 시간마다)가 돌 때까지 남아 메시지를 받는다. 로그아웃, 비밀번호 재설정과 변경은 곧바로 닫는다.
- **`deleteBoardsAndBlocks`로 지운 공유 보드.** `deleteBoard`와 달리 그 보드의 읽기 토큰으로 블록을 따라가던 WebSocket 연결을 닫지 않고, 블록마다의 삭제 메시지도 그 연결에 가지 않는다. 따라가던 익명 독자는 보드가 지워진 것을 모른다(`contracts/websocket.md` 3절).
- **복제에서 복사하지 못한 본문의 이미지.** 파일을 복사하지 못하면(저장소의 오류, 기록에 없는 파일) 복사본의 본문이 원래 파일의 이름을 그대로 가리킨다. 그 파일은 다른 보드의 것일 수 있어, 그 카드의 본문을 다시 저장하면 이미지를 빼기 전까지 400이다. 이미지 블록은 같은 경우에 파일 없는 블록이 된다. 아카이브 가져오기에서 아카이브에 파일이 없는 본문의 이미지도 같다(이 서버가 쓴 아카이브에서는 생기지 않는다. 없는 파일도 빈 항목으로 실린다).
- **제목의 U+0000.** PostgreSQL에서 블록과 보드의 제목(`TEXT`)에 U+0000이 있으면 DB가 거절해 500이다. SQLite는 저장한다. 본문은 검증이 거절한다(1절).
- **`setSharing`의 `token: null`.** 거절하지 않는다(`null`인 멤버를 먼저 버린다).

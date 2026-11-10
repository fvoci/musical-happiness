# 내장 템플릿의 변환 산출물

원본의 내장 템플릿 아카이브(`server/assets/templates.boardarchive`)를 새 서버가 받을 모양으로 바꾼 결과다. 서버 설계(저장소 루트의 `docs/s6-design.md` 5.7절의 "3.10의 갱신")가 정한 대로, 서버가 이 폴더를 저장소 루트의 `crates/server/assets/templates/`에 받아 내장 템플릿으로 쓴다. 이 README를 뺀 파일은 도구가 쓴 것이고 손으로 고치지 않는다.

```bash
cd apps/web
node tools/templates/convert.ts --write tools/templates/output   # 다시 만든다
node tools/templates/convert.ts --check tools/templates/output   # 최신인지만 본다
node tools/templates/convert.ts --archive <파일> --write tools/templates/output   # 아카이브가 다른 곳에 있을 때
```

같은 입력이면 같은 바이트가 나온다. `tests/tools/templates.test.ts`가 이 폴더가 최신인지를 본다(`pnpm --filter web run test`).

**아카이브가 없어도 이 폴더는 검사된다.** 아카이브는 원본 저장소의 `server/assets/templates.boardarchive`에 있었다. 도구는 그 자리(이 저장소 루트 기준), 환경 변수 `FVOCI_TEMPLATE_ARCHIVE`, 인자 `--archive <파일>`의 순서로 찾는다(`tools/original.ts`). 없으면:

- `--write`와 건수 출력은 멈춘다(다시 만들 재료가 없다).
- `--check`는 "원본이 없어 건너뜀"을 한 줄 찍고 이 폴더끼리의 앞뒤만 본다: 글에 대한 약속(아래), 본문이 검증을 통과하는지, `index.json`의 건수가 목록과 맞는지, `index.json`·`CHANGES.md`가 템플릿 파일과 `changes.json`에서 도구가 쓰는 그대로인지.
- 테스트는 아카이브를 다시 변환하는 5건을 건너뛰고(한 줄을 찍는다), 이 폴더만으로 도는 것(템플릿·카드·본문·뷰의 수, 참조의 온전함, 원본 이름과 바깥 주소가 남지 않음, 문서가 검증을 통과함, 파일끼리 맞는지)은 늘 돈다.
- `index.json`의 `source.path`는 어디서 읽었든 원본 저장소에서의 자리를 적는다(산출물의 바이트가 읽은 자리에 달리지 않게).

## 파일

| 파일           | 내용                                                                                                                 |
| -------------- | -------------------------------------------------------------------------------------------------------------------- |
| `index.json`   | 형식 판(`format: 3`), 원본 아카이브의 SHA-256, 템플릿 목록(키, 파일, 제목, 뷰·카드·본문·파일 수), 변환과 변경의 건수 |
| `NN-<키>.json` | 템플릿 하나: `key`, `onboarding`, `board`, `views`, `cards`, `files`                                                 |
| `changes.json` | 글에서 바뀐 곳의 목록(`changes`)과, 바꾸지 않았지만 검수할 때 볼 것(`reviewNotes`)                                   |
| `CHANGES.md`   | 같은 목록을 사람이 읽는 표로                                                                                         |

## 템플릿 파일의 모양 (임시)

**타입은 임시다.** 서버의 계약이 없을 때 서버 설계 문서와 아카이브의 내용을 보고 `tools/templates/output.ts`에 손으로 적었다. 계약은 그 뒤에 나왔다(저장소 루트의 `contracts/openapi.json`). 거기서 생성한 타입(보드, 블록, 본문)에 맞추고 다시 만드는 일이 남아 있다.

```jsonc
{
  "key": "meeting-agenda",        // 제목이 바뀌어도 변하지 않는 이름
  "onboarding": false,            // 새 사용자가 받는 보드 하나만 true
  "board": { "id", "title", "description", "icon", "showDescription", "cardProperties": [{ "id", "name", "type", "options": [{ "id", "value", "color" }] }] },
  "views": [{ "id", "type": "view", "title", "fields": { /* viewType, groupById, cardOrder, filter, sortOptions, … 원본의 뷰 멤버 그대로 */ } }],
  "cards": [{ "id", "type": "card", "title", "fields": { "icon", "isTemplate", "properties": { "<속성 id>": "…" } }, "body": { "type": "doc", "content": [] } }],
  "files": [{ "fileId", "archiveMember", "bytes" }]
}
```

- **본문**(`body`)은 카드마다 문서 하나다(`body_version` 1의 허용 목록을 통과한다). 본문이 없는 카드는 `null`이다. 콘텐츠 블록과 `contentOrder`는 없다.
- **날짜 속성의 값**은 새 와이어 모양이다: `{"from": "2022-11-22"}`. 아카이브의 값은 모두 시각 없는 날짜였다(19건).
- **식별자**는 아카이브의 것 그대로다. 한 템플릿 안에서 뷰의 `cardOrder`, 카드의 선택지 값, 뷰의 `groupById` 같은 참조를 잇는 데만 쓴다. 서버가 넣을 때 새 식별자로 바꿔야 한다(형식도 UUID가 아니다).
- **`onboarding`**: 원본은 온보딩 보드를 제목(`Welcome to …!`)으로 찾았다. 그 제목은 이번에 바뀐 글의 하나라서 표식을 따로 두었다. 서버는 제목이 아니라 이 표식(또는 `key: "welcome"`)으로 찾아야 한다.
- **보드에 넣지 않은 것**: `isTemplate`·`templateVersion`·보드 종류(공개 여부)·`trackingTemplateId`·만든 사람과 시각. 서버가 넣을 때 정한다(원본은 내장 템플릿을 `isTemplate: true`, 공개 보드로 넣었다). 판 번호가 필요하면 `index.json`의 `source.sha256`이나 이 폴더의 내용으로 정할 수 있다.
- **카드의 `isTemplate`**은 보드 안의 카드 템플릿이다(5장). 뷰의 `defaultTemplateId`가 가리킨다.
- **그림**(`files`): 본문의 `image` 노드가 `fileId`로 가리키는 파일 14개(노드는 15개)다. 바이트는 여기에 복사하지 않았고 `archiveMember`가 원본 아카이브 안의 위치를 가리킨다. 서버 설계 3.10의 "이미지를 뺄지"가 아직 미결이라 노드를 남겼다. 빼기로 하면 본문에서 `image` 노드를 걷어 내고 다시 만든다(도구에 그 옵션은 아직 없다).

## 변환이 한 일

기계적인 것(글의 뜻과 무관, `index.json`의 `conversions`):

| 항목                                                    | 수                                |
| ------------------------------------------------------- | --------------------------------- |
| 본문이 된 콘텐츠 블록                                   | 219 (카드 58장의 문서 58개)       |
| 새 모양으로 바꾼 날짜 값                                | 19                                |
| 제목에서 `(NEW)` 표식이나 바깥 공백을 뗀 것             | 7 (원본도 넣을 때 `(NEW)`를 뗀다) |
| 뷰의 `cardOrder`에서 뺀, 없는 카드의 식별자             | 18                                |
| 건너뛴 끊어진 참조(`contentOrder`에 있는데 블록이 없다) | 42                                |
| 버린 주인 없는 콘텐츠 블록(부모 카드가 없다)            | 38                                |

글을 바꾼 것(결정 14, `index.json`의 `changes`, 목록은 `CHANGES.md`): 모두 47곳.

| 규칙                                                                  | 수  |
| --------------------------------------------------------------------- | --- |
| 원본의 두 이름 → `fvoci`                                              | 9   |
| 제품을 가리키는 낱말 "Boards" → `fvoci`                               | 5   |
| 본문의 바깥 링크와 그것을 설명하는 문장을 뺌                          | 1   |
| 원본 회사로 가는 링크 속성(`url`)의 값을 뺌                           | 22  |
| 그 밖의 바깥 주소인 링크 속성(`url`)의 값을 `example.com` 아래로 옮김 | 10  |

- 본문의 바깥 링크는 1개였고, 바깥 주소의 대부분은 본문이 아니라 **링크 속성의 값**에 있었다(32건).
- **링크 속성의 값 32건은 도메인으로 갈랐다**(2026-11-10 22:40 KST에 바꿈. 그 전에는 32건을 모두 비웠다).
  - **뺀 것 22건**: 원본 회사로 가는 주소다. 호스트의 한 조각이 원본의 두 이름 가운데 하나인 것(원본 회사의 사이트와 문서: 제품 소개 페이지 10, 보도 자료 2, 문서 9)과, 경로에 그 이름이 든 것(다른 사이트에 있는 원본 회사 계정의 게시물 1). 값만 비웠고 속성 자체는 남겼다.
  - **옮긴 것 10건**: 그 밖의 바깥 주소. 지어낸 회사 다섯의 사이트(`liminarycorp.com` → `https://liminarycorp.example.com`), 다른 사이트의 그림 4·글 1.
  - 옮기는 규칙(`rewrite.ts`의 `rewriteLinkValue`): 호스트에서 앞의 `www.`와 마지막 조각을 떼고 `.example.com`을 붙인다. 스킴은 `https`. 경로·질의·조각은 그대로 둔다. `example.com`은 예시용으로 예약된 도메인(RFC 2606)이라 누구의 사이트로도 가지 않는다.
  - 게시물 1건은 2026-11-10 23:35 KST에 "옮김"에서 "뺌"으로 바꿨다. 호스트는 다른 사이트이지만 원본 회사 계정의 글이라 원본 회사로 가는 주소다(`rewriteLinkValue`: 경로에 원본의 이름이 있으면 뺀다).
- "Boards"는 결정의 글자("원본 이름")보다 한 걸음 나간 판단이다. 번역에서 같은 낱말을 제품 이름일 때 `fvoci`로 바꾸기로 한 결정을 따랐다. 다섯 곳을 읽고 정했고, 도구는 그 수가 달라지면 멈춘다(`REVIEWED_PRODUCT_WORDS`).
- 결과에 원본의 이름이 하나라도 남으면 도구가 멈춘다. `http://`·`https://`·`www.`는 링크 속성의 값이 통째로 `https://…example.com` 주소인 경우(위의 10건)에만 남을 수 있고, 그 밖의 자리(제목, 설명, 본문, 다른 속성)에 하나라도 있으면 도구가 멈춘다(`output.ts`의 `promiseProblems`). 본문의 링크 처리(바깥 링크와 그 문장을 뺌)는 그대로다.

바꾸지 않았지만 검수할 것(`reviewNotes`): 예시 이메일 10건(`@email.com`은 실제로 있는 도메인이다), 그림 15건, 원본 회사의 다른 제품(채널, 다이렉트 메시지)을 말하는 문단 3건. 온보딩의 "Share cards on Channels" 카드는 카드 전체가 이 제품에 없는 기능을 설명한다.

# 서버 인수인계

고친 때: 2026-11-11 14:00 KST(서버의 다음 묶음이 맨 앞 "지금 상태"를 고쳐 간다). 2026-11-11 03:52 KST에 웹앱과 한 브랜치(`master`)로 합치면서 맨 앞의 절을 더하고 브랜치를 가리키는 말을 고쳤다. 앞선 판(S5 시점, 401줄)은 `git show 3c2a474:docs/handoff-s5.md`로 본다(이 저장소의 해시다).

## 저장소가 하나로 합쳐졌다

서버와 웹앱은 따로 갈라진 브랜치에서 만들어졌고, 2026-11-11에 `master`에서 합쳐졌다. 서버는 저장소 루트에(`crates/`, `xtask/`, `contracts/`, `docs/`), 웹앱은 `apps/web/`에 있다. 이 문서는 서버의 인수인계이고, 웹앱의 것은 `apps/web/docs/handoff.md`다.

- **브랜치.** 일마다 `master`에서 워크트리를 새로 갈라 일하고, 끝나면 `master`에 합친다. 합치기 전의 브랜치 `server`와 `web`의 커밋은 모두 `master`에 들어 있다.
- **푸시.** 일한 세션이 자기 브랜치를 이름으로 올린다(`git push origin <브랜치>`). 강제 푸시는 하지 않는다. 올라간 커밋은 고쳐 쓰지 않는다.
- **검증 명령.** 양쪽 모두 저장소 루트에서 돌린다.

  | 쪽 | 명령 | 명령의 설명과 기대 결과 |
  |---|---|---|
  | 서버 | `cargo xtask check`, `cargo xtask test` | 이 문서의 1절 |
  | 웹앱 | `pnpm install --frozen-lockfile` 뒤에 `pnpm --filter web run check`, `test`, `build`, `e2e` | `apps/web/PORTING.md`의 "검증 명령"과 "마지막 검증". `pnpm`을 놓는 법은 `apps/web/docs/handoff.md`의 "도구가 있는 곳" |

- **보관 폴더.** 커밋되지 않는 파일은 이 기기의 보관 폴더(`musical-happiness-carry/`) 아래에 있다. `server/`에는 템플릿 아카이브와 조사 자료가, `web/`에는 글꼴 산출물과 그 원본, 마지막 검증의 출력이 있다. 새 워크트리에 놓는 명령은 각 폴더의 `README.md` 3절에 있다. 서버는 템플릿 아카이브 없이 컴파일되고(템플릿은 저장소의 산출물이다), 웹앱은 글꼴을 놓으면 `build`가 글꼴을 받으러 네트워크에 나가지 않는다.
- **무시 규칙.** 저장소 루트의 `.gitignore`가 `target/`, `node_modules/`, 템플릿 아카이브를 무시한다. 웹앱의 빌드 산출물과 글꼴은 `apps/web/.gitignore`가 맡는다.
- **컨테이너.** 필요할 때 만들고 그 일이 끝나면 닫는다. `cargo xtask test`가 PostgreSQL과 S3 컨테이너를 스스로 준비하고, 돌린 세션이 일을 마칠 때 멈춘다(2절). 웹앱의 검증은 컨테이너를 쓰지 않는다.

## 지금 상태

- **서버.** 설계(`docs/s6-design.md`)의 "구현 순서" 1–9단계가 끝났다. 1–7단계는 오류 응답과 경로 규칙, 요청 크기 한도, 새 기준 스키마, 쿠키 인증, 로그인 시도 제한, 접두어 `/api/v1`, 계약 파일이고, 8단계(본문, 날짜 값, 템플릿)와 9단계(청크 업로드, 스트리밍, 한도)는 아래 목록의 tracer-bullet들로 들어갔다.
- **지금의 작업 묶음(브랜치 `fvoci/server-next`, 2026-11-11 04:08 KST에 시작).** 지시는 보관 폴더의 `PROMPT-server-next.md`다. 일을 tracer-bullet(기능 하나가 스키마부터 계약·테스트·스냅샷까지 끝에서 끝까지 도는 것) 하나씩으로 끊고, 하나가 끝날 때마다 검증하고 커밋해 올린 뒤 아래 목록에 표시한다. 올리기 전마다 새 하위 에이전트가 그 diff를 `beautiful-rust:rust-review`와 설계·계약에 견주어 검토하고, 받아들인 지적은 같은 tracer-bullet에서 고친다. 받아들이지 않은 지적은 `PORTING.md`에 까닭과 함께 적는다.

  | # | tracer-bullet | 상태 |
  |---|---|---|
  | 0 | WebSocket 메시지를 지금 모양대로 적는다(`contracts/websocket.md`, `websocket.json`, 견주는 테스트), 웹앱의 G1–G6에 답한다 | 끝 |
  | 1 | 카드의 본문 하나를 쓰고 읽는다: `body` 열, `createCard`·`patchCard`·`getCard`, 목록의 본문 여부와 할 일 수, 읽은 판의 409, 검증의 최소한, 벡터 몇 개. 설계의 "답을 기다리는 것" 셋을 결정으로 옮긴다 | 끝 |
  | 1b | 설정의 문서와 예시 파일(지시문 9절): `docker/fvoci/config.json`을 지금의 키로 고치고 읽히는지 보는 테스트, `docs/configuration.md`에 키 전부(S3 하위 키, `FVOCI_*`, 명령줄 인자, 없앤 키), 문서와 코드의 키가 어긋나면 실패하는 검사. 쓰이지 않는 키와 S3 키의 환경 변수는 보고에 | 끝 |
  | 2 | 허용 목록 전체: 노드·마크·속성·포함 관계, 판 2(수식 노드), 길이를 UTF-16으로, `href` 규칙, 검색용 평문(`body_text`), 공유 벡터 전체(웹앱 문서 4.4 포함) | 끝 |
  | 2b | 블록 길의 본문: `createBlocks`·`patchBlock`·`patchBlocks`·boards-and-blocks의 `body`와 `field: body`, 본문을 가질 수 있는 블록 종류, 이미지 `fileId`가 같은 보드의 파일인지, 복제가 본문의 이미지를 새 파일로 바꾸는 것 | 끝 |
  | 2c | 본문과 첨부가 가리키는 파일의 표(`file_refs`)와 유예를 둔 수거(설계 5.7의 C) | 끝 |
  | 3 | 댓글의 본문: 댓글 블록이 `body`를 받는다(카드와 같은 검증, `readAt`, 이미지 규칙). 설계 결정 11번을 "됨"으로 | 끝 |
  | 4 | 날짜 속성 값(5.7의 B): 날짜 속성은 날짜 값이나 `null`만, 다른 속성은 날짜 값을 받지 않는다. 마이그레이션 4의 데이터 단계가 저장된 값과 날짜 필터를 바꾼다(휴지통 포함), 예전 아카이브도. 템플릿의 날짜 값이 새 모양이라 템플릿보다 먼저 한다 | 끝 |
  | 5 | 템플릿 받기: 산출물을 저장소에, 이미지 빼기, `onboarding` 표식, `onboard`의 500, Dockerfile에서 아카이브를 뺌 | 끝 |
  | 5b | 아카이브 판 3(계약의 타입 그대로, 본문의 이미지 파일까지), 옛 형식(판 1, 판 2) 가져오기를 걷음. 설계 결정 14번 | 끝 |
  | 6 | 콘텐츠 블록 걷어 내기: 콘텐츠 블록 종류, `contentOrder`, `moveContentBlock`, 종류별 `fields` 한도. 저장된 콘텐츠 블록은 16에서 지운다 | 끝 |
  | 7 | 파일 하나를 청크로 올리고 받는다(올리기 세션, 로컬 저장소): `createUpload`, `putUploadPart`, `getUpload`, `completeUpload`, `deleteUpload`, 설정 `upload_part_bytes`와 `tmp_path` | 끝 |
  | 8 | 업로드의 한도(429, 설정 `max_pending_upload_bytes`), 만료와 정리(한 시간마다와 시작할 때), 시작할 때 남은 임시 파일 지우기, `uploadFile`의 스트리밍과 부분 하나 한도(413 `upload_in_parts`), `maxfilesize` 기본 100 MiB | 끝 |
  | 8b | S3 멀티파트: S3 설정에서 부분을 S3에 바로 올리고(`MultipartStore`), 버리거나 끝나면 `abort_multipart`. 운영 문서에 버킷의 `AbortIncompleteMultipartUpload` 규칙(설계 5.6의 7) | 끝 |
  | 9a | 내보내기를 임시 파일에 쓰고 길이와 함께 흘려 보낸다. 시작할 때 남은 것을 지운다 | 끝 |
  | 9b | 가져오기를 올리기 세션(용도 `import`)으로, 아카이브를 디스크에서 읽고 한도(256 MiB, 풀린 합계 1 GiB, 항목 수, `board.jsonl`, 파일 항목) | 끝 |
  | 10 | 응답 압축(정적 `.zst`, API의 gzip·brotli·zstd) | 끝 |
  | 11 | `cargo xtask test`가 띄운 컨테이너를 멈춘다, 이름에서 `hafgufa-`를 뗀다 | 끝 |
  | 12 | 남은 결함(`docs/contract-pending.md` 8절), 끝난 세션의 WebSocket | 끝 |
  | 13 | 웹앱의 질문에 답한다(`docs/contract-pending.md`의 새 절) | 끝 |
  | 14 | 빌드 크기(디버그 정보 수준) | 끝 |
  | 15 | 사용자 결정 1(11:05 KST): `patchCard`가 바뀐 속성만 받아 합치고 지울 속성을 따로 받는다 | 끝 |
  | 16 | 사용자 결정 2: 저장된 옛 콘텐츠 블록을 데이터 단계로 지우고(파일은 수거의 길로) 그것을 다루던 코드를 걷는다 | 끝 |
  | 17 | 사용자 결정 3: 공유 링크의 익명 독자에게 나가는 본문의 멘션에서 사용자 ID를 비운다(HTTP와 WebSocket) | 끝 |
  | 18 | 사용자 결정 4: `logging_cfg_*`, `audit_cfg_*`, `filess3config.trace`를 없앤 키로 | 끝 |
  | 19 | 사용자 결정 5: S3의 비밀 아닌 설정을 `FVOCI_*` 환경 변수로도 받는다 | 끝 |
  | 끝 | 묶음 전체의 리뷰, 마지막 `cargo xtask check`·`test` | 끝 |

  관찰: 테스트 `system::a_request_is_finished_when_its_client_disconnects`가 전체 실행에서 두 번(05:50, 13:20 KST) "보드 20개 가운데 19개만 카테고리에 들어감"으로 실패했다. 보드는 저장된 뒤에 카테고리에 들어가는데, 기다림이 "100 ms 동안 바뀌지 않으면 끝"이었다. 이제 만든 보드가 모두 들어갈 때까지(10초까지) 기다린다. 끝내 들어가지 않는 보드는 여전히 실패한다. 단독과 전체 실행 네 번에서는 재현되지 않았다.

  이 묶음에서 "사용자 결정 필요"로 남긴 것(까닭과 권고는 적힌 자리에):

  - 웹앱의 질문 가운데 계약의 모양이나 권한에 걸린 것(`docs/contract-pending.md` 9절의 "사용자 결정 필요" 줄).
  - 공유 링크의 익명 독자에게 사람 속성(`person`, `multiPerson`)의 값인 사용자 ID가 그대로 나간다. 결정 7과 멘션은 `createdBy`, `modifiedBy`, 멘션만 비운다. 로그인했지만 보드의 멤버가 아닌 사용자(다른 조직)는 읽기 토큰으로 읽어도 ID를 모두 받는다. 권고: 사람 속성도 비운다(보드의 `cardProperties`로 사람 속성을 가린다). 다른 조직의 사용자는 익명 독자와 같이 다룬다.

- **설정.** 키 전부는 `docs/configuration.md`에 있고, `config.rs`의 테스트가 문서와 코드(키, 기본값, 환경 변수, 없앤 키)를 견준다. 키를 더하면 같은 커밋에서 문서에 넣는다. 예시 파일 `docker/fvoci/config.json`은 테스트가 읽고 검사한다. `logging_cfg_*`, `audit_cfg_*`, `filess3config.trace`는 없앤 키다(사용자 결정 11:05 KST). `authmode`와 `prometheusaddress`는 예약된 키다(사용자 결정, 05:04 KST). S3의 키 가운데 자격이 아닌 것은 `FVOCI_FILESS3CONFIG_*` 환경 변수로도 받고, 자격은 `AWS_*`로만 받는다(사용자 결정 11:05 KST).

- **작업 자리.** 저장소는 `fvoci/musical-happiness`(비공개)이고 브랜치는 `master`다. 일마다 `master`에서 워크트리를 새로 만들고 끝나면 `master`에 합친다.
- **푸시.** 일한 세션이 자기 브랜치를 이름으로 올린다. 강제 푸시는 하지 않는다. **올라간 커밋은 `amend`·`rebase`·`reset`으로 고쳐 쓰지 않는다.** 고칠 것은 새 커밋으로 얹는다.
- **내장 템플릿은 저장소 안에 있다.** 웹앱 변환기의 산출물(`apps/web/tools/templates/output/`) 가운데 `index.json`이 적은 템플릿 파일 13개를 서버 크레이트의 `assets/templates/`에 복사해 바이너리에 넣는다. 새 워크트리에서도 그대로 컴파일된다. 옛 아카이브는 더 쓰지 않는다. 산출물을 다시 만들면 그 파일들을 다시 복사한다. 두 쪽이 다르면 `templates::the_server_copy_of_the_templates_is_the_converter_output`이 실패한다. `index.json`은 복사하지 않는다(테스트가 변환기 쪽의 것을 읽는다. 그 안의 원본 아카이브 경로는 웹앱의 테스트가 고정한다).
- **CI.** 이 저장소에는 워크플로 파일이 없다. 검증은 1절의 명령을 손으로 돌린다.

## 이 문서가 담는 것

저장소의 다른 파일에 적혀 있지 않은 것만 담는다: 검증 명령과 걸리는 시간, 스냅샷과 계약을 갱신하는 절차, 이 기기의 컨테이너, 걸렸던 함정, 리뷰를 맡긴 방식, 커밋되지 않는 조사 자료의 자리, 다음에 할 일.

| 알고 싶은 것 | 있는 곳 |
|---|---|
| 서버가 지금 어떻게 동작하나, 왜 그렇게 정했나, 리뷰의 지적과 처리, 남긴 문제 | `PORTING.md`의 "S6: 서버 정리"(A–Q절) |
| 결정된 설계와 구현 순서 | `docs/s6-design.md` |
| API의 계약 | `contracts/openapi.json` |
| 계약에서 곧 바뀔 부분, 그대로 둔 불규칙, 알려진 결함 | `docs/contract-pending.md` |
| 라우트와 영역별 세부, 결정 대기 항목의 지금 동작 | `crates/server/PORTING.md` |
| 스토어, 모델 | `crates/store/PORTING.md`, `crates/model/PORTING.md` |
| CDN과 프록시 뒤에 둘 때 | `docs/fvoci-cdn.md` |
| 웹앱의 계획 | 저장소 밖, 사용자의 계획 폴더의 `fvoci-web-svelte.md` |
| 웹앱의 편집기 스파이크, 날짜 모델, 계약에 대한 가정 | 웹앱의 문서 `apps/web/docs/spike-editor.md`, `apps/web/PORTING.md`, `apps/web/docs/contract-assumptions.md` |
| 원본(Focalboard의 Go 서버)과 견줄 때 쓰던 도구 | 7절 |

## 1. 검증

```bash
cargo xtask check    # 포맷, clippy, 단위 테스트, 문서, 이름, 층, 계약, 라이선스·권고. 외부 서비스 없음
cargo xtask test     # 컨테이너 준비 → SQLite 전체 → PostgreSQL(store, server) → S3 저장소 테스트
cargo xtask openapi  # contracts/openapi.json 이 서버가 내는 것과 다르면 실패. --write 가 고쳐 쓴다
```

**기대 결과.**

| 무엇 | 결과 |
|---|---|
| `cargo xtask check` | 여덟 단계 PASS. 층의 규칙은 `xtask/src/layers.rs`, 남은 위반은 `xtask/exceptions/layers.txt`(줄어들 수만 있다) |
| `cargo xtask test` | SQLite 1,095건, PostgreSQL 956건, S3 7건, 실패 0(2026-11-11, `fvoci/server-next`) |
| 회귀 스냅샷 | 591단계(시나리오 21개) |
| 권한 표 | 636칸 |

- 앞 두 실행의 `ignored` 7건은 S3 테스트다. 세 번째 단계가 그것을 돌린다.
- 수는 커밋마다 달라진다. 문서보다 실행 결과를 믿는다.
- 건너뛴 단계는 요약에 `SKIP`으로 나온다(`--skip-licenses`, `--database sqlite|postgres`). `SKIP`이 있는 실행을 전체 통과로 보고하지 않는다.
- `cargo xtask test`는 `FVOCI_REGRESSION_UPDATE`가 설정돼 있으면 시작하지 않는다(아래의 갱신 절차).
- `cargo xtask test`는 스스로 띄운 컨테이너를, 마지막으로 쓴 실행이 끝날 때 멈춘다(tracer-bullet 11).
- 환경 변수로 다른 PostgreSQL·S3·컨테이너 이름·포트를 주는 법은 `xtask/src/test.rs`와 `xtask/src/containers.rs`의 머리 주석에 있다.

**걸리는 시간.** 이 기기에서 빌드 캐시가 있을 때, 2026-11-11 02–03시 KST에 잰 값이다.

| 무엇 | 시간 |
|---|---|
| `cargo xtask check`, 소스를 고친 뒤 | 30–40초 |
| `cargo xtask check`, 고친 것이 없을 때 | 5초 |
| `cargo xtask test` | 2분 40초–4분 10초 |
| 회귀 테스트만(`cargo test -p fvoci-server --test regression`) | SQLite 29–30초 |
| API 테스트만(`--test api`, 487건) | SQLite 15초, PostgreSQL 37초 |

- `check`의 단계별 시간은 clippy 10초, 단위 테스트 3–13초, 문서 4초, 계약 3초, cargo-deny 1–2초다.
- `test`의 단계별 시간은 SQLite 65초, PostgreSQL 96–186초, S3 1초 미만이다. PostgreSQL은 기기가 바쁘면 길어진다. 186초였던 실행의 원인은 따로 보지 않았다.

**커밋마다 clippy를 통과시킨다.** 워크스페이스 린트가 엄격해서(`too_many_lines`, `as_conversions`, `print_stderr`, `similar_names` 등) 테스트 코드에서 자주 걸린다. `cargo doc`도 경고가 오류다. 없는 항목을 가리키는 문서 링크 하나로 check가 실패한다.

### 회귀 스냅샷을 갱신하는 절차

스냅샷은 `crates/server/tests/regression/golden/`에 있고 SQLite와 PostgreSQL이 같은 파일을 쓴다. 응답을 의도해서 바꿨을 때만 갱신한다.

1. 갱신하기 **전에** 비교로 한 번 돌려 무엇이 어긋나는지 읽는다: `cargo test -p fvoci-server --test regression`. 실패 메시지가 단계 이름, 요청, 다른 자리를 보여 준다. 바꾸려던 것만 어긋났는지 여기서 본다.
2. 기록한다: `FVOCI_REGRESSION_UPDATE=1 cargo test -p fvoci-server --test regression`. 다시 쓴 파일마다 `recorded <파일>: CHANGED|unchanged|NEW (not compared)` 한 줄이 나온다. 이 실행은 무엇을 받든 통과하므로 검증이 아니다.
3. `git diff -- crates/server/tests/regression/golden`을 **읽는다.** 단계별 요약은 `golden_diff.py <전 디렉터리> <뒤 디렉터리> [status|all]`로 뽑는다. 스크립트는 5절의 조사 자료에 있다. 전 것은 `git show <커밋>:<경로>`로 꺼낸다.
4. 변수 없이 다시 돌려 통과를 확인하고, PostgreSQL에서도 돌린다(`cargo xtask test`). 한 DB에서 기록한 것이 다른 DB에서 어긋나면 그 응답의 순서나 값이 DB에 달린 것이다.
5. 코드와 **같은 커밋**에 넣고, 무엇이 왜 바뀌었는지 `PORTING.md`에 적는다.

주의:

- 변수는 정확히 `1`이어야 한다. 다른 값(`0`, `true`)은 오류이고 `CI`에서는 기록을 거부한다. 셸에 `export`해 두지 않는다.
- 하네스는 목록을 **온 순서대로** 기록한다. 목록을 돌려주는 라우트마다 순서가 정해져 있고(`PORTING.md` M절, 계약의 라우트 설명) 스냅샷이 그 순서를 고정한다.
- 새 목록 라우트를 만들면 스토어의 쿼리에 순서를 넣고 `crates/store/tests/store/order.rs`에 테스트를 더한다. 순서 없는 목록은 두 DB에서 스냅샷이 어긋나는 것으로 드러난다.
- 타임스탬프는 모두 `<ts>`, ID는 `<id:N>`, `request_id`는 `<request-id>`로 바뀐다. 응답 본문을 **문자열로** 기록하는 관찰 단계는 이 정규화를 지나지 않는다. 한 번 그래서 요청마다 다른 `request_id`가 스냅샷에 들어가 다음 실행이 실패했다(`cards`의 "list: bad page"). JSON으로 기록하게 고쳤다.
- 권한 표의 준비 요청은 성공해야 한다. 실패하면 단언으로 멈춘다. 칸이 엉뚱한 이유의 상태 코드를 기록하지 않게 한 것이다.
- 없앤 라우트(채널, 통계, 컴플라이언스, 메타데이터)의 단계와 권한 표의 행은 남겨 두었다. 404를 고정한다.

### 계약을 고치는 절차

`contracts/openapi.json`은 손으로 고치지 않는다. 핸들러의 `#[utoipa::path]`와 문서 주석, 와이어 타입, `api/contract.rs`의 표에서 나온다. 만드는 방식은 `PORTING.md` O절에 있다.

1. 라우트를 더하거나 고친다. 핸들러에 `#[utoipa::path(...)]`를 달고 `routes()`에 `routes!(핸들러)`로 넣는다. 주석 없는 핸들러는 `routes!`가 받지 않는다.
2. 새 스키마가 생기면 `api/contract.rs`의 `SCHEMAS`에 늘 있는 멤버를 적는다. 빠뜨리면 문서를 만드는 일이 실패하고 무엇이 빠졌는지 말한다.
3. 요청의 모양이 답과 다르면 `api/contract/bodies.rs`에 문서 전용 타입을 둔다. 해석하는 타입과 맞는지는 사람이 본다(검사가 없다).
4. 목록을 답하면 순서를 설명에 적고 `contract.rs`의 테스트 표 `ORDERS`에 넣는다. 빠뜨리면 `every_list_says_its_order`가 실패한다.
5. `cargo xtask openapi --write`로 파일을 다시 쓰고, `git diff contracts/`를 읽고, 코드와 같은 커밋에 넣는다. 곧 바뀔 부분이 달라졌으면 `docs/contract-pending.md`도 고친다.

### xtask에 넣지 않은 것

```bash
target/tools/bin/cargo-about generate --manifest-path crates/server/Cargo.toml -o NOTICE about.hbs   # 의존성이 바뀌었을 때
DOCKER_BUILDKIT=1 docker build -f docker/fvoci/Dockerfile -t fvoci-server:s5 .                      # 이미지
```

- 의존성이 바뀐 커밋마다 `NOTICE`를 다시 만든다. 마지막으로 만든 것은 응답 압축(tower-http의 압축, brotli, zstd)을 더했을 때다(tracer-bullet 10). `zstd-sys`가 넣는 C 라이브러리의 BSD 고지는 cargo-about이 보지 못해 `about.hbs`에 손으로 적었다.
- 이미지는 회귀 테스트를 세운 때(`PORTING.md` A절) 뒤로 다시 빌드하지 않았다. 이 기기에 있는 `fvoci-server:s5`는 태그 `parity-s5` 시점의 서버다. 지금의 서버와 스키마도 API도 다르다.
- `cargo xtask check`의 마지막 단계는 `cargo-deny`가 있어야 돈다. 워크트리의 `target/tools/bin`이나 검색 경로에서 찾는다. 워크트리에 두는 명령: `CARGO_TARGET_DIR=$PWD/target/tools/build cargo install cargo-deny --locked --root $PWD/target/tools`.

## 2. 이 기기의 상태

**평소에 떠 있는 컨테이너는 없다.** 2026-11-11 03:28 KST에 사용자가 이 작업의 컨테이너 다섯(`hafgufa-pg`, `hafgufa-s3`, `focalboard-port-maria`, `focalboard-port-mysql`, `focalboard-port-pg`)과 다 쓴 볼륨을 모두 지웠다. 테스트가 PostgreSQL에 쌓아 둔 `t_*` 스키마도 함께 사라졌다. 그 뒤로는 `cargo xtask test`가 둘을 만들고(PostgreSQL은 `postgres:18`), 테스트 실행이 시작하거나 만든 것은 그것을 쓰는 마지막 실행이 끝날 때 멈춘다(tracer-bullet 11). 멈춘 채 남아 있는 것은 다음 `cargo xtask test`가 다시 시작한다. 이름은 2026-11-11 12:10 KST에 `hafgufa-pg`, `hafgufa-s3`에서 `fvoci-test-pg`, `fvoci-test-s3`로 바뀌었다. 옛 이름의 둘은 멈춘 채 이 기기에 남아 있고 더 쓰이지 않는다(지울지는 사용자가 정한다. 같은 포트를 쓰므로 옛 것을 띄우면 새 것이 시작하지 못한다).

**컨테이너를 쓰는 규칙(사용자 결정).**

- 컨테이너는 필요할 때 만들고 그 일이 끝나면 닫는다.
- 테스트를 돌린 세션은 작업 묶음이 끝날 때 자기가 띄운 컨테이너와 프로세스를 멈춘다. `cargo xtask test`가 이것을 스스로 한다.
  - 이 기기의 워크트리들이 컨테이너를 함께 쓴다. 테스트 실행이 시작하거나 만든 컨테이너는 그것을 쓰는 마지막 실행이 끝날 때(통과, 실패, Ctrl-C 모두) 멈춘다. 다른 실행이 아직 쓰는 동안에는 멈추지 않는다. 테스트 실행이 띄우지 않은 것(손으로 띄운 것)은 그대로 둔다.
  - 실행들은 시스템 임시 폴더의 `fvoci-xtask-containers/`에서 만난다. 컨테이너마다 쓰는 동안 잡는 잠금, 시작하고 멈추는 동안 잡는 잠금, 테스트 실행이 시작했다는 표시(`<이름>.started`)가 있다. 표시는 멈출 때 지운다.
  - Ctrl-C를 받으면 돌던 단계가 끝나기를 기다린 뒤 나머지를 건너뛰고, 컨테이너를 놓는다. 컨테이너를 기다리던 중이면 기다림을 그만둔다. 그 사이 다시 Ctrl-C를 눌러도 같고, `docker stop`은 Ctrl-C를 받지 않는 제 프로세스 그룹에서 돈다.
  - 터미널이 아니라 xtask 프로세스에만 보낸 `SIGTERM`은 돌던 `cargo test`에 닿지 않는다. xtask는 그 단계가 끝나기를 기다린다. `SIGKILL`로 죽이면 컨테이너가 떠 있는 채 남는다. 표시가 남아 있으므로, 그 컨테이너를 쓰는 다음 실행이 끝날 때 멈춘다.
- `supabase-*`는 다른 작업의 것이다. 건드리지 않는다. 포트 8000의 서비스(`supabase-envoy`)도 그 작업의 것이다.

`cargo xtask test`가 만드는 컨테이너는 둘이다. 값은 모두 테스트 전용이다. 떠 있는 동안에는 워크트리 여럿이 함께 쓴다.

| 컨테이너 | 주소 | 접속 |
|---|---|---|
| `fvoci-test-pg` (PostgreSQL) | `127.0.0.1:15433` | 사용자 `fvoci`, 비밀번호 `fvoci-test`, DB `fvoci` |
| `fvoci-test-s3` (S3 호환) | `127.0.0.1:18333` | 인증 없음, 버킷 `fvoci-test` |

- `cargo xtask test`가 멈춘 컨테이너는 시작하고 없는 것은 만든다. 재시작 정책은 없다.
- 같은 이름의 컨테이너가 다른 포트에 게시돼 있으면 xtask가 거절한다.
- 패닉으로 끝난 테스트는 PostgreSQL에 `t_*` 스키마를 남긴다. 해는 없다. 컨테이너를 지우면 함께 없어진다.
- `/tmp`는 RAM이다. 빌드 산출물과 큰 파일은 워크트리의 `target/` 아래에 둔다.
- **빌드 크기.** `[profile.dev]`의 `debug = "line-tables-only"`(2026-11-11 12:49 KST). 깨끗한 빌드에서 잰 값: 디버그 정보 전부(2) 빌드 89초·`target` 6.6 GB·서버 바이너리 317 MB, `line-tables-only` 56초·3.78 GB·124 MB, `1` 59초·4.41 GB·185 MB, `0` 57초·2.80 GB·76 MB. clippy는 어느 것이나 28–34초.
- 태그 `parity-s5`는 이 저장소의 원격에도 있다.

컨테이너를 만드는 명령과 새 클론에서 처음 빌드할 때 알아 둘 것은 보관 폴더의 `server/README.md` 4–5절에 있다. 그 문서는 컨테이너를 지우기 전(03:12–03:19 KST)에 쓰였다. 컨테이너가 떠 있다는 말과 스키마의 수는 지금과 다르다.

## 3. 걸렸던 함정

앞선 판 5절의 것(`/tmp`는 RAM, `.gitignore`가 무시하는 디렉터리 이름, 셸의 `cd`, 시간 제한 없는 `curl` 등)은 그대로 유효하다. 아래는 그 뒤에 새로 걸린 것이다.

요청 본문과 한도

- **413을 바로 답하면 클라이언트가 못 받는다.** 본문을 쓰는 중에 연결이 닫혀 broken pipe가 된다. 4 MiB+1바이트 요청 9번 중 1번이 그랬다. 그래서 거절한 본문을 8 MiB·5초까지 읽어 버린 뒤 답한다(`api/limits.rs`의 `discard`). 같은 테스트를 6번(요청 54번) 돌려 모두 413을 받았다. 업로드 한도를 넣을 때 같은 일이 생긴다. 단, `Expect: 100-continue`가 있으면 읽지 않는다(읽는 것이 "보내라"가 된다).
- **axum의 추출기 한도(2 MB)는 자기 형식의 평문 413을 낸다.** 우리 한도 아래에서 먼저 걸린다. 그래서 라우터 묶음마다 `DefaultBodyLimit::disable()`과 우리 미들웨어를 함께 둔다. 새 라우터 묶음을 만들면 둘 다 붙인다. 관리 소켓에서 한 번 빠뜨렸다.
- **한도 미들웨어는 세션 검사보다 먼저 돈다.** 그래서 토큰이 없는 요청은 어디서나 64 KiB다. 토큰이 있는데 본문이 64 KiB를 넘으면 미들웨어가 세션을 먼저 확인한다. 없는 세션이면 본문을 읽지 않고 401이다. 본문이 없는 라우트에 본문을 보내면 한도까지는 읽는다.

오류 응답과 404 규칙

- **새 라우트는 `tests/api/visibility.rs`의 표에 넣는다.** "숨겨진 것"과 "없는 것"의 답이 상태·코드·문구까지 같아야 한다. 문구에 요청의 ID를 되풀이하면 표의 비교가 실패한다. boards-and-blocks의 문구를 그렇게 고쳤다.
- **친절한 409가 구멍이 된다.** 카테고리를 만들 때 겹치는 ID에 409를 답하게 했다가, 그 답이 남의 카테고리 ID의 존재를 알려 준다는 것을 리뷰가 찾았다. 클라이언트가 ID를 정하는 자리는 모두 같은 문제다.
- **저장소의 실패를 "없음"이나 "권한 없음"으로 읽지 않는다.** `has_permission_to_board`는 `Result<bool>`이다. `.is_ok()`, `Err(_) =>`로 뭉개면 DB가 잠깐 실패할 때 모든 보드가 404가 된다.
- **`AppError::context`는 로그로만 간다.** 4xx의 문구는 바뀌지 않는다. 클라이언트에게 할 말은 생성자에 적는다.
- **레코드 본문은 `decode_object`로 읽는다.** `decode_json`은 배열을 멤버 순서대로 읽어 버린다(`[]`가 빈 패치가 됐다). 목록이 본문인 라우트만 `decode_json`이다.

작업 방식

- **임시 파일은 파일 하나만 지운다.** 스키마를 시험하던 임시 테스트를 치우며 `rm -rf crates/model/tests`를 했다. 커밋된 테스트 다섯 파일이 함께 지워진 것을 한참 뒤 `git status`에서 알았다. `git checkout HEAD -- <경로>`로 되돌렸다. 시험용 파일은 `target/` 아래나 스크래치 디렉터리에 둔다.
- **파이썬으로 여러 파일을 고치는 스크립트는 `assert` 하나에 통째로 멈춘다.** 앞의 파일은 써졌고 뒤는 안 써진 상태가 된다. 실패 뒤에는 어느 편집이 들어갔는지 다시 본다. `cargo fmt`가 줄을 다시 나누므로 다음 스크립트의 찾을 문자열은 포맷한 뒤의 글에서 잡는다.
- **한 작업 트리의 변경을 두 커밋으로 나눌 때.** 뒤 커밋의 파일을 `target/` 아래에 복사해 둔다. 작업 트리를 앞 커밋의 모양으로 되돌려 검증하고 커밋한다. 복사본을 되돌려 놓고 다시 검증해 커밋한다. `git stash`는 쓰지 않는다(스택이 워크트리끼리 공유된다).
- **수는 다시 센다.** 스냅샷 요약에서 "상태가 바뀐 단계"를 눈으로 더해 77로 적었고 스크립트로 세니 76이었다. 그 커밋의 메시지에는 77이 남아 있다.
- **시각은 `date`로 확인한다.** 커밋 메시지에 짐작한 시각을 적었다가 고쳤다.
- **닫힌 로컬 포트로의 연결은 거절되지 않고 매달린다.** 포트 탐침에는 제한 시간을 둔다(xtask의 탐침에 2초를 넣었다).

## 4. 리뷰를 맡긴 방식

`beautiful-rust:rust-review` 스킬을 하위 에이전트에게 맡겼다. 결과와 처리는 `PORTING.md`의 F절, J절, P절에 있다. 여기에는 방식만 적는다.

- 에이전트는 `general-purpose`이고 모델을 `fable`로 지정했다(Fable 5.2). 보고 끝에 자기 모델과 스킬을 읽었는지 적게 했다.
- 범위를 겹치지 않게 나눴다. 예: 하나는 커밋 둘의 `git diff`, 하나는 앞서 리뷰하지 않은 커밋들을 "지금의 코드로". 뒤의 것은 뒤 커밋이 이미 고친 줄을 지적하지 않게 하려는 것이다.
- 지침에 넣은 것:
  - 읽기 전용. 파일, git 상태, 컨테이너, 프로세스를 바꾸지 말 것. `cargo fmt`는 `--check`만.
  - 저장소가 무엇인지 두 문장, 의도를 적은 설계 문서의 절 번호.
  - 일반 점검표 말고 **특히 봐 달라는 것**의 목록. 예: 404 규칙의 구멍, 한도 미들웨어의 우회, 리다이렉트, 낡은 주석.
  - 출력 형식: Must fix / Should fix / Consider, `경로:줄`, 구체적인 입력과 잘못된 결과, 고치는 법. 읽어서 확인한 것만 적고 아니면 "unverified".
- 내 결론은 주지 않았다. "이 부분은 괜찮다"고 적으면 그대로 돌아온다.
- 각각 9–12분, 토큰 26만–29만이 들었다. 백그라운드로 돌리고 그동안 문서를 썼다.

주의점:

- **리뷰어는 테스트를 돌리지 않았다.** 지적마다 코드를 읽거나 테스트를 써서 확인한 뒤에 고쳤다. 대부분 맞았다. 틀린 예: 권한 표의 준비 요청에 단언을 넣으라는 지적은 그대로 넣자 멤버가 아닌 호출자에서 실패했다. 그 호출자는 준비가 실패하는 것이 맞다.
- **사용자 결정과 부딪히는 지적이 있다.** 사용자가 정한 값을 바꾸라는 지적은 고치지 않고 보고에 올린다.
- **"남긴다"의 이유는 확인해서 적는다.** 하네스의 정렬을 남겼던 이유("순서가 정해지지 않은 목록이 있다")는 정렬을 끄고 두 DB에서 돌려 본 뒤에야 근거가 됐다.
- 리뷰어는 이 세션의 제약(건드리면 안 되는 컨테이너와 다른 워크트리)을 모른다. 지침에 다시 적는다.
- 줄 번호는 지침에 적은 커밋 기준이다. 그 뒤에 고쳤으면 어긋난다.
- 리뷰가 찾은 것 가운데 둘은 그 변경이 새로 만든 결함이었다(위의 409, 세션 조회의 실패가 401이 되는 것). 응답 규칙을 바꾸는 단계에서는 리뷰를 건너뛰지 않는다.

## 5. `target/s6/`의 조사 자료 (커밋되지 않는다)

이 자료는 저장소에 없다. 이 기기의 보관 폴더 `server/target/s6/`에 있다. 무엇을 옮겼고 무엇을 옮기지 않았는지는 그 폴더의 `README.md` 1–2절에 있다. 아래에는 다음 단계에 쓰는 것만 적는다.

| 파일 | 무엇 | 언제 쓰나 |
|---|---|---|
| `research/facts.md` | 설계의 보완 a–f의 근거가 된 바깥 문서의 인용문. 출처 주소와 받은 시각이 있다 | 설계 5.1–5.6을 구현할 때. 설계 문서는 요약만 싣는다 |
| `research/raw/` | 받은 원문. 목록은 표 아래 | 인용문의 앞뒤를 볼 때. 다시 받으면 판이 달라졌을 수 있다(초안 둘은 RFC가 아니다) |
| `gz/` | 형제 저장소 GroundZero의 `contracts/openapi.json`과 xtask 일부 | OpenAPI 도구의 구성을 다시 볼 때(설계 5.5). 코드를 복사하지 않고 구성만 참고한다 |
| `golden_diff.py` | 스냅샷 두 벌을 단계별로 비교해 상태 코드의 전이를 센다 | 스냅샷을 갱신한 뒤의 요약(1절) |
| `hashtime/` | Argon2id 한 번의 시간을 잰 작은 프로그램 | 해시 대기열의 수치를 다시 볼 때(`PORTING.md` I절) |
| `xtask-test-*.log` 등 단계마다의 출력 | 전체 테스트와 검사의 출력 | 그때의 수를 대조할 때 |
| `7d/annotate.py`, `7d/*.spec` | 핸들러의 문서 주석과 `#[utoipa::path]`를 영역별 명세 파일에서 넣은 스크립트 | 볼 일이 거의 없다. 주석은 이제 소스에서 직접 고친다 |
| `review-todo.md`, `review-final/todo.md` | 리뷰의 지적을 모은 목록. `PORTING.md` J절의 번호(M1, S1 등)가 앞의 것이다 | 지적의 원문을 볼 때 |

- `research/raw/`에 든 것: 쿠키 규격 초안 6265bis, RFC 6455·7239, WHATWG WebSockets·Fetch, OWASP 문서 넷과 ASVS V7, NIST 800-63B, SQLite·PostgreSQL의 외래 키와 부분 색인, S3·R2의 멀티파트, Cloudflare와 CloudFront의 한도, tus, IETF 재개 업로드 초안, `chrono-tz`·`jiff`의 레지스트리 정보.
- 보관 폴더로 옮기지 않은 것 가운데 다시 쓸 만한 것은 하나다. 내장 템플릿 아카이브를 푼 `tpl/`이고, 템플릿 산출물을 받을 때 대조하는 데 쓴다(설계 5.7). `python3 -m zipfile -e server/assets/templates.boardarchive target/s6/tpl`로 다시 만든다. 주인 없는 블록과 끊어진 참조를 센 스크립트는 남기지 않았다. 세는 법은 설계 문서 맨 끝 "원본과 견준 것"의 표에 적은 정의 그대로다.
- 그 밖에 옮기지 않은 것은 지워도 되는 것들이다: 계약에서 `openapi-typescript`로 뽑아 본 타입(`7d/ts/api.d.ts`), 단계 앞의 스냅샷 사본(`golden-*`), 요청 크기를 재려고 임시로 고친 빌드(`fvoci-server-measure`, 설계 4.1), 작업 중의 사본(`final/`, `s4/`).

## 6. 다음 세션이 먼저 볼 것

**끝난 작업 묶음.** 설계의 "구현 순서" 8단계와 9단계는 `fvoci/server-next`가 했다. 바뀐 조작과 멤버는 `docs/contract-pending.md`에 있다. 커밋마다 계약 파일과 그 문서를 함께 고친다.

1. **(끝났다)** **본문 저장과 날짜 값**(설계 3.7, 5.7). 허용 목록 판 2와 공유 벡터 `contracts/body-vectors.json`이 들어갔다(맨 앞 "지금 상태"의 tracer-bullet 목록).
2. **(끝났다)** **템플릿 받기**(설계 3.10, 5.7). 웹앱의 변환기가 낸 산출물을 받아 넣고, 저장소 밖의 템플릿 아카이브에 대한 의존을 끊었다. 새 클론에서 서버가 그대로 컴파일된다.
3. **(끝났다)** **청크 업로드, 스트리밍, 파일 한도**(설계 4.3, 5.6).
4. **(끝났다, tracer-bullet 10)** **응답 압축에 zstd를 더한다.** 결정된 것은 gzip, brotli, zstd 셋이다. 정적 파일은 웹앱의 빌드가 `.zst`를 이미 만든다. 서버는 지금 미리 압축된 brotli와 gzip 파일만 골라 보낸다(`crates/server/src/web.rs`). `.zst`를 골라 보내는 일이 남았다.
5. **(끝났다, tracer-bullet 11)** **`cargo xtask test`가 스스로 띄운 컨테이너를 끝날 때 멈추게 한다.** 함께 컨테이너 이름에서 옛 워크트리 이름(`hafgufa-`)을 뗀다. 이름의 기본값은 `xtask/src/containers.rs`에 있다.

**후속 후보(이 묶음 밖, 사용자 결정 2026-11-11 05:04 KST).** 설정에 자리만 있고 아직 아무 일도 하지 않는 예약된 키 둘이다(`docs/configuration.md`의 "예약된 키").

- `prometheusaddress`: Prometheus 지표를 내는 주소.
- `authmode`: OIDC 로그인. 지금은 `native`만 받는다.

**웹앱 쪽에 전할 것.**

- 계약 파일 `contracts/openapi.json`과 `docs/contract-pending.md`. 웹앱과 같은 트리에 있다.
- 타입 생성은 웹앱 쪽이 한다. `openapi-typescript` 7.13.0이 이 파일을 받는 것은 확인했다.
- 날짜 값의 시각 표기는 UTC 밀리초 정수다(설계 5.7).
- WebSocket 메시지는 계약에 없다(`docs/contract-pending.md` 6절).
- 바꾸는 요청은 `fetch`의 기본 방식(`cors`)으로 보낸다. 서버가 페이지에 `Referrer-Policy: no-referrer`를 붙인다. 그래서 다른 방식(`same-origin`, `no-cors`, 브라우저가 직접 보내는 폼)의 요청은 `Origin: null`이 되어 403이다(계약의 머리말, `PORTING.md` P절).
- API 밖의 주소는 `/ws`, `/static/…`, 웹앱의 화면뿐이다. `hello`와 `ping`도 `/api/v1` 아래에 있다(`PORTING.md` Q절).

**남은 문제를 찾는 곳.**

- 마지막 리뷰의 지적과 반영, 남긴 것과 까닭: `PORTING.md` P절.
- 서버의 결함으로 남은 것: `docs/contract-pending.md` 8절.
- 알려진 채로 남긴 불규칙: `docs/contract-pending.md` 7절과 `PORTING.md`의 S6 각 절.
- 사용자의 확인이 필요한 채로 남은 것: `PORTING.md`의 "S6: 서버 정리" 머리에 모았다.
- 큰 것 셋: 요청 스키마를 손으로 쓴 것(해석 쪽과 어긋나도 검사가 없다), 멤버십에 시각이 없어 멤버 목록이 ID 순서인 것, WebSocket 메시지가 계약 밖인 것.

## 7. 원본(Focalboard의 Go 서버)과 견줄 때 쓰던 것

태그 `parity-s5`까지는 서버의 응답을 원본 서버와 견줘 맞췄다. 그 뒤로는 원본과의 호환을 지키지 않으므로 아래의 것은 더 쓰지 않는다. 기록으로만 남긴다.

| 무엇 | 있는 곳 |
|---|---|
| 원본 서버(오라클)를 다시 빌드하는 법, 차등 비교 스크립트, 개발 서버 스크립트의 전문 | 앞선 판의 1절 "레거시", 3절, 4절 |
| 스크립트 자체(`scripts/`) | 태그 `parity-s5` |
| S5의 검증 결과, DB 왕복의 데이터, Cypress 로그, 플러그인 저장소 PR의 사본(`target/reference/`), 하위 에이전트에게 준 지침서(`target/agent-brief.md`) | 보관 폴더 `server/` 아래의 같은 경로 |
| 회귀 테스트를 세울 때 Python 차등 비교와 대조한 기록(`baseline-differential.txt`, `baseline-permissions.txt`, `crosscheck.txt`) | 보관 폴더의 `target/s6/` |
| 원본 서버의 바이너리와 Go 캐시(`target/oracle/`), 원본 화면을 눌러 보던 개발 서버 둘의 데이터(`target/dev/`) | 옛 저장소의 서버 워크트리에만 있다. 옮기지 않았다 |

원본 화면을 눌러 보던 개발 서버 둘(포트 8002의 Rust 서버 + 원본 React 번들, 포트 8001의 원본 Go 서버)은 2026-11-10 19:41 KST에 멈췄다. 호환을 끝낸 뒤의 서버는 원본 번들과 맞지 않는다(오류 본문, 404 규칙, 없앤 라우트). 다시 띄울 일이 없다.

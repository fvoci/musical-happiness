# fvoci 포팅 노트 (전체)

이 저장소의 `server/`(Go)와 `webapp/`(React/TypeScript)를 Rust로 옮긴 결과가 `crates/` 아래에 있다. 이 문서는 전체에 걸친 사실만 적고, 크레이트별 API 매핑·테스트 매핑·의도적 차이·보존한 quirk는 각 크레이트의 `PORTING.md`에 있다. 원본 참조는 저장소 기준 상대 경로와 행 번호로 적는다.

## 기준

- 포팅 기준 커밋: `a84bbb65`. 원본이 바뀌면 이 커밋과의 차이로 추적한다.
- 배포 라이선스: MIT (`LICENSE`).
- 범위: standalone 서버(native 인증, single-user 모드). 플러그인 모드와 그 전용 코드는 옮기지 않는다.

## 크레이트

| 크레이트 | 경로 | 원본 | 상태 |
|---|---|---|---|
| `fvoci-model` | `crates/model` | `server/model`, `server/utils/utils.go`, `server/services/auth/email.go` | 완료 (S1) |
| `fvoci-store` | `crates/store` | `server/services/store` | 완료 (S2) |
| `fvoci-server` | `crates/server` | `server/api`, `server/app`, `server/auth`, `server/ws`, `server/web`, `server/server`, `server/main`, `server/services/*` | 완료 (S3–S5) |

## 검증 도구

S6부터의 진입점은 `cargo xtask check`와 `cargo xtask test`다(아래 "S6" 절, 명령의 세부는 `docs/handoff-s5.md` 1절). 표에서 "레거시"라고 적은 것은 태그 `parity-s5`까지 원본과 비교하는 데 쓴 도구로, 이후의 변경에는 쓰지 않는다. **`scripts/` 아래의 것은 주 브랜치에서 걷어 냈고(2026-11-10 17:44 KST) 태그 `parity-s5`에 남아 있다.** 꺼내 쓰는 방법은 `docs/handoff-s5.md` 1절에 있다.

| 도구 | 위치 | 용도 |
|---|---|---|
| 검사 | `cargo xtask check` (`xtask/`) | 포맷, clippy, 단위 테스트, 문서, 이름, 라이선스·권고. 외부 서비스 없음 |
| 테스트 | `cargo xtask test` | 컨테이너 준비 뒤 SQLite 전체, PostgreSQL(store, server), S3 저장소 테스트 |
| 회귀 테스트 | `crates/server/tests/regression/` (스냅샷은 `golden/`) | `parity-s5`의 응답을 기대값으로 고정한 시나리오 15개(382단계)와 권한 표(636칸). `cargo test --workspace`에 포함. 갱신은 `FVOCI_REGRESSION_UPDATE=1` |
| 오라클 (레거시) | `target/oracle/go-server`, 실행은 `scripts/oracle-run.sh <name> <port> [single-user 토큰] [dbtype] [dbconfig]` | 원본 서버를 Go 1.21.8, 태그 `json1 sqlite3`로 빌드한 바이너리. 차등 비교, 스키마 비교, DB 왕복에 쓴다 |
| PostgreSQL | 컨테이너 `hafgufa-pg` (127.0.0.1:15433, PostgreSQL 18.6) | 스토어·서버 테스트의 PostgreSQL 실행. `FVOCI_STORE_TEST_DB_TYPE=postgres`, `FVOCI_STORE_TEST_CONN_STRING=postgres://fvoci:fvoci-test@localhost:15433/fvoci?sslmode=disable` |
| 원본 웹 번들 (레거시) | `webapp/pack` | `npm ci && npm run pack` (Node 26.13.0에서 빌드됨) |
| Cypress (레거시) | `scripts/e2e-cypress.sh <label> <server>` | 원본 스펙 6개(테스트 11개)를 임의의 서버 바이너리에 실행. 결과는 `target/e2e/<label>/` |

| 포트 실행 (레거시) | `scripts/fvoci-run.sh <name> <port> [single-user 토큰] [dbtype] [dbconfig] [filespath]` | 오라클 실행 스크립트와 같은 인자로 Rust 서버를 띄운다 |
| 차등 비교 (레거시) | `scripts/differential/run.py --left <원본 URL> --right <포트 URL>` | 같은 요청 시퀀스(시나리오 15개)를 두 서버에 보내 상태 코드와 JSON을 비교한다. ID·시각·비밀값은 정규화한다 |
| 권한 표 (레거시) | `scripts/differential/permissions.py --left … --right …` | 라우트 × 역할 × 보드 종류별 상태 코드를 비교한다 |
| DB 왕복 (레거시) | `scripts/differential/roundtrip.sh sqlite` / `postgres <URL>` | 한 서버가 쓴 DB와 파일을 다른 서버로 읽고 쓴다 |
| S3 호환 서버 | 컨테이너 `hafgufa-s3` (SeaweedFS, 127.0.0.1:18333, 버킷 `fvoci-test`) | S3 파일 백엔드 테스트 |
| 라이선스 | `cargo deny check`(`deny.toml`), `cargo about generate …`(`about.toml`, `about.hbs`) | 의존성 라이선스·권고 검사, `NOTICE` 생성. 도구는 `target/tools/bin`에 설치했다 |

등록된 의도적 차이는 `scripts/differential/expected.py`에 있다(태그 `parity-s5`). 빌드 산출물, 오라클의 Go 빌드 캐시, 테스트 DB, E2E 실행 디렉터리는 모두 `target/` 아래에 두며 git에서 무시한다.

### 기준선 (S0, 2026-11-10)

- 오라클 빌드: 성공 (`target/oracle/go-server`, 47.6 MB).
- 원본 웹 번들 빌드: 성공 (webpack 경고 3건).
- Cypress 기준선(오라클 + 원본 번들): **11/11 통과**
  (`loginActions` 1, `createBoard` 4, `manageGroups` 3, `groupByProperty` 1, `cardBadges` 1, `cardURLProperty` 1).

## 이름

산출물의 이름은 `fvoci`다(크레이트 `fvoci-*`, 바이너리 `fvoci-server`, env 접두사 `FVOCI_`, 쿠키 `FVOCIAUTHTOKEN`). 옛 이름의 별칭은 두지 않는다. `crates/` 아래에는 원본의 두 이름이 한 번도 나오지 않아야 하며, 오라클을 실행할 때 필요한 옛 이름의 env는 `scripts/` 쪽에 둔다.

## 사용자 결정이 필요한 항목

포팅 중에 나온 질문을 여기에 모은다. 답을 받기 전까지는 각 항목에 적힌 "현재 동작"으로 둔다.

| # | 항목 | 현재 동작 | 단계 |
|---|---|---|---|
| 1 | **사용자 설정(preferences)의 카테고리 값.** 원본은 `preferences` 테이블의 `category` 컬럼에 원본 제품 이름을 값으로 쓴다(`server/services/store/sqlstore/user.go:273,297,378`). 계획은 "DB에는 옛 이름이 없다"고 전제했지만 이 한 곳에는 있다. `crates/` 아래에 옛 이름을 두지 않는다는 조건과 Go 서버와의 DB 호환이 여기서 충돌한다 | **닫힘(사용자 결정 2026-11-10): `fvoci`로 확정.** 코드는 처음부터 이 값이라 바뀐 것이 없다. 호환을 끝냈으므로 원본이 만든 DB와 설정을 주고받을 일이 없다. 새 기준 스키마에서는 값이 하나뿐인 이 열 자체를 없애는 안을 냈다(`docs/s6-design.md` 3.1) | S2, S6에서 정리됨 |
| 2 | **viewer·commenter의 카드 단건 조회.** 원본은 `GET /cards/{id}`에 `ManageBoardCards`를 요구해 viewer와 commenter가 403을 받는다(`server/api/cards.go:363`) | **결정됨(2026-11-10): 고침.** `ViewBoard`로 바꿨다. `crates/server/PORTING.md` 의도적 차이 45 | S4 |
| 3 | **보안 결정 대기 항목.** 고치면 정상 클라이언트의 동작이나 와이어 형식이 바뀌는 보안 관련 항목은 `crates/server/PORTING.md`의 "보안 결정 대기" 표(S1~)에 모았다 | S5까지는 전부 원본대로였다. **S6에서 정리됨:** S3, S4, S12, S14, S15, S16, S18을 고쳤다(아래 "S6" 절의 C). 남은 S1·S2(관리자), S5·S17(크기 한도), S6·S9·S11(404 범위), S8(공유 토큰)은 설계 안을 냈고 확인을 기다린다(`docs/s6-design.md`) | S3–S4, S6 |
| 4 | **의존성 보안 권고.** 번들 SQLite의 RUSTSEC-2026-0459(DB 파일과 크기가 다른 `-wal` 파일을 함께 열면 초기화되지 않은 메모리를 돌려줄 수 있다). 최신 sqlx 0.9.0이 수정된 `libsqlite3-sys` 0.39를 받지 않는다 | `deny.toml`에 사유와 함께 예외로 등록. 서버는 운영자가 지정한 DB 파일만 연다. sqlx가 올라가면 예외를 지운다. **S6에서 조사(2026-11-10), 고치지 않음:** 아래 "결정 4의 조사" | S5 |

### 결정 4의 조사: RUSTSEC-2026-0459 (2026-11-10)

- **무엇인가.** CVE-2026-119375. SQLite 3.54.0 전의 판은 DB 파일과 **페이지 크기가 다른** `-wal` 파일(write-ahead log)을 함께 열면 초기화되지 않은 페이지 캐시 메모리를 돌려줄 수 있다. 조작한 파일 한 쌍을 열게 하면 평범한 질의가 그 프로세스 메모리의 묵은 바이트를 내놓는다. 권고의 분류는 메모리 노출, 공격 경로는 로컬이고 피해자가 그 파일을 열어야 한다(CVSS 4.0 벡터 `AV:L/AC:L/AT:N/PR:N/UI:P/VC:H/VI:N/VA:N`). 영향받는 것은 SQLite를 소스로 묶어 빌드하는 `libsqlite3-sys` 0.6.0–0.38.2이고, 0.39.0(SQLite 3.54.0)에서 고쳐졌다. 출처: 권고 DB의 `crates/libsqlite3-sys/RUSTSEC-2026-0459.md`.
- **여기서는.** `Cargo.lock`의 `libsqlite3-sys`는 0.37.0이고 `sqlx`의 `sqlite` 기능이 묶음 빌드를 켠다. 그래서 `cargo deny`가 이 권고를 잡는다.
- **왜 예외로 두었나.** 두 가지다. (1) 올릴 수 없다. 최신 릴리스인 `sqlx-sqlite` 0.9.0이 `libsqlite3-sys`를 `>=0.30.1, <0.38.0`으로 묶어 두어 0.39를 받지 않는다. (2) 성립 조건이 이 서버에 맞지 않는다. 서버는 운영자가 설정한 DB 파일 하나만 연다. 공격하려면 그 파일과 옆의 `-wal` 파일을 바꿔치기할 수 있어야 하는데, 그럴 수 있는 사람은 이미 DB 전체를 읽고 쓸 수 있다. 사용자가 올린 파일을 SQLite로 여는 경로는 없다(아카이브 가져오기는 ZIP과 JSON만 읽는다).
- **언제 해소되나.** sqlx의 다음 릴리스. sqlx 저장소의 `main`은 이미 `sqlx-sqlite`의 범위를 `>=0.30.1, <0.40.0`으로 넓혔다(2026-11-10에 저장소의 `sqlx-sqlite/Cargo.toml`로 확인). 그 릴리스가 나오면 `cargo update`로 `libsqlite3-sys`를 0.39 이상으로 올리고 `deny.toml`의 예외를 지운다. 릴리스 날짜는 확인하지 못했다.
- **그때까지 할 수 있는 다른 것(하지 않았다).** `sqlite-unbundled` 기능으로 시스템의 SQLite에 링크하면 이 크레이트의 묶음 사본은 쓰지 않게 되지만, 노출이 배포 환경의 SQLite 판에 달리게 되고 Docker 이미지에 라이브러리를 넣어야 한다. 릴리스되지 않은 sqlx를 git 의존성으로 쓰는 것은 `deny.toml`의 출처 규칙(`unknown-git = "deny"`)에 어긋난다.

## 서버 트랙 결과 (S0–S5)

| 단계 | 결과 |
|---|---|
| S0 | 워크스페이스, MIT 라이선스, 오라클 빌드, 원본 번들 빌드, Cypress 기준선 11/11 |
| S1 | `fvoci-model`: 와이어 타입. 테스트 69개 |
| S2 | `fvoci-store`: SQLite + PostgreSQL. 테스트 121개, 양쪽 DB에서 통과 |
| S3–S4 | `fvoci-server`: standalone API 전체, WebSocket, 정적 서빙, 파일(로컬·S3), 아카이브, 템플릿, 온보딩, 웹훅, 세션 정리, 관리자 소켓, TLS. 테스트 512개, 양쪽 DB에서 통과. 영역을 병렬로 만들어 공유 파일이 겹치므로 커밋은 하나다 |
| S5 | 차등 비교 382단계·권한 표 636칸·DB 왕복(SQLite, PostgreSQL)·Cypress 11/11 통과. Dockerfile, CI 워크플로, CDN 문서, `NOTICE`, `deny.toml` |

검증 명령과 결과, 미검증 항목은 `crates/server/PORTING.md`의 "검증" 절에 있다.

## S6: 서버 정리 (2026-11-10~)

계획은 저장소 밖 `~/.claude/plans/fvoci-web-svelte.md`의 "S6. 서버 정리"다. `parity-s5` 뒤로는 원본 서버·원본 웹 번들과의 호환을 지키지 않는다(사용자 결정 2026-11-10). 그래서 이 문서와 크레이트 문서에 적힌 "와이어 동작이 바뀌는 수정은 하지 않고 결정 대기에 올린다"는 규칙은 끝났고, S6부터는 **고치고 기록한다.** 사용자가 보는 기능이 달라지거나 설계 선택이 필요한 것만 먼저 안을 내고 확인받는다. "의도적 차이", "보안 결정 대기", "보존한 quirk"의 항목은 지우지 않고 "S6에서 정리됨"으로 표시한다.

### A. 기준 고정

원본이라는 외부 기준이 없어지므로, 서버를 고치기 전에 `parity-s5` 시점의 응답을 회귀 테스트의 기대값으로 고정했다.

- **무엇을 옮겼나.** `scripts/differential/scenarios.py`의 시나리오 15개와 `permissions.py`의 권한 표를 `crates/server/tests/regression/`의 통합 테스트로 옮겼다. 정규화 규칙(식별자는 처음 나온 순서의 `<id:N>`, 밀리초 시각은 `<ts>`, 비밀 값은 `<secret>`, 객체 목록은 식별자를 가린 표현으로 정렬)도 같은 규칙으로 옮겼다. 시나리오마다 테스트 하나, 권한 표는 보드 종류마다 테스트 하나이고, 테스트마다 서버를 따로 띄운다.
- **기대값.** HEAD가 `parity-s5`와 서버 코드가 같을 때(차이는 문서 한 개) 기록했다. SQLite에서 기록한 스냅샷이 PostgreSQL에서 그대로 통과한다.
- **옮기기 전 대조(2026-11-10).** 같은 빌드의 Rust 서버에 기존 Python 하네스로 시나리오와 권한 표를 돌려 얻은 기록과 새 스냅샷을 단계·칸 단위로 비교했다.

  | 대상 | Python 기록 | 새 스냅샷 | 차이 |
  |---|---|---|---|
  | 시나리오 15개 | 382단계 | 382단계 | 1건: `system`의 "ping version and sku"에서 DB 종류 `sqlite3`을 `<db>`로 적는다(두 DB가 한 스냅샷을 쓰게 하려는 의도한 차이. 값이 `sqlite3`·`postgres` 중 하나인지는 테스트가 따로 확인한다) |
  | 권한 표 | 636칸 | 636칸 (비공개 318, 공개 318) | 0건 |

  대조 직전에 원본 대 포트 비교도 다시 돌려 S5와 같은 결과임을 확인했다(382단계 예상 밖 0건·등록 21건, 636칸 예상 밖 0건·등록 11건). 스냅샷을 일부러 고쳐(본문 한 글자, 상태 코드 하나) 테스트가 실패하고 어느 단계의 무엇이 다른지 보고하는 것도 확인했다.
- **갱신 방법은 하나다.** `FVOCI_REGRESSION_UPDATE=1 cargo test -p fvoci-server --test regression`이 스냅샷을 다시 쓴다. 응답을 의도해서 바꾸는 커밋은 바뀐 스냅샷을 함께 담는다. 리뷰어는 그 `git diff`에서 응답이 어떻게 달라졌는지 본다.
- **스냅샷 라이브러리를 쓰지 않고 직접 비교한다.** 이유: (1) 손이 가는 부분은 비교가 아니라 정규화이고, 이 API에 맞춘 규칙(단계에 걸친 식별자 번호 매기기, 순서 없는 목록의 정렬)은 라이브러리가 주지 않아 어차피 직접 써야 한다. (2) 스냅샷이 보기 좋은 JSON과 텍스트 표라서 "바뀐 응답은 같은 커밋의 diff로 본다"는 흐름에 맞고, 대기 파일(`.snap.new`)이나 리뷰용 CLI를 따로 설치할 필요가 없다. (3) 실패 보고(단계 이름, 요청, JSON 경로별 차이)는 Python 하네스에 있던 것을 옮긴 100줄 남짓이다. (4) 의존성이 늘지 않는다. 잃는 것: 대화형 리뷰(`cargo insta review` 같은 것)와 인라인 스냅샷. 필요해지면 그때 바꾼다.
- **조율은 xtask로 한다.** `cargo xtask check`와 `cargo xtask test`(`xtask/`, 별칭은 `.cargo/config.toml`). 명령 구성은 형제 저장소 `fvoci/groundzero`의 xtask에서 뼈대만 참고했다(별칭, 외부 서비스가 없는 `check`와 컨테이너를 준비하는 `test`의 구분, 모든 단계를 끝까지 돌리고 요약). 코드는 복사하지 않았고, 의존성은 이미 쓰던 `anyhow`와 `clap`뿐이다. CI 워크플로(`.github/workflows/fvoci-ci.yml`)도 같은 명령을 부르게 바꿨다(푸시가 필요해 실행하지 못했다).
- **이름 검사가 찾은 것.** `cargo xtask check`의 이름 검사는 `crates/`와 `xtask/`의 코드에서 원본의 두 이름이 0건임을 확인한다. 스냅샷에는 1건이 있다(`golden/templates.json`의 온보딩 보드 설명). 내장 템플릿(`server/assets/templates.boardarchive`, 원본의 것을 그대로 바이너리에 넣는다)의 본문과 링크에 원본 이름이 37번 나오기 때문이다. 검사는 이것을 실패가 아니라 주석으로 보고한다. 템플릿을 새로 쓸 때 없어진다(`docs/s6-design.md`).

검증(2026-11-10, rustc 1.99.1):

| 검증 | 결과 |
|---|---|
| `cargo xtask check` | format, clippy, unit tests, documentation, names, licenses and advisories 모두 PASS |
| `cargo xtask test` — SQLite | 741개 통과(모델 69, 스토어 121, 서버 537: 단위 78·API 423·회귀 25·저장소 2·웹 7·웹훅 2, xtask 14) |
| `cargo xtask test` — PostgreSQL | 658개 통과(스토어 121, 서버 537) |
| `cargo xtask test` — S3 | 1개 통과(SeaweedFS) |
| Docker 이미지 | `docker build -f docker/fvoci/Dockerfile`이 워크스페이스에 `xtask`가 추가된 뒤에도 `--locked`로 빌드된다(빌드 캐시가 있는 상태에서 확인) |
| 컨테이너 준비 경로 | 다른 이름과 포트의 임시 컨테이너로 "없음 → 생성", "멈춤 → 시작"을 확인하고 지웠다. 기존 `hafgufa-pg`·`hafgufa-s3`는 "떠 있음 → 그대로 사용" |

걷어 낸 것(사용자 확인 뒤, 2026-11-10 17:44 KST, 별도 커밋): `scripts/differential/`, `scripts/oracle-run.sh`, `scripts/fvoci-run.sh`, `scripts/e2e-cypress.sh`. 태그 `parity-s5`에 남아 있다. `target/oracle/`(오라클 바이너리와 Go 빌드 캐시)과 개발 서버는 그대로 두었다.

### B. 설계 안

`docs/s6-design.md`. 오류 응답 규칙, 경로 규칙과 끝 슬래시, 새 기준 스키마, 크기 한도의 네 가지를 선택지·장단점·권고와 함께 냈다. **확인을 기다리며, 구현한 것은 없다.** 보안 결정 대기의 S1·S2·S5·S6·S8·S9·S11·S17과 "보존한 quirk"의 일괄 정리가 이 문서의 결정에 달려 있다.

### C. 확인이 필요 없는 수정

항목마다 고치기 전의 동작을 테스트로 먼저 확인했고(이미 그 동작을 고정한 테스트가 있으면 그것으로, 없으면 새로 써서 수정 전 코드에서 통과시킨 뒤), 허용되는 경우와 거절되는 경우를 모두 테스트에 넣었다. 자세한 것은 `crates/server/PORTING.md`의 "보안 결정 대기" 표에 항목별로 있다.

| 항목 | 바뀐 것 | 응답 스냅샷 |
|---|---|---|
| S3 | 관리자가 비밀번호를 재설정하면 그 사용자의 세션을 모두 지우고 WebSocket 연결을 닫는다 | 변화 없음 |
| S4 | `usessl`인데 인증서나 키 파일이 없으면 시작을 거부한다 | 변화 없음 |
| S12 | 멤버가 보드에서 빠지면 그 보드의 블록에 건 구독을 끝낸다 | 변화 없음 |
| S14 | 카드 속성 정의와 값의 모양을 쓰기에서 검증한다. 잘못된 모양은 400 | 변화 없음(시나리오의 속성 데이터는 모두 규칙에 맞는다) |
| S15 | 역할 관리 권한이 없는 추가자는 보드의 최소 역할만 준다. 빈 값이면 viewer | 변화 없음 |
| S16 | 파일 소유 규칙을 모든 저장 배치에 적용한다. 보드를 말하지 않는 경로의 파일은 주지 않는다 | 변화 없음 |
| S18 | 온보딩 보드를 제목이 아니라 표식(시스템이 만든 보드 + 표식 속성)으로 찾는다 | 변화 없음 |
| 결정 1 | 설정 카테고리 값 `fvoci` 확정. 닫음 | — |
| 결정 4 | RUSTSEC-2026-0459 조사. 고치지 않음(위 "결정 4의 조사") | — |

스냅샷이 하나도 바뀌지 않은 것은 우연이 아니라 범위 때문이다. 일곱 항목 모두 `parity-s5`의 시나리오가 지나지 않는 경로(관리자 소켓, TLS 시작, 멤버 제거 뒤의 구독, 잘못된 모양의 속성, 공개 보드에서 editor가 고른 역할, 템플릿 배치의 파일, 사용자가 만든 같은 제목의 템플릿)를 바꾼다. 응답 형식을 바꾸는 일(오류 규칙, 404 범위)은 B의 승인 뒤에 한다.

검증(2026-11-10, C의 마지막 커밋에서. rustc 1.99.1):

| 검증 | 결과 |
|---|---|
| `cargo xtask check` | format, clippy, unit tests, documentation, names, licenses and advisories 모두 PASS |
| `cargo xtask test` — SQLite | 781개 통과(모델 75, 스토어 125, 서버 567: 단위 84·API 447·회귀 25·저장소 2·웹 7·웹훅 2, xtask 14) |
| `cargo xtask test` — PostgreSQL | 692개 통과(스토어 125, 서버 567) |
| `cargo xtask test` — S3 | 1개 통과(SeaweedFS) |
| 커밋마다 | A부터 S14까지 커밋 9개를 하나씩 체크아웃해 `cargo fmt --all --check`와 `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`가 통과하는 것을 확인했다. 테스트는 항목마다 해당 범위를 두 DB에서 돌렸고, 전체는 마지막 커밋에서 돌렸다 |

하지 않은 것: 이 변경들에 대한 별도의 `rust-review`는 돌리지 않았다(작성하면서 규칙을 따랐고, C를 마친 뒤 변경분을 다시 읽어 최소 역할이 admin인 보드의 경우를 테스트로 확인했다). Docker 이미지는 A에서만 다시 빌드했다. CI 워크플로는 푸시하지 않아 실행하지 못했다.

S6에서 새로 본 것:

- **S16.** 보드를 "템플릿으로 복제"해 생긴 파일은 템플릿 배치에 기록과 함께 저장되어, 그 템플릿을 볼 수 없는 사용자도 파일 이름을 알면 자기 보드 경로로 받을 수 있었다(포트에서 재현). 이제 막혔다.
- **S18.** 내장 템플릿이 없는 서버에서는 한 사용자가 만든 같은 제목의 공개 템플릿이 다른 사용자의 온보딩 보드로 복사됐다(포트에서 재현). S5의 기록은 "재현 안 됨"이었다.
- **내장 템플릿의 원본 이름.** 위 A의 "이름 검사가 찾은 것".

## 플러그인 저장소의 수정 사항 점검

원본에서 갈라진 플러그인 저장소가 분리 뒤에 넣은 보안·검증 수정 15건(서버 쪽)을 원본 standalone 경로에 대해 하나씩 확인했다. 원본에 같은 결함이 있는지는 오라클에서 재현해 판단했고, 처리 결과는 `crates/server/PORTING.md`의 보안 수정 표, 의도적 차이, 보안 결정 대기 표에 PR 번호와 함께 적었다.

| PR | 결과 |
|---|---|
| #114, #179, #247, #268 (파일 소유) | 부분 수정. 새로 저장하는 파일은 보드에 묶고 다른 보드를 통한 읽기·복사·내보내기를 거절. `sourceBoardID`의 권한 검사 추가. 원본 배치 파일과 쓰기 검증은 결정 대기 S16. **S6에서 정리됨:** 소유 규칙을 모든 배치에 적용(쓰기 검증은 넣지 않음. 서버 문서 S16) |
| #135 (댓글 삭제) | 수정(사용자 결정) |
| #162 (카드 단건 조회) | 수정(사용자 결정, 의도적 차이 45) |
| #207 (멤버 역할) | admin 부여는 수정. 최소 역할 강제는 원본 웹 앱의 결과와 달라 결정 대기 S15. **S6에서 정리됨:** 최소 역할을 준다. 빈 값이면 viewer(플러그인은 editor) |
| #209 (`channelId`) | 이미 고친 묶음 라우트 검사와 같다 |
| #243, #216 (속성 검증) | 읽기 쪽만 수정(의도적 차이 46). 쓰기 검증은 결정 대기 S14. **S6에서 정리됨:** 쓰기 검증을 넣음 |
| #220 (가져오기 크기) | 부분 반영(항목당 한도, `board.jsonl` 70MB). 나머지는 결정 대기 S17 |
| #115, #219, #211 | 해당 없음(사유는 서버 문서와 점검 목록에) |
| #238 | 원본 `a84bbb65`에는 없는 버그. 포트가 달랐던 것을 고쳤다 |
| #276 | 채택하지 않음(v40 호환 유지) |

## 작성 모델

주 세션과 하위 에이전트는 Claude Fable 5.2로 돌았다. 예외는 파일·아카이브·템플릿·온보딩 영역을 맡은 하위 에이전트 하나로, 작업 도중(안전 분류기가 응답을 한 번 중단시킨 뒤) Claude Opus 4.8로 바뀌어 끝까지 갔다. 그 에이전트가 쓴 파일은 다음과 같다.

- `crates/server/src/app/{files,export,import,templates,onboarding}.rs`
- `crates/server/src/api/{files,archive,templates,onboarding}.rs`
- `crates/server/tests/api/{files,export,templates,onboarding}.rs`

이 영역은 병합 뒤 주 세션이 직접 읽고 고쳤고(파일의 보드 묶기, 복사 실패 처리, 내보내기의 중복 항목, 가져오기의 블로킹 작업과 한도), Fable 5.2로 `rust-review`를 따로 돌려 Must 2건과 Should 8건을 반영했다(`crates/server/PORTING.md` "리뷰"). 차등 비교의 `files`·`archive`·`templates` 시나리오와 권한 표의 파일·아카이브 칸, DB 왕복의 파일 관찰이 이 영역을 덮는다.


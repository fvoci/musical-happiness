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

S6부터의 진입점은 `cargo xtask check`와 `cargo xtask test`다(아래 "S6" 절, 명령의 세부는 `docs/handoff.md` 1절). 표에서 "레거시"라고 적은 것은 태그 `parity-s5`까지 원본과 비교하는 데 쓴 도구로, 이후의 변경에는 쓰지 않는다. **`scripts/` 아래의 것은 주 브랜치에서 걷어 냈고(2026-11-10 17:44 KST) 태그 `parity-s5`에 남아 있다.** 꺼내 쓰는 방법은 `docs/handoff-s5.md` 1절에 있다.

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
| 3 | **보안 결정 대기 항목.** 고치면 정상 클라이언트의 동작이나 와이어 형식이 바뀌는 보안 관련 항목은 `crates/server/PORTING.md`의 "보안 결정 대기" 표(S1~)에 모았다 | S5까지는 전부 원본대로였다. **S6에서 정리됨:** S3, S4, S12, S14, S15, S16, S18을 고쳤다(아래 "S6" 절의 C). 남은 S1·S2(관리자), S5·S17(크기 한도), S6·S9·S11(404 범위), S8(공유 토큰)은 설계 안을 냈고 확인을 기다린다(`docs/s6-design.md`). **S6에서 정리됨(2026-11-10 18:38 KST):** 설계가 결정됐고(아래 D), S6·S9·S11과 S5의 JSON 본문 부분, S12의 남은 부분, S17의 오류 응답 부분을 고쳤다. 남은 것은 S1·S2·S8(새 기준 스키마 단계)과 S5·S17의 파일 부분(설계 5.6의 확인 뒤) | S3–S4, S6 |
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

### D. 결정 반영과 구현 1단계 (2026-11-10 18:38 KST)

`docs/s6-design.md`의 결정 표에 결정을 적었다(2026-11-10 17:42 KST). 승인된 구현 순서 가운데 1단계(오류 본문과 상태 코드, 경로 규칙, 보존한 quirk 정리)와 2단계의 파일과 무관한 한도(JSON 본문, 세션 없는 라우트, WebSocket 메시지)를 구현했다. 2단계의 나머지(업로드·가져오기·내보내기의 스트리밍과 파일 한도), 3단계(새 기준 스키마) 이후는 보완 안(설계 5절)의 확인 뒤에 한다.

**무엇이 왜 바뀌었나.**

| 바뀐 것 | 왜 | 어디 |
|---|---|---|
| 오류 본문이 `{"error": 코드, "message", "reason"?, "field"?, "limit"?, "request_id"}`다. 전에는 `{"error": 문구, "errorCode": 상태}` | 화면이 문구가 아니라 코드로 분기하게 하고, 사용자가 본 오류를 로그의 한 줄과 잇는다(설계 1.2). 응답 헤더 `x-request-id`가 같은 값을 싣고 요청 로그에도 남는다 | `fvoci_model::ErrorBody`, `server::error`, `request_log` |
| 상태 코드가 한 표를 따른다: 400·401·403·404·405·409·413·500. 501과 "분류되지 않아 500"이 없어졌다 | 같은 종류의 잘못이 라우트마다 다른 코드를 받고 있었다(설계 1.3) | `ErrorKind`, 핸들러 전반 |
| 볼 수 없는 것은 없는 것과 같은 404, 볼 수는 있는데 못 하는 것은 403 | 403과 404의 차이로 남의 보드·블록·카테고리·구독·파일의 존재를 알 수 있었다(결정 대기 S6·S9·S11, 설계 1.4의 규칙 B) | `api::access`, `App::user_category`, 테스트 `visibility.rs` |
| 검사 순서가 세션(401) → 볼 수 있나(404) → 권한(403) → 본문(400) → 충돌(409)이다 | 권한 없는 사용자가 본문을 바꿔 가며 검증 문구를 읽을 수 없게 한다. 예외는 필요한 권한이 본문에 달린 블록 삽입과 대상이 본문에 있는 boards-and-blocks다 | 핸들러 전반 |
| 채널, 통계, 컴플라이언스, 보드 메타데이터, linkable 검색 라우트를 없앴다. `subscriberType: "channel"`과 `/users/me?channelID`도 없다 | 원본 웹앱이 불러서 거절만 하던 라우트다(결정 4). 채널 열과 `channelId` 멤버는 새 기준 스키마 단계에서 뺀다 | `api::{channels,statistics,compliance}` 삭제 |
| 경로 규칙: API 접두어 아래의 모르는 경로와 끝 슬래시는 JSON 404, 아는 경로의 다른 메서드는 JSON 405와 `Allow`, 화면 경로의 끝 슬래시는 308, 화면 경로의 `GET`·`HEAD` 밖의 메서드는 405, 기준 경로는 슬래시가 있든 없든 화면 | 전에는 모르는 경로와 틀린 메서드가 모두 200과 `index.html`이라 API 클라이언트가 HTML을 JSON으로 읽다 실패했다(설계 2.3) | `lib.rs`의 라우터, 테스트 `paths.rs` |
| 요청 크기 한도: JSON 4 MiB(`max_request_bytes`), 세션 없는 로그인·가입 64 KiB, WebSocket 메시지 64 KiB. 넘으면 413과 `limit` | 로그인한 사용자가, 로그인·가입은 누구나, 서버 메모리를 원하는 만큼 쓰게 할 수 있었다(결정 대기 S5, 설계 4.2) | `api::limits`, 테스트 `limits.rs` |
| 보존한 quirk 37개 | 위 규칙에 걸린 것들이다. 항목별 지금의 동작은 `crates/server/PORTING.md`의 "보존한 quirk" | — |

**1단계에 넣지 않은 것.** 성공 응답의 201·204(설계 1.5)와 경로 이름(2.4)은 구현 순서대로 OpenAPI를 낼 때 한다. `channelId` 멤버와 열, UUID 형식이 아닌 ID의 404(2.3의 마지막 행), `GET /teams/{아무 ID}`와 가입 토큰(S1·S2), 공유 토큰(S8)은 새 기준 스키마 단계다. 사용자 행이 없는 세션이 받는 404는 그대로이고, 세션이 사용자를 외래 키로 가리키게 되면 없어진다.

**규칙을 적용하다 고친 것(요청이 옳은데 거절되던 것).** 블록 없는 보드의 복제와 그런 보드의 아카이브 가져오기(전에는 500, 규칙만 적용하면 400이 되는데 요청에는 잘못이 없다), 묶음 패치의 브로드캐스트(팀 `""`로 나가 아무도 받지 못했다), single-user 모드의 `POST /users`·`GET /users/single-user`.

**구현하다 본 것.** 413을 헤더만 보고 바로 답하면 클라이언트가 본문을 쓰는 중에 연결이 닫혀 답을 못 읽는 일이 있었다(4 MiB+1바이트 요청 9번 중 1번이 broken pipe). 거절한 본문을 8 MiB·5초까지 읽어 버린 뒤 답하게 해서, 같은 테스트를 6번(요청 54번) 돌려 모두 413을 받았다. axum의 추출기 한도(2 MB)는 이 한도들 아래에서 자기 형식의 413을 내므로 껐다.

**응답 스냅샷.** 15개 시나리오 382단계 가운데 188단계가 바뀌었다(단계 수와 권한 표의 636칸은 그대로다). 상태 코드가 바뀐 것은 76단계이고 나머지 112단계는 본문 형식만 바뀌었다(처음에 77과 111로 적었던 것을 2026-11-10 19:28 KST에 다시 세어 고쳤다. 아래 표의 합이 76이다). 권한 표는 636칸 가운데 61칸이 바뀌었다(비공개 50, 공개 11).

| 전 → 뒤 | 단계 | 무엇 |
|---|---|---|
| 403 → 404 | 31 | 비공개 보드의 비멤버, 없는 보드, 남의 카테고리·구독·설정 |
| 500 → 400 | 12 | 잘못된 본문, 개수가 안 맞는 재정렬, 아카이브가 아닌 가져오기 |
| 501 → 404, 401 → 404 | 5, 3 | 없앤 라우트(시나리오와 권한 표의 행은 남겨 그 경로가 404임을 고정한다) |
| 400 → 409 | 5 | 이미 있는 사용자 이름·이메일, 마지막 관리자 |
| 200 → 404 | 4 | 남의 카테고리 숨기기, 지운 카테고리 다시 지우기, 없는 파일의 정보, 모르는 API 경로 |
| 400 → 404 | 3 | 없는 카드(조회·패치), 남의 카테고리 수정 |
| 500 → 404 | 3 | 없는 파일 다운로드(경로 탈출 시도 포함), 없는 카테고리에 보드 넣기 |
| 400 → 403 | 3 | 남을 위한 카테고리·구독, 헤더 없는 요청(`reason: csrf`) |
| 401 → 403 | 2 | 가입 토큰 없음·틀림 |
| 200 → 400 | 2 | 없는 사용자를 멤버로 추가, 가져오기의 틀린 필드 |
| 500 → 403, 200 → 405, 404 → 200 | 1, 1, 1 | 시스템 카테고리 삭제, 아는 경로의 틀린 메서드, 공유 기록이 없는 보드의 공유 조회 |

권한 표는 비공개 보드의 outsider 열이 403에서 404로 바뀌었고(공개 보드에서는 ID만 받는 라우트인 카드 조회·패치, 콘텐츠 이동, 구독 생성과 지워진 보드의 undelete만), `board: metadata` 행은 모든 칸이 404다.

### E. 보완 안 (2026-11-10 19:02 KST)

`docs/s6-design.md` 5절에 보완을 선택지·장단점·권고와 함께 냈다(처음 일곱, 뒤에 하나 더): 쿠키 인증(5.1), 로그인 시도 제한(5.2), 삭제와 복원의 구조(5.3), 세션 수명(5.4), OpenAPI의 순서와 API의 판 번호(5.5), 청크 업로드(5.6), 본문 저장의 확정과 날짜 값(5.7). **확인을 기다리며 구현한 것은 없다.** 5.7은 웹앱 트랙의 편집기 스파이크(`fvoci/web` `fa7625b1`)를 받아 3.7을 확정안으로 바꾸고 3.10의 템플릿 변환을 "웹앱 쪽 변환기의 산출물을 받아 쓴다"로 고쳤다. 구현 순서도 고쳤다(OpenAPI를 새 기준 스키마 바로 뒤로).

**추가(2026-11-10 19:24 KST).** 보완 h를 더했다: 사용자 표시 설정(시간대, 주의 시작 요일, 12·24시간 표기)을 계정에 두는 안(5.8. 설정과 화면 상태를 나누고 설정은 알려진 키만 검증, 시간대 이름은 바이너리에 넣은 시간대 DB로 검증). 5.7의 날짜 값은 받은 방향에 맞춰 고쳤다: 날짜만 있는 값은 달력 날짜 문자열, 시각은 UTC 밀리초 정수 그대로, 시간대는 값이 아니라 보는 사람의 설정에서 온다. 웹앱 트랙이 적은 안은 시각을 RFC 3339 문자열로 적으므로 그 차이를 웹앱 세션에 전해야 한다. "확인하지 못한 것"은 5.9로 옮겼다.

5.7을 쓰며 확인한 것: 내장 템플릿 아카이브에는 주인 없는 콘텐츠 블록 38개와 끊어진 `contentOrder` 참조 42개가 있다(아카이브에서 직접 셌고 스파이크 문서의 수와 같다). 온보딩 보드의 카드 10장은 `contentOrder` 65개 가운데 33개가 없는 블록을 가리킨다. 서버는 저장된 그대로 주고 원본 웹앱은 없는 ID를 건너뛰어 그리므로, `parity-s5`의 화면에는 오류 없이 있는 블록 32개만 보였다(회귀 스냅샷의 "welcome template block counts"가 그 32개를 종류별로 고정한다).

### F. 리뷰 (rust-review, 2026-11-10 19:19 KST)

`beautiful-rust:rust-review`를 하위 에이전트 둘에 맡겼다. 둘 다 Fable 5.2로 지정했고 다른 모델로 넘어간 부분은 없다(각자 보고 끝에 모델과 스킬을 읽었음을 적었다). 읽기 전용이었고 테스트는 돌리지 않았다. 하나는 D의 두 커밋(`9e920dcb`, `37ca5527`)을, 다른 하나는 아직 리뷰하지 않았던 A(`fe4732d3`)와 C(일곱 커밋)를 지금의 코드로 봤다.

**D(1단계와 한도)의 리뷰: Must fix 2, Should fix 10, Consider 16.**

| 지적 | 처리 |
|---|---|
| Must: 카테고리를 만들 때 본문의 `id`가 누군가의 카테고리와 겹치면 409, 아니면 200이라 남의 카테고리 ID의 존재를 알 수 있었다(이 단계에서 내가 넣은 409가 만든 구멍이다) | 고침. 새 카테고리는 ID를 정할 수 없다(400, `field: id`). 겹치는지 보는 조회를 없앴다. `reorder`라는 ID의 카테고리·보드를 가리키던 두 라우트도 없앴다(그 경로의 다른 메서드는 405) |
| Must: 세션 조회에서 저장소가 실패하면 세션이 선택인 라우트(보드·블록·파일 읽기)가 401을 답해 클라이언트가 멀쩡한 세션을 버리게 된다 | 고침. "세션 없음"만 없는 것으로 보고 저장소 실패는 500이다 |
| 끝 슬래시 리다이렉트가 역슬래시를 거르지 않는다(`/\host/` → 브라우저가 `//host`로 읽는다) | 고침, 테스트 추가 |
| 한도가 세션 검사보다 앞이라 세션 없는 요청도 4 MiB를 받는다 | 고침(작은 쪽): 세션 토큰을 싣지 않은 요청은 어느 라우트에서나 64 KiB다. **더 고침(2026-11-10 19:41 KST):** 토큰이 있고 본문이 64 KiB를 넘으면(또는 길이를 모르면) 미들웨어가 세션을 먼저 확인한다. 없는 세션이면 본문을 읽지 않고 그 라우트가 답할 401을 답한다(`limits::a_made_up_session_token_does_not_get_the_larger_limit`). 작은 요청은 전처럼 라우트가 확인하므로 세션 조회가 두 번 되는 것은 큰 본문뿐이다. **남은 것:** 세션이 있는 사용자가 본문을 읽지 않는 라우트에 본문을 보내면 한도까지는 읽는다. 권한 검사는 본문을 읽은 뒤다. 둘 다 로그인한 사용자가 한도 안에서 하는 일이라 한도가 막는 범위 안이다 |
| 거절한 본문을 읽어 버리는 것이 `Expect: 100-continue`로 기다리는 클라이언트에게 "보내라"가 된다 | 고침. 그 헤더가 있으면 읽지 않고 바로 413 |
| 레코드 본문을 배열로 보내도 통과했다(파생된 디코더가 배열을 순서대로 읽는다. `[]`가 빈 패치, 배열 로그인) | 고침. 레코드 본문은 모두 객체여야 한다 |
| 앱 계층이 붙인 문맥(`moveBoardsToDefaultCategory: …`, `cannot patch card …`)이 4xx의 문구로 나간다 | 고침. 문맥은 로그로만 간다 |
| 멤버십·보드 조회의 저장소 실패가 "권한 없음"으로 읽혀 모든 보드 라우트가 404를 답한다 | 고침. 조회가 `Result`를 돌려주고 실패는 500이다. 멤버 추가의 사용자 조회 실패도 전파한다 |
| 같은 오류를 두 번 기록 | 고침 |
| 낡은 문서 주석 9곳, 없는 한도를 말하는 주석 2곳 | 고침. 업로드와 가져오기에 한도를 새로 넣지는 않았다(5.6의 확인 뒤) |
| `visibility` 테스트에 "자기 것 + 숨겨진 것"의 경우가 없다 | 추가. 18개 요청(자기 보드 경로의 남의 블록, boards-and-blocks, 콘텐츠 이동 양방향, `sourceBoardID`, 자기 카테고리의 남의 보드, 남의 ID로 카테고리 만들기, 자기 보드 경로의 남의 파일). 이 과정에서 boards-and-blocks의 거절 문구가 요청의 블록 ID를 되풀이하던 것을 뺐다 |
| Consider 가운데 고친 것 | 복제의 검사 순서(볼 수 있는지가 대상 팀보다 먼저), 기준 경로 검증(`/:x`가 라우터를 패닉시켰다), `/ws`의 핸드셰이크 아닌 요청을 API 형식으로, 관리 소켓에도 본문 한도, 한도 미들웨어의 버퍼 크기와 오류 문구 |
| Consider 가운데 남긴 것 | 본문 검증(400)이 권한(403)보다 앞인 곳(블록 삽입은 문서에 적은 예외, 나머지는 팀 검사가 실패할 수 없어 닿지 않는다). 팀 검사가 항상 참인 것과 `GET /teams/{아무 ID}`(새 스키마 단계). `decode_object`가 `Value`를 거치는 것(한도 4 MiB 안이다). 오류 타입의 `Display`와 `source`의 겹침, `From` 안의 로그, 저장소의 not-found를 한 줄로 바꾸는 도우미, 겹치는 조회와 복사(동작에 영향이 없다). 원본 순서를 흉내 내는 검증과 채널 ID 검사, 파일 정보의 채움 멤버(와이어가 바뀌는 새 스키마 단계에서 걷는다) |

**A와 C의 리뷰: Must fix 2, Should fix 15, Consider 23.**

| 지적 | 처리 |
|---|---|
| Must: S18이 표식을 흉내 내지 않고도 뚫린다. 내장 템플릿은 누구나 속한 팀의 공개 보드라 아무나 join해 editor가 되고, 모든 새 사용자가 복사받는 온보딩 보드를 고칠 수 있었다(원본도 그렇다) | 고침. 템플릿은 join할 수 없다(403). 내장 템플릿에 join·수정·삭제·블록 패치·멤버 추가가 모두 거절되고 읽기와 복제는 되는 것을 테스트로 고정했다(`templates::a_built_in_template_cannot_be_joined_or_changed`) |
| Must: `FVOCI_REGRESSION_UPDATE`가 비어 있지만 않으면(`0`, `false`도) 스냅샷을 다시 쓰고 통과한다. `cargo xtask test`가 그 변수를 물려받는다 | 고침. 정확히 `1`일 때만 기록하고 다른 값은 오류다. 기록한 파일마다 한 줄(`CHANGED`/`unchanged`/`NEW`)을 낸다. `CI`에서는 기록을 거부한다. `cargo xtask test`는 변수가 있으면 시작하지 않는다 |
| 권한 표의 준비 요청이 실패해도 칸이 기록된다 | 고침. 준비 요청은 성공해야 한다(멤버가 아닌 호출자의 구독 준비는 404여야 한다) |
| xtask: docker 실패의 이유가 사라진다, 반쯤 만들어진 컨테이너, 연결에 제한 시간이 없다, cargo-deny의 실패가 "설치하라"로 보인다, 건너뛴 부분이 요약에 없다 | 고침. stderr를 싣고 "없는 컨테이너"만 없는 것으로 본다. 포트 탐침에 2초 제한. 설치 안내는 도구가 없을 때만. 요약에 `SKIP` 줄 |
| xtask: 같은 이름의 아무 컨테이너나 쓴다 | 일부 고침. 컨테이너가 기대한 포트에 게시하는지 확인한다. 라벨 검사는 넣지 않았다(이미 있는 `hafgufa-pg`·`hafgufa-s3`에는 라벨이 없고, 건드리지 않기로 한 컨테이너다) |
| S3: 재설정 전에 옛 비밀번호를 읽은 로그인이 재설정 뒤에 세션을 만든다. WebSocket 인증도 같은 틈이 있다 | 고침. 로그인은 세션을 만든 뒤 비밀번호가 그대로인지 다시 보고 아니면 세션을 지운다. WebSocket은 사용자를 연결에 붙인 뒤 세션을 다시 확인한다 |
| S14: 같은 `id`의 속성 정의 둘이 통과한다. 패치가 한 ID를 지우고 다시 넣으면 스스로 중복을 만든다 | 고침. 중복 ID는 400이고, 패치의 병합은 정의를 한 번만 낸다. `id` 없는 정의의 오류가 몇 번째인지 말한다 |
| 회귀 하네스가 객체의 목록을 모두 정렬해 순서의 오류를 가린다 | **남김, 이유를 실험으로 확인(2026-11-10 19:41 KST).** 정렬을 끄고 SQLite에서 기록한 뒤 다시 비교했다: SQLite에서도 실행마다 달라졌고(두 번 돌려 `members`와 `search`, `members`와 `archive`가 실패) PostgreSQL에서는 `boards`·`search`·`cards`가 실패했다(보드 목록, 보드 검색, 카드 목록의 순서가 다르다). 이 라우트들에는 정해진 순서가 없다. 정렬을 빼면 스냅샷이 흔들리므로, 라우트마다 순서를 계약으로 정한 뒤(OpenAPI 단계)에 뺀다. **새로 안 것:** 멤버 목록, 보드 목록, 보드 검색, 카드 목록의 순서가 DB와 실행에 따라 다르다. 웹앱이 서버의 순서에 기대면 안 된다 |
| S15: 최소 역할이 빈 보드에서 추가는 viewer, join은 editor라서 editor가 남을 먼저 추가해 viewer로 묶어 둘 수 있다 | **남김.** 추가의 viewer는 사용자 결정이고 join의 editor는 원본 동작이다. 묶인 사용자는 나갔다가 다시 join하면 editor가 된다. 둘을 맞출지는 사용자 판단이라 보고에 올린다 |
| 멤버 추가의 확인 후 쓰기, 마지막 관리자 확인과 삭제가 따로인 것(동시 요청의 경쟁) | **남김.** 고치려면 행 잠금이 필요한데 SQLite와 PostgreSQL에서 방법이 다르고, 멤버십 쓰기는 새 스키마 단계(설계 5.3)에서 다시 짠다. 알려진 경쟁으로 적어 둔다: 두 관리자가 동시에 나가면 관리자 없는 보드가 남을 수 있다 |
| Consider 가운데 남긴 것 | 타임스탬프를 모두 `<ts>`로 가리는 것, ID의 종류 접두사, 바이너리 본문을 길이로만 고정하는 것(하네스의 정밀도를 올리는 일이다). 실패한 시나리오의 teardown. S3 테스트가 0건이어도 통과하는 것, SeaweedFS 이미지의 태그 고정(지금 컨테이너를 다시 받게 된다), 이름 검사가 기록된 응답의 옛 이름을 실패로 보지 않는 것. 기록이 빈 경로인 파일의 정보, 복사에 실패한 파일의 기록. 자기 비밀번호를 바꿀 때 다른 세션을 끊는 것은 설계 5.4의 안이다 |

### G. 구현 3단계: 새 기준 스키마 (2026-11-10 19:59 KST~)

`docs/s6-design.md` 5절의 보완 a–h가 모두 권고대로 승인됐고 추가 결정 다섯(같은 문서 5.10)을 받았다. 설계 문서의 "구현 순서" 3단계를 아래 순서로 구현한다. 커밋 하나가 줄 하나다. 커밋마다 `cargo xtask check`와 `cargo xtask test`(SQLite, PostgreSQL, S3)를 통과시켰다.

| 커밋 | 무엇 | 응답 스냅샷 |
|---|---|---|
| `408faa63` | **마이그레이션 체계(3.9).** 스키마를 sqlx의 migrator가 적용한다(`crates/store/schema/{sqlite,postgres}/0001_baseline.sql`, 기록은 `_sqlx_migrations`). 테이블 접두사를 없앴다. 원본 서버의 DB(SQLite의 `db_migrations`, PostgreSQL의 `…schema_migrations`), 기록 없는 테이블, 더 새 서버가 올린 DB, 고쳐 쓴 이력을 이유와 함께 거절한다. `dbtableprefix`는 설정이 아니다(값이 있으면 시작 오류) | 변화 없음 |
| `297a452b` | **single-user 모드 제거(5.1의 6번 (a)).** 플래그, 토큰 변수, 가상 사용자와 그 특례(세션, 사용자, 팀, 공유, 가져오기, WebSocket)를 걷어 냈다. 가상 사용자의 고정된 이름은 새 스키마의 식별자가 될 수 없어서, 사용자가 정한 순서(인증 전달 단계)보다 앞당겼다 | 변화 없음(시나리오는 이 모드를 쓰지 않았다) |
| `8cde3db3` | **UUIDv7과 비밀 값(3.2, 3.3).** 아래 | 382단계 가운데 221단계. 상태 코드는 2단계 |
| `d4e10cb1` | **Argon2id와 세션 토큰 해시(3.4, 3.5).** 아래 "Argon2id와 세션 토큰에서 바뀐 것" | 382 → 385단계(새 3단계), 본문만 7단계 |
| `909fb7eb` | **삭제와 복원(5.3), 외래 키.** 아래 "삭제와 복원에서 바뀐 것" | 385 → 426단계(새 시나리오 `trash` 41단계). 기존 385단계 가운데 26단계, 상태 코드는 1단계 |
| `cb3fd0ce` | **세션 수명(5.4).** 한가함 한도 14일(`session_idle_days`)과 절대 한도 90일(`session_max_days`, 세션의 `expires_at`). 마지막 사용 시각은 마지막 기록에서 10분이 지났을 때만 쓴다(전에는 요청마다). 정리는 한 시간마다 돌고 끝난 세션을 바로 지운다(31일 하한이 없다). 비활성화된 사용자의 세션은 조회에서 걸러진다. `session_expire_time`, `session_refresh_time`은 설정이 아니다(값이 있으면 시작 오류로 새 키를 알려 준다). 테스트 `users::a_session_ends_when_it_was_not_used_for_the_idle_limit`, `…_at_its_absolute_limit_however_much_it_is_used`, `the_use_of_a_session_is_recorded_at_most_every_ten_minutes`, 스토어 `sessions::a_session_of_a_deactivated_user_is_not_found`. 자기 비밀번호를 바꿀 때 다른 세션을 끝내는 것은 `d4e10cb1`에 들어갔고, 지금의 세션에 새 토큰을 주는 것은 쿠키를 다시 줄 수 있는 인증 전달 단계에서 한다 | 변화 없음 |
| `224ef584` | **관리자(3.6).** 아래 "관리자에서 바뀐 것" | 426 → 455단계(새 시나리오 `admins` 29단계). 기존 단계 가운데 8단계, 본문만 |
| `b53d518e` | **서버가 만드는 공유 토큰(3.8).** 아래 "공유 토큰에서 바뀐 것" | 455 → 470단계(시나리오 `sharing`에 15단계). 기존 단계 가운데 11단계, 본문만 |
| `ef9a8d96` | **사용자 표시 설정(5.8).** 아래 "표시 설정에서 바뀐 것" | 470 → 500단계(새 시나리오 `settings` 30단계). 기존 단계 가운데 4단계, 본문만 |
| `7b845798` | **최소 역할(5.10).** 아래 "최소 역할에서 바뀐 것" | 500 → 514단계(시나리오 `members`에 14단계). 기존 단계 가운데 62단계, 본문만 |

**UUIDv7과 비밀 값에서 바뀐 것.**

| 바뀐 것 | 왜 | 어디 |
|---|---|---|
| 엔티티의 식별자(팀, 보드, 블록, 사용자, 카테고리, 카테고리-보드 행, 세션, 파일 기록)가 UUIDv7이다. 표기는 하이픈이 든 소문자 36자. PostgreSQL은 `uuid` 열, SQLite는 `TEXT`. 종류 접두사(`b…`, `c…`)는 없다 | 설계 3.2. 색인의 삽입 지역성. 순서의 뜻은 없다(정렬은 만든 시각으로 한다) | `fvoci_model::{new_id, is_id}`, `crates/store/src/db.rs`의 `Id`·`OptId`·`Row::id` |
| **식별자가 아닌 글은 아무것도 가리키지 않는다.** 조회는 아무 줄도 찾지 못하고(그 라우트의 404), 그것을 저장하려는 쓰기는 400 `bad_request`다. 다른 표기의 UUID(대문자, 중괄호, 하이픈 없음)와 nil UUID(전부 0)도 식별자가 아니다 | 설계 2.3의 마지막 행. PostgreSQL의 `uuid` 열에 잘못된 값이 가지 않고, 두 DB의 답이 같다. 경로를 미리 거르는 미들웨어는 두지 않았다: 세션(401)보다 먼저 404가 나가면 검사 순서가 어긋난다. 대신 저장소가 막고, 테스트가 "없는 것"과 답이 같은지 라우트마다 비교한다 | `StoreError::InvalidId`, 테스트 `visibility::text_that_is_no_identifier_answers_as_an_identifier_of_nothing`(33개 요청 × 5가지 글), `visibility::what_a_user_cannot_see_…`(42개 요청에 세 번째 경우) |
| 세션 토큰, 가입 토큰, 저장 파일 이름은 식별자 생성기를 쓰지 않는다. 운영체제 난수 256비트, base64url 43자 | 설계 3.3. UUIDv7은 앞이 시각이고 한 밀리초 안에서는 카운터가 이어진다 | `fvoci_server::auth::secret::new_secret` |
| **팀.** 루트 팀은 처음 시작할 때 UUIDv7으로 만들어진다(가장 오래된 팀이 루트다). 팀 `0`은 없다. `GET /teams/{id}`는 그 팀만 답하고 다른 ID는 404다. 경로가 가리키는 팀이 없으면 팀 라우트 전부가 404 `team not found`, 본문이 가리키면 400 `no such team`이다 | 결정 6(팀은 유지, 루트 팀도 UUIDv7). 보존한 quirk "`GET /teams/{아무 ID}`는 루트 팀을 답한다"가 닫혔다. 가입 토큰을 관리자만 보는 것은 관리자 단계에서 한다 | `App::{root_team, team}`, `api::access::{require_team_access, require_team_in_body}` |
| **내장 템플릿은 표식 열로 구분한다.** `boards.is_builtin`(와이어 `isBuiltin`). 만든 사용자가 없는(`createdBy`가 빈) 템플릿이다. 루트 팀에 속하고 모든 팀에 제공된다: `GET /teams/{team}/templates`가 그 팀의 템플릿과 내장 템플릿을 함께 답한다. `system`이라는 사용자와 내장 템플릿의 관리자 멤버십은 없다. 클라이언트가 보낸 `isBuiltin`은 무시한다. 사용자가 루트 팀에 만든 템플릿은 `trackingTemplateId`를 받지 않는다(전에는 팀 `0`의 템플릿이면 받았다) | 결정 6. 온보딩(S18)이 "시스템이 만든 보드"를 `createdBy`의 글자가 아니라 이 열로 본다 | `Store::{get_builtin_templates, remove_builtin_templates}`, `app/templates.rs` |
| **채널이 없다.** 보드와 패치의 `channelId` 멤버, 채널 열, 채널 권한 검사가 없다. 패치에 `channelId`가 와도 무시한다 | 결정 4에서 이 단계로 미뤄 둔 것 | 모델 `Board`, `BoardPatch` |
| **파일.** 저장 이름은 비밀 값 + 확장자다(템플릿 보드에 올려도 같다. 전에는 클라이언트의 이름을 그대로 썼다). 파일 기록(`file_info`)에 기본 키, `board_id`, 저장 이름(`name`), 올린 이름(`original_name`)이 있고, 파일은 `(보드, 저장 이름)`으로 찾는다. 기록이 없는 파일은 어느 보드로도 나가지 않는다. 정보 JSON은 `id, create_at, delete_at, name, extension, size, archived`다(원본의 채움 멤버 여섯을 뺐다) | 설계 3.2·3.9. S16의 "파일은 한 보드의 것"이 경로 문자열이 아니라 열이 됐다. 이름의 첫 글자를 떼어 기록을 찾던 규칙이 없어졌다 | `app/files.rs`, `store/file_info.rs` |
| 읽기 토큰만으로(세션 없이) 읽은 보드와 블록의 `createdBy`·`modifiedBy`는 빈 값이다 | 결정 7. UUIDv7이 그 사용자의 가입 시각을 드러낸다 | `api/boards.rs`, `api/blocks.rs` |
| 사용자 이름과 이메일은 대소문자와 무관하게 하나뿐이다(유일 색인 `lower(…)`). 조회도 대소문자를 무시한다. 동시에 온 가입 둘 가운데 하나는 409다(전에는 둘 다 들어갔다) | 설계 3.9. SQLite의 `lower()`는 ASCII만 접는다(PostgreSQL은 DB의 로케일을 따른다). 두 DB의 차이로 적어 둔다 | `store/users.rs`, `StoreError::Conflict` |
| 보드에 바로 달린 블록에는 부모가 없다. 복제와 가져오기에서 "부모 = 보드 ID"는 빈 `parentId`가 된다(전에는 복사본이 **원본** 보드의 ID를 부모로 들고 있었다). 가져오기는 부모가 아카이브에 없는 블록을 건너뛴다(내장 템플릿의 주인 없는 블록 38개가 이제 저장되지 않는다. 어느 카드에도 속하지 않아 화면에 보인 적이 없다) | UUID 열에는 옛 형식의 ID가 들어갈 수 없다. 블록의 부모를 외래 키로 묶는 것은 삭제와 복원 단계에서 한다(그때 `POST /boards/{id}/cards`가 부모로 보드 ID를 넣는 것도 고친다) | `fvoci_model::renew_boards_and_blocks_ids`, `app/import.rs`의 `drop_orphans` |
| 스키마에서 뺀 것: `blocks.root_id`·`channel_id`, `boards.channel_id`, `categories.channel_id`, `sharing.workspace_id`, `subscriptions.workspace_id`, `board_members.roles`, `users.mfa_secret`·`auth_service`·`auth_data`·`props`, 테이블 `notification_hints`·`system_settings`·`board_members_history`. 길이 제한이 붙은 `VARCHAR`는 `TEXT`로, 대부분의 열은 `NOT NULL`로 | 설계 3.1. 쓰이지 않거나 값이 하나뿐이었다. 저장소의 쓰이지 않는 함수(`get_board_count`, `get_active_user_count`, `update_user`, `update_session`, 시스템 설정 등)도 함께 지웠다 | `crates/store/schema/` |

**스냅샷.** 382단계 그대로, 권한 표 636칸은 한 칸도 바뀌지 않았다. 바뀐 221단계 가운데 상태 코드가 바뀐 것은 둘이다: `GET /teams/anything`이 200(루트 팀)에서 404로, 팀 없는 `POST /boards`가 403에서 400(`no such team`)으로. 나머지 219단계는 본문만 바뀌었다: 팀 ID가 `0`에서 식별자로, `channelId`가 빠지고 `isBuiltin`이 생기고, 복제·가져온 카드의 `parentId`가 비고, 파일 이름이 `<secret:N>`이 되고, 공유로 읽은 보드·블록의 작성자가 비고, 파일 정보의 멤버가 줄었다. 내장 템플릿의 속성 ID(옛 형식 27자)는 이제 식별자로 보지 않아 스냅샷에 그대로 나온다(아카이브의 상수다). 회귀 하네스는 UUID를 `<id:N>`으로, 43자 비밀 값을 `<secret:N>`으로 바꾼다.

**테스트.** 스토어의 테스트(원본 스위트를 옮긴 것)는 새 스키마에 맞게 고쳤다: 식별자는 모두 생성한 것이고, 줄이 가리키는 것(사용자, 팀, 보드)은 모두 실제로 저장한다(외래 키는 삭제와 복원 단계에서 넣는다). 없앤 함수의 테스트는 지웠다(`get_board_count`, `get_active_user_count`, `update_session`, `upsert_team_settings`, 멤버십 이력, 시스템 설정). 스토어 테스트의 변환(네 묶음)과 서버 API 테스트의 일부(보드 묶음, 사이드바)는 하위 에이전트에게 맡겼다. 여섯 모두 Fable 5.2로 지정했고 다른 모델로 넘어간 것은 없다(각자 보고에 적었다). 에이전트가 찾아 고친 것: PostgreSQL 테스트 스키마 이름에 하이픈이 들어간 것(하네스), nil UUID가 식별자로 통해 "식별자가 아닌 글"의 조회에 답하던 것, 템플릿이 아닌 보드까지 내장으로 표시되던 것, 카테고리 생성이 팀보다 본문을 먼저 보던 것.

| 검증 (이 커밋, 2026-11-10 20:48 KST, rustc 1.99.1) | 결과 |
|---|---|
| `cargo xtask check` | format, clippy, unit tests, documentation, names, licenses and advisories 모두 PASS |
| `cargo xtask test` — SQLite | 782건 통과, 실패 0 |
| `cargo xtask test` — PostgreSQL | 690건 통과, 실패 0 |
| `cargo xtask test` — S3 | 1건 통과 |

**Argon2id와 세션 토큰에서 바뀐 것 (2026-11-10 21:09 KST).**

| 바뀐 것 | 왜 | 어디 |
|---|---|---|
| 비밀번호 해시가 Argon2id다: m=19456 KiB, t=2, p=1, PHC 문자열(`$argon2id$v=19$m=19456,t=2,p=1$…`)로 `users.password_hash`에. bcrypt와 그 검증 경로는 없다 | 결정 8, 설계 3.4. 매개변수는 OWASP Password Storage Cheat Sheet의 요약 값이자 `argon2` 0.6.0의 기본값이다(크레이트 소스 `src/params.rs`의 `DEFAULT_*`를 직접 읽었다) | `auth/password.rs` |
| 로그인이 성공했는데 저장된 해시의 매개변수가 지금 기준과 다르면 새로 해시해 저장한다. 저장된 해시가 그사이 바뀌었으면(비밀번호 변경) 건드리지 않는다 | 기준을 올려도 사용자가 비밀번호를 다시 정하지 않아도 된다 | `App::login`, `Store::replace_password_hash`, 테스트 `users::a_login_renews_a_hash_made_with_other_parameters` |
| 비밀번호의 길이: 최소 8**자**(유니코드 스칼라 수. 전에는 8바이트), 최대 1,024바이트. 가입, 변경, 관리자의 설정 모두 같은 규칙이다. 72바이트 제한과 그에 딸린 400, 64바이트 상한(`max-length`)은 없다. 한도를 넘는 비밀번호로 로그인하면 해시하지 않고 틀린 비밀번호와 같은 401이다 | 설계 3.4. bcrypt는 앞 72바이트만 봤다. 최대는 아주 긴 입력으로 해시 시간을 늘리는 것을 막는다 | `fvoci_model::check_new_password`, 테스트 `users::passwords_are_bounded_…`, `admin::socket::admin_set_password_applies_the_password_rules` |
| 동시에 도는 해시는 4개다(전에는 프로세서 수) | 결정 8. 해시 하나가 19 MiB를 쓴다. 대기열과 대기 시간은 로그인 제한 단계에서 넣는다 | `app/auth.rs` |
| **세션 토큰은 SHA-256 다이제스트로 저장한다**(`sessions.token_hash`, 유일). 토큰 자체는 로그인 응답으로 한 번 나가고 어디에도 저장되지 않는다. 가입 토큰과 공유 토큰은 평문이다 | 결정 9. DB나 백업을 읽은 것만으로는 세션을 쓸 수 없다. 토큰이 256비트 난수라 느린 해시는 필요 없다 | `auth::secret::token_digest`, `store/sessions.rs`, 테스트 `users::the_store_keeps_the_digest_…` |
| 세션 행이 `id, token_hash, user_id, created_at, last_used_at, expires_at, user_agent`다. `auth_service`, `props`는 없다. 만료 규칙(한가함 한도, 절대 한도)과 기록 간격은 세션 수명 단계에서 바꾼다. 지금은 전처럼 마지막 사용에서 `session_expire_time`이 지나면 끝나고, 요청마다 사용 시각을 쓴다 | 설계 5.4의 행 모양을 한 번에 맞췄다 | 모델 `Session` |
| **자기 비밀번호를 바꾸면 그 사용자의 다른 세션이 모두 끝난다.** 바꾼 세션은 남는다. 다른 세션으로 인증한 WebSocket 연결도 닫는다 | 설계 5.4의 3번 (b). 비밀번호 변경과 세션 삭제가 한 트랜잭션이라 여기서 함께 했다 | `Store::change_user_password`, `Hub::close_other_sessions_of_user`, 테스트 `users::changing_the_password_ends_the_other_sessions_of_the_user` |
| 가입·비밀번호 요청의 검증 오류가 `field`를 싣는다(`username`, `email`, `password`, `oldPassword`, `newPassword`) | 설계 1.2. 화면이 어느 칸의 오류인지 안다 | `AuthParamError::field` |

스냅샷: `auth` 시나리오에 세 단계를 더했다(다른 기기에서 로그인, 비밀번호 변경 뒤 그 기기의 401, 바꾼 세션의 200). 본문이 바뀐 일곱 단계는 `field`가 붙은 여섯과 길이 한도의 문구 하나다. 아카이브 항목 이름의 정렬이 무작위 파일 이름에 흔들리지 않게 하네스의 정렬 기준을 고쳤다. 의존성: `argon2` 0.6, `sha2` 0.11을 더하고 `bcrypt`를 뺐다(`NOTICE` 다시 생성, `cargo deny` 통과). 검증: `cargo xtask check` 여섯 단계 PASS, `cargo xtask test` SQLite 790건·PostgreSQL 697건·S3 1건, 실패 0.

### H. 구현 4단계: 인증 전달 (2026-11-10 23:04 KST)

설계 5.1(쿠키)과 5.4의 3번(비밀번호를 바꾼 세션에 새 토큰)을 구현했다. 커밋 하나다.

| 바뀐 것 | 왜 | 어디 |
|---|---|---|
| **로그인이 세션 쿠키를 설정하고 본문에는 토큰이 없다**(본문은 `{}`). 쿠키는 `HttpOnly; SameSite=Lax; Path=/`이고 `Max-Age`는 한가함 한도다. `serverRoot`가 `https`이면 이름이 `__Host-fvoci_session`이고 `Secure`가 붙는다. `http`이면 `fvoci_session`이다. 각 모드는 자기 이름의 쿠키만 읽는다. 로그아웃은 `Max-Age=0`으로 지운다. `securecookie` 설정은 없다(값이 있으면 시작 오류) | 설계 5.1의 1–3번. 페이지의 스크립트가 토큰을 볼 길이 없다 | `auth::token::SessionCookie`, `api/auth.rs` |
| **토큰을 받는 길은 둘이다:** `Authorization: Bearer <토큰>`과 세션 쿠키. 둘 다 오면 헤더를 쓴다(헤더의 토큰이 세션이 아니면 쿠키가 유효해도 401). 브라우저가 아닌 클라이언트는 로그인 응답의 `Set-Cookie`에서 토큰을 읽어 헤더로 보낸다. **없앤 길:** 쿼리의 `access_token`(주소가 로그와 기록에 남는다), `token` 스킴, 옛 쿠키 이름 `FVOCIAUTHTOKEN` | 설계 5.1의 3번. 쿼리의 토큰은 설계가 남기기로 한 길에 없다 | `auth::token::{Credential, bearer_token}` |
| **위조 방지 검사의 범위.** 안전하지 않은 메서드(`GET`·`HEAD`·`OPTIONS` 밖)에만 한다. (1) `Origin`이 있으면 `serverRoot`의 출처이거나 `allowed_origins`의 하나여야 한다. 아니면 403 `foreign_origin`. (2) `Authorization: Bearer`가 없으면 `X-Requested-With: XMLHttpRequest`가 있어야 한다. 아니면 403 `csrf`. `GET`은 헤더 없이 된다(이미지 태그와 링크) | 설계 5.1의 4·7번 | `api::guard_against_forgery`, `auth::token::Origins` |
| **설계와 다른 점 둘.** 헤더는 "쿠키로 인증된" 요청이 아니라 "Bearer가 없는" 요청에 요구한다: 세션이 없는 로그인·가입도 포함된다(위조된 로그인은 브라우저를 공격자의 계정에 로그인시킨다). `Origin` 검사는 Bearer로 인증한 요청에도 한다(다른 출처의 페이지가 그 토큰을 보낼 이유가 없다) | 더 엄격한 쪽으로 정했다. 브라우저가 아닌 클라이언트는 `Origin`을 보내지 않아 영향이 없다 | 같은 곳 |
| **쿠키의 갱신.** 세션의 사용을 기록하는 요청(마지막 기록에서 10분이 지난 첫 요청)의 응답에 쿠키를 `Max-Age`를 채워 다시 준다. 쿠키로 온 요청에만 한다 | 설계 5.1의 1번, 5.4. 브라우저가 쿠키를 잊는 때가 서버가 세션을 끝내는 때와 맞는다 | `api::session::{renew_session_cookie, CookieRenewal}` |
| **비밀번호를 바꾼 세션은 새 토큰을 받는다.** 응답이 쿠키를 다시 설정하고 옛 토큰은 401이다. 다른 세션이 끝나는 것은 전과 같다. 비밀번호 변경, 다른 세션 삭제, 토큰 교체가 한 트랜잭션이다 | 설계 5.4의 3번 | `Store::change_user_password`, `App::change_password` |
| **WebSocket은 핸드셰이크에서 인증한다.** `AUTH` 메시지는 없다(와도 모르는 동작으로 무시한다). `Origin`이 있으면 허용된 출처여야 하고 아니면 403 `foreign_origin`. 세션은 쿠키의 것이다. 쿠키가 없거나 세션이 아니면 익명 연결이다(공유 보드의 블록만 읽기 토큰으로 구독한다). `Origin`이 없으면 `Authorization: Bearer`가 있어야 하고(없으면 401) 그 토큰은 세션이어야 한다. Bearer 토큰이 세션이 아니면 출처가 맞아도 401이다 | 설계 5.1의 5번. 브라우저는 아무 페이지나 WebSocket을 열게 두고 쿠키를 싣는다 | `lib.rs`의 `websocket_identity`, `ws::Identity` |
| **파일 응답의 헤더.** `Content-Security-Policy: sandbox; frame-ancestors 'none'`(전에는 `Frame-ancestors 'none'`만). 모든 응답에 `Referrer-Policy: no-referrer` | 설계 5.1의 7·8번. 올린 SVG·HTML을 문서로 열어도 스크립트와 출처가 없다. 공유 보드의 주소(읽기 토큰)가 다른 사이트로 새지 않는다 | `api/files.rs`, `lib.rs`의 `send_no_referrer` |
| 설정 `allowed_origins`(목록, 기본 빈 값): `serverRoot`의 출처 말고 더 받을 출처. 개발용이다. `serverRoot`와 이 목록이 `http`·`https` 주소가 아니면 시작 오류다 | 설계 5.1의 5번 | `config.rs` |

**스냅샷.** 514 → 536단계. 새 시나리오 `cookies`(21단계)가 로그인의 쿠키와 빈 본문, 읽기와 바꾸기에서의 헤더·출처 규칙, 없앤 길, 로그아웃을 고정한다. `auth`에 1단계(비밀번호를 바꾸기 전의 토큰이 401). 기존 단계 가운데 8단계가 바뀌었고 상태 코드는 하나다: 헤더 없는 `GET /clientConfig`가 403에서 200으로. 나머지는 본문·헤더다(로그인 본문의 `token`이 없어졌고, 파일의 CSP가 바뀌었다). 권한 표 636칸은 그대로다. 회귀 하네스는 로그인의 `Set-Cookie`에서 토큰을 읽고, 단계마다 Bearer와 쿠키를 골라 보낼 수 있다. 쿠키의 토큰은 `<secret:N>`으로 정규화된다.

**테스트.** API `cookies::*` 10개(쿠키의 속성, HTTPS의 이름, 로그아웃, 헤더 검사, 출처 검사, 헤더와 쿠키의 우선순위, 갱신, 비밀번호 변경, 파일을 쿠키만으로 읽기와 헤더, WebSocket 핸드셰이크의 출처와 세션 조합), `websocket::a_message_does_not_sign_a_connection_in`, 단위 `auth::token::tests` 7개.

### I. 구현 5단계: 로그인 시도 제한과 `trusted_proxies` (2026-11-10 23:24 KST)

설계 5.2를 구현했다. 커밋 하나다. 수치는 모두 설정이다(설계 5.10의 표).

| 바뀐 것 | 왜 | 어디 |
|---|---|---|
| **쌍(계정, 주소)의 대기.** 한 주소가 한 계정에서 연속으로 실패하면 5회(`login_pair_free_failures`)까지는 그냥 두고, 그 뒤로 실패할 때마다 30초(`login_pair_base_delay_seconds`)에서 두 배씩, 한 시간(`login_pair_max_delay_seconds`)까지 기다리게 한다. 기다리는 동안은 맞는 비밀번호도 429다. 다른 주소에서 오는 같은 계정의 로그인은 막히지 않는다 | 설계 5.2의 1번 (c). 남의 계정을 잠글 수 없다 | `auth::limiter::LoginLimiter` |
| **계정의 잠금.** 어디서 오든 연속 실패 100회(`login_account_max_failures`)면 그 계정의 로그인을 한 시간(`login_account_lock_seconds`) 막는다. 잠금이 풀리면 다시 센다 | 설계 5.2의 1번. 주소를 바꿔 가며 하는 시도(NIST의 상한 100) | 같은 곳 |
| **성공하면** 그 계정의 연속 실패와, 성공한 주소의 그 계정 기록을 지운다. 다른 주소(추측하는 쪽)의 대기는 남는다 | 설계는 "그 계정의 기록을 지운다"였다. 주인이 로그인했다고 추측하는 쪽의 대기까지 풀어 줄 이유가 없어 좁혔다 | `Attempt::succeeded` |
| **없는 계정도 같다.** 같은 기록(시도한 이름으로), 같은 응답(401 `invalid_credentials`), 같은 양의 일(아무도 모르는 값의 해시에 대해 한 번 검증)이다. 있는 계정은 이름·이메일 어느 쪽으로 시도하든 사용자 ID 하나로 센다 | 설계 5.2의 2번. 응답 시간이 계정의 존재를 드러냈다(리뷰 S6) | `app/auth.rs`의 `login`, `auth::password::verify_against_no_account` |
| **주소의 제한.** 주소당 로그인·가입 시도 분당 20회(`login_address_attempts_per_minute`, 성공 포함), 한 번에 하나. IPv6는 /64로 묶는다. 0이면 주소별 제한을 끈다(모든 사용자가 한 주소에서 오고 그것을 풀어 줄 프록시가 없는 배치). 토큰이 없거나 틀려서 해시 전에 거절되는 가입은 시도로 세지 않는다 | 설계 5.2의 3번 | `LoginLimiter::{admit_login, admit_registration}` |
| **해시 대기열.** 동시 4(`password_hash_concurrency`), 대기 16칸(`password_hash_queue`), 대기 5초(`password_hash_queue_seconds`). 자리가 없거나 차례가 제때 오지 않으면 해시하지 않고 429다. 로그인, 가입, 비밀번호 변경, 관리자 소켓의 재설정이 모두 이 대기열을 쓴다. 프로세스 전체의 정적 세마포어를 앱마다의 대기열로 바꿨다. 차례는 일이 끝날 때까지 잡고 있다(요청이 끊겨도) | 설계 5.2의 4번. 리뷰 S13 | `auth::hash_queue::HashQueue` |
| **거절은 429 `rate_limited`**, `Retry-After` 헤더와 본문의 `retry_after`(초, 올림, 1 이상) | 설계 5.2의 5번 | `AppError::rate_limited`, `fvoci_model::ErrorBody::retry_after` |
| **`trusted_proxies`.** 믿는 프록시의 주소·범위 목록. 연결의 상대가 목록에 있을 때만 `X-Forwarded-For`를 오른쪽부터 읽어 목록에 없는 첫 주소를 클라이언트로 본다. 비어 있으면 연결의 상대가 클라이언트다. 요청 로그와 로그인 제한이 같은 주소를 쓴다. `trust_forwarded_for`는 설정이 아니다(값이 있으면 시작 오류). 전에는 켜면 가장 왼쪽(클라이언트가 지어낼 수 있는 쪽)을 읽었다 | 설계 5.2의 6번 | `client_address::{TrustedProxies, ClientAddress}`, `request_log.rs` |
| 기록은 메모리에 둔다. 종류마다 10만 개까지이고, 넘치면 더 말할 것이 없는 기록(대기가 끝나고 오래된 것)부터, 그래도 넘치면 임의의 10분의 1을 버린다. 계정 이름은 SHA-256으로 줄여 둔다 | 설계 5.2의 7번 | `auth::limiter` |

**Argon2id 측정(2026-11-10 23:12 KST, 이 기기: Ryzen 9 5950X, 논리 코어 32, 릴리스 빌드, m=19456 KiB·t=2·p=1).** 해시 한 번은 혼자일 때 중앙값 13.8 ms(12.4–22.0 ms)다. 동시에 돌리면 한 번에 걸리는 시간이 늘고 처리량은 8개에서 멈춘다: 2개 18.4 ms(초당 109회), 4개 24.3 ms(165회), 8개 40.7 ms(197회), 16개 84.6 ms(189회). 메모리 대역이 한계로 보인다(측정한 것은 시간뿐이다). 측정 프로그램은 `target/s6/hashtime/`에 있다(커밋하지 않았다).

**대기열 수치를 다시 본 결과.** 제안값(동시 4, 대기 16, 5초)을 그대로 둔다. 동시 4에서 초당 165회이므로 대기 16칸이 다 차 있어도 마지막 차례가 0.1초쯤에 온다. 5초는 이 기기에서는 닿지 않는 상한이고, 해시가 열 배 느린 기기(한 번 240 ms)에서도 16칸이 1초쯤에 빠진다. 동시를 8로 올리면 처리량이 20% 늘지만 메모리가 152 MiB까지 들고 로그인 한 번의 지연이 24 ms에서 41 ms가 된다. 주소당 분당 20회가 앞에서 막으므로 올릴 이유가 없다고 봤다.

**스냅샷.** 536 → 554단계. 새 시나리오 `login_limits`(18단계): 있는 계정과 없는 계정이 같은 순서로 401 여섯 번 뒤 429, 맞는 비밀번호도 기다리는 것, 다른 계정은 막히지 않는 것. 기존 단계는 바뀌지 않았다. 대기 초는 실행마다 1초 어긋날 수 있어 하네스가 `<seconds>`로 바꾼다(값의 범위는 API 테스트가 본다). 하네스의 서버는 주소별 제한을 끈다(모든 클라이언트가 127.0.0.1이다).

**테스트.** 단위 `auth::limiter::tests` 8개(대기의 증가, 실패한 주소만, 성공의 효과, 계정 잠금, 주소의 분당 횟수와 동시 1, IPv6 /64, 주소 없는 시도, 넘칠 때 버리는 순서), `auth::hash_queue::tests` 3개, `client_address::tests` 5개. API `login_limits::*` 6개(쌍의 대기, 프록시 뒤의 주소 구분, 믿지 않는 헤더, 계정 잠금, 주소의 분당 횟수, 대기열 넘침).

### J. 리뷰(rust-review)와 반영 (2026-11-10 23:48 KST)

스키마 단계(`693c4362..7b845798`)를 rust-review로 검토했다. 하위 에이전트 둘(스토어·모델, 서버)을 Fable 5.2로 지정했고 둘 다 그 모델로 돌았으며 스킬을 불러왔다고 보고했다. 읽기만 했고 `cargo`는 돌리지 않았다(검토 대상 커밋을 `git show`로 읽었다). 번호는 `target/s6/review-todo.md`의 것이다. 인증 전달·로그인 제한·`/api/v1`·OpenAPI 단계의 리뷰와 보안 관점 검토는 그 단계들이 끝난 뒤 따로 한다.

**Must fix (4건, 모두 반영).**

| # | 지적 | 반영 |
|---|---|---|
| M1 | `patch_block`·`patch_board`가 잠그기 전에 읽어, PostgreSQL에서 동시에 온 두 패치 가운데 하나가 사라진다(카드의 제목과 속성을 동시에 고치면 한쪽이 지워진다) | 블록은 보드를 알아낸 뒤 잠그고 다시 읽는다. 보드는 잠근 뒤 읽는다. `insert_board`의 `UPDATE`가 0줄이면 `NotFound`다 |
| S1 | 가져오기의 `drop_orphans`가 깊이에 비례해 반복한다(블록 수의 제곱). 큰 아카이브 하나로 런타임 워커를 오래 붙잡을 수 있다 | 보드에 달린 블록에서 내려가는 한 번의 순회로 바꿨다. 20만 개 사슬이 0.4초(디버그 빌드)다. 어디에도 달리지 않은 순환도 함께 버린다 |
| S2 | `POST /boards/{id}/blocks?sourceBoardID=`가 아직 저장하지 않은 블록을 패치하려다 404가 나고 복사한 파일이 남는다(S6 이전부터 있던 순서) | 저장 전에 메모리에서 블록의 `fileId`를 복사본으로 바꾼다(`App::copy_card_files_into`). 테스트 `files::blocks_posted_from_a_source_board_get_copies_of_their_files` |
| S3 | 읽기 토큰을 끄거나 바꿔도 그 토큰으로 구독한 WebSocket 연결이 계속 받는다. 익명 구독자에게 가는 블록에 작성자 ID가 들어 있다(HTTP는 비운다) | 연결이 어느 공유 보드의 토큰으로 구독했는지 기억하고, 공유를 끄거나 새 토큰을 내거나 보드를 지우면 그 연결을 닫는다. 세션 없는 구독자에게는 작성자를 비워 보낸다 |

**Should fix (18건 가운데 17건 반영, 1건은 일부).**

| # | 지적 | 반영 |
|---|---|---|
| M2 | `add_member`가 잠그지 않아, 동시에 제거되면 없는 멤버십을 "새로 추가됨"이라고 답한다 | 보드 줄을 잠근다 |
| M3 | `delete_board`의 잠금(`FOR NO KEY UPDATE`)이 외래 키로 들어오는 행(멤버십, 파일 기록)을 막지 못해, 그사이 들어온 행이 휴지통에 담기지 않고 사라진다 | 삭제는 `FOR UPDATE`로 잠근다 |
| M4 | 화면 상태의 100개 한도가 동시 요청에서 지켜지지 않는다 | 사용자 줄을 잠근 뒤 센다. 없는 사용자는 `UserNotFound`다 |
| M5, S8 | 휴지통을 비우면 파일의 위치를 아는 유일한 기록이 사라지고 파일이 영영 남는다. 올리기·복사가 실패하면 기록 없는 파일이나 파일 없는 기록이 남는다 | `purge_trash`가 지운 보드의 파일 기록을 돌려주고 서버가 그 파일을 지운다(`FileBackend::delete`). 올리기는 기록 저장이 실패하면 파일을 지운다. 복사는 파일을 먼저 복사하고 실패하면 기록도 이름도 만들지 않는다. **남긴 것:** 따로 지운 블록의 파일 수거는 다음 묶음(5.7)이다 |
| M6 | 가리키는 보드·사용자·블록이 없을 때 외래 키 위반이 500으로 나간다 | 멤버 추가·참여, 파일 기록, 구독, 카테고리 자리에서 `NotFound`로 바꾼다 |
| M7, S7 | 비밀번호 재설정과 로그인이 겹치면 옛 비밀번호로 연 세션이 남을 수 있다. 해시를 갱신하는 로그인과 겹친 다른 로그인은 맞는 비밀번호인데 거절된다 | 세션 저장이 "사용자의 해시가 검사한 그 해시인가"를 같은 트랜잭션에서 본다(사용자 줄 잠금). 해시가 바뀌었으면 지금의 해시로 한 번 더 검사한다 |
| M8 | 검사 없는 `save_member`·`delete_member`가 공개 API로 남아 있다 | 테스트 전용(`test-util`)으로 돌렸다 |
| M9 | 설정 타입의 `FromStr`가 `Err = ()`다 | `ParseSettingError` |
| M10, S11 | 코드와 맞지 않는 주석(지워진 보드의 멤버십, 정리 주기, 카테고리의 삭제 표시, 모델 크레이트의 머리말, `minimum_role` 멤버) | 고쳤다. `# Errors`의 `InvalidId` 누락은 손댄 함수만 고쳤다 |
| S4 | 비밀번호 변경이 그사이의 관리자 재설정을 덮어쓴다 | 검사한 해시가 그대로일 때만 바꾼다. 아니면 409 |
| S5 | 로그아웃해도, 세션이 만료돼도 그 세션의 WebSocket 연결이 남는다 | 로그아웃이 그 세션의 연결을 닫는다. 정리 작업이 지운 세션의 연결을 닫는다(한 시간마다. 그사이의 만료는 다음 정리 때 닫힌다) |
| S6 | 없는 사용자의 로그인이 더 빨리 거절된다 | 로그인 제한 단계(I절)에서 고쳤다 |
| S9 | 저장소 오류를 404·401로 답하는 곳 넷 | 사용자 조회, 복사 원본 보드 확인, 읽기 토큰 확인이 오류를 그대로 올린다. 설정에서 공유가 꺼져 있으면 읽기 토큰 확인은 오류가 아니라 "아니오"다 |
| S10 | 카테고리에 넣는 단계가 실패하면 멤버십 알림이 빠진다 | 저장한 뒤 바로 알리고 그다음에 카테고리에 넣는다. 블록 복제도 같다 |
| S12 | 가입이 이메일 충돌과 이름 충돌을 오류 문구로 구별한다 | `StoreError::Conflict(Taken::{Username, Email})` |

**Consider (15건).** 반영: 해시 대기의 상한과 앱마다의 대기열(S13, I절), 사라진 보드에 멤버를 추가하면 200 `null`이던 것을 404로(S15), 업로드 로그에서 저장 이름을 뺌(S16), 정리와 비우기를 시작할 때도 한 번 돌림(S17), 설정의 저장소 API를 타입으로(M11), `touch_session`이 시각을 뒤로 돌리지 않게(M13 일부), 휴지통 왕복 테스트(M15), 부모 = 보드인 조회(M16), `boards.type`·`categories.type`의 `CHECK`(M17 일부).

남긴 것과 이유:

| # | 지적 | 왜 남겼나 |
|---|---|---|
| S14 | 가입 토큰 재발급에서 팀 확인이 관리자 확인보다 먼저다 | 팀 라우트는 모두 "없는 팀은 무엇보다 먼저 404"다(G절). 팀의 존재는 로그인한 사용자에게 비밀이 아니다. `require_admin`의 주석을 그에 맞게 고쳤다 |
| S18 | `hash_password_with`가 `argon2::Params`를 공개 시그니처에 드러낸다 | API 테스트(다른 매개변수의 해시를 갱신하는 테스트)가 쓴다. 테스트 전용 기능으로 가르면 서버 크레이트에 기능이 하나 는다 |
| M12 | `lock_board`를 트랜잭션에 묶는 타입(`TxScope`)이 없다 | 맞는 지적이다. 스토어의 모든 내부 함수 시그니처가 바뀌는 리팩터링이라 이 묶음에서 하지 않았다. 지금 호출하는 곳은 모두 트랜잭션 안이다. 남은 문제에 적는다 |
| M13 | 토큰 다이제스트와 `(now, idle_since)`가 맨 문자열·정수다 | 같은 이유(시그니처 리팩터링). 시각을 뒤로 돌리는 것만 막았다 |
| M14 | `set_sharing`의 토큰이 느슨한 `&str`다 | 서버는 늘 256비트 난수를 넘긴다. 읽는 쪽이 빈 토큰을 거절한다 |
| M17 | `created_at`과 `create_at`이 섞여 있다. `ORDER BY name`이 DB의 정렬 규칙을 따른다 | 열 이름을 통일하면 와이어 이름과 어긋난다(와이어 정리는 계약 단계와 다음 묶음). 정렬 규칙의 차이는 두 DB의 차이로 적어 둔다 |
| M18 | 서버가 쓰지 않는 공개 함수(`get_block_counts_by_type` 등), 사용자를 비활성화하는 함수가 없음 | 원본 스토어에서 옮긴 것들이다. 쓰임이 정해질 때 지우거나 쓴다. 비활성화를 넣을 때는 `instance` 줄 잠금이 필요하다는 것을 남은 문제에 적는다 |
| M19 | 작은 중복과 이름(`sqlite_id`, 모델 enum의 `FromStr`) | 동작과 무관하다 |

**스토어 테스트.** 스토어의 통합 테스트를 바뀐 시그니처에 맞추고 경쟁 테스트를 더하는 일은 하위 에이전트 하나에 맡겼다(Fable 5.2로 지정했고 그 모델로 돌았으며 `rust-best-practices` 스킬을 불러왔다고 보고했다. `crates/store/tests/store/` 아래만 고쳤다). 197건이 215건이 됐다. 더한 것:

| 지적 | 테스트 |
|---|---|
| M1 | `blocks::two_patches_of_a_block_at_once_both_take_effect`(80회), `boards::two_patches_of_a_board_at_once_both_take_effect`(50회) |
| M2 | `boards::add_member_racing_remove_member_answers_what_is_true`(100회) |
| M3 | `boards::a_member_added_while_the_board_is_deleted_goes_with_the_board_or_is_not_found`, `boards::a_file_saved_while_the_board_is_deleted_goes_with_the_board_or_is_not_found`(각 50회) |
| M4 | `users::changes_at_once_never_overfill_the_bag_of_client_state` |
| M5 | `boards::purge_trash_returns_the_file_records_of_the_boards_it_removed` |
| M7, S4 | `sessions::a_session_opened_with_the_old_password_does_not_outlive_a_reset`(100회), `sessions::create_session_with_a_hash_that_is_not_the_users_stores_nothing`, `users::change_user_password_from_a_stale_hash_changes_nothing`, `users::of_two_password_changes_from_the_same_hash_one_is_made` |
| M13 | `sessions::touch_session_does_not_move_the_time_backwards` |
| M15 | `boards::a_board_comes_back_from_the_trash_with_every_member_it_had`(구조체를 `..` 없이 적어, 멤버가 늘면 값을 줄 때까지 컴파일되지 않는다) |
| M16 | `blocks::get_blocks_with_the_board_as_parent_returns_the_blocks_directly_on_the_board` |
| M17 | `setup::the_schema_refuses_a_board_type_that_is_none`, `setup::the_schema_refuses_a_category_type_that_is_none` |

경쟁 테스트 넷(보드 삭제, 멤버 제거, 비밀번호 재설정과의 경쟁)에서 두 순서가 두 DB 모두에서 실제로 나왔다고 에이전트가 보고했다(임시 출력으로 확인하고 지웠다고 했다. 내가 다시 확인하지는 않았다).

에이전트가 스토어에서 찾은 것과 처리:

- **SQLite에서 쓰기가 굶는다.** 기존 테스트 `blocks::concurrent_writers_on_one_board_all_succeed`(한 보드에 16개 태스크가 20번씩 쓴다)가 전체 스위트에서 간헐적으로 `database is locked`로 실패했다(에이전트의 측정: 새 테스트를 빼고 25회 가운데 10회, 넣고 59회 가운데 12회. 실패한 쓰기는 busy timeout인 5.03초를 기다렸다. 기기의 load average가 40–60이던 때다. 리뷰 반영 전의 스토어와는 비교하지 않았다). SQLite의 쓰기 잠금은 기다리는 쪽이 이따금 다시 물어볼 뿐이라, 쓰기가 이어지면 한 연결이 매번 밀릴 수 있다. **고친 것:** SQLite의 트랜잭션을 스토어 안에서 온 순서대로 줄 세운다(`Store::transaction`의 `tokio::sync::Mutex`). SQLite는 어차피 쓰기가 하나다. busy timeout은 줄 밖의 것(단일 문장 쓰기, 다른 프로세스)에만 남는다. 고친 뒤 load average 40에서 전체 스위트 12회가 모두 통과했고 걸리는 시간이 9–11초에서 5초 안팎으로 줄었다. 단일 문장 쓰기(`with_conn`)는 줄을 서지 않으므로 같은 굶주림이 이론상 남는다. 남은 문제에 적는다.
- `create_session`의 `# Errors`가 사용자 식별자에 대해 `InvalidId`라고 적었는데 실제 답은 `false`다. 문서를 고쳤다.
- `add_member`에 식별자가 아닌 글자를 주면 보드 자리에서는 `NotFound`, 사용자 자리에서는 `InvalidId`다(쓰는 자리의 식별자가 아닌 글자는 `InvalidId`라는 스토어의 규칙 그대로다). 문서가 `NotFound`만 적고 있어 고쳤다.
- `categories.type`의 `CHECK`가 생겨 빈 종류를 `custom`으로 읽던 갈래에 닿을 수 없다. 지웠다. 모르는 종류는 여전히 `Corrupt`다.

**스냅샷.** 응답이 바뀐 단계가 없다(554단계 그대로).

**남은 문제(이 리뷰에서 넘긴 것).**

- 보드 잠금(`lock_board`)이 트랜잭션 안에서만 불린다는 것을 타입이 보장하지 않는다(M12). 새 호출을 넣을 때 사람이 지켜야 한다.
- 세션 API의 토큰 다이제스트와 시각 인자가 맨 문자열·정수다(M13).
- 사용자를 비활성화하는 함수가 없다. 넣을 때는 마지막 관리자 규칙 때문에 `instance` 줄을 잠가야 한다(M18).
- 따로 지운 블록의 파일은 수거되지 않는다(M5·S8의 나머지, 설계 5.7의 `file_refs`).
- SQLite에서 트랜잭션 밖의 단일 문장 쓰기는 대기열에 서지 않는다. 쓰기 트랜잭션이 5초 넘게 이어지면 `database is locked`가 날 수 있다.
- 세션 만료와 그 세션의 WebSocket 연결이 닫히는 때 사이에 최대 한 시간의 틈이 있다(S5).

**최소 역할에서 바뀐 것.** (2026-11-10 22:44 KST)

| 바뀐 것 | 왜 | 어디 |
|---|---|---|
| **보드에는 늘 최소 역할이 있다.** `viewer`, `commenter`, `editor` 가운데 하나이고 새 보드의 기본값은 `editor`다. 빈 값과 `admin`은 최소 역할이 아니다: 만들 때나 고칠 때 보내면 400 `invalid-board-minimum-role`. 스키마도 막는다(`NOT NULL DEFAULT 'editor'`, `CHECK`) | 결정(5.10). 누구나 참여해 관리자가 되는 보드는 두지 않는다 | `fvoci_model::MinimumRole`, `crates/store/schema/` |
| 멤버 추가(역할 관리 권한이 없는 추가자)와 참여(join) 모두 보드의 최소 역할을 준다. 전에는 빈 값일 때 추가는 viewer, join은 editor였다(S6 F절에 남겼던 불일치가 닫혔다). 역할 관리 권한이 있는 추가자는 전처럼 요청한 역할을 준다 | 결정(5.10) | `api/members.rs` |
| **다시 참여하면 최소 역할로 올린다. 내리지는 않는다.** 최소 역할보다 낮은 멤버(역할이 하나도 없는 멤버 포함)가 join하면 그 역할이 되고, 같거나 높은 멤버(관리자 포함)는 그대로다. 읽고 쓰는 것이 한 트랜잭션이라 동시에 온 join은 멤버십 하나를 만든다 | 결정(5.10) | `Store::join_board`, `Joined`, `App::join_board` |
| **멤버의 권한은 멤버가 가진 역할이다(해석, 확인 필요).** 전에는 보드의 최소 역할을 모든 멤버가 "가진 것으로" 쳤다(원본의 동작. 빈 값이 기본이라 평소에는 효과가 없었다). 이제 모든 보드에 최소 역할이 있고 기본이 `editor`라서 그 규칙을 두면 어느 보드에도 editor보다 낮은 멤버가 있을 수 없다: 관리자가 viewer로 추가해도 편집할 수 있고, "낮은 멤버가 다시 참여하면 올린다"는 결정이 뜻을 잃는다. 그래서 최소 역할은 "참여·추가가 주는 역할"로만 쓰고 권한 계산에서는 뺐다. 최소 역할을 올려도 이미 있는 멤버는 다시 참여할 때까지 그대로다 | 결정의 넷째 줄("역할 관리 권한이 있는 추가자는 요청한 역할을 준다")과 셋째 줄이 성립하려면 이 해석이어야 한다고 판단했다. 사용자가 다르게 뜻했다면 `member_has_permission` 한 곳을 되돌리면 된다 | `permissions::member_has_permission` |
| 멤버십 JSON에서 `minimumRole`이 빠졌다. 보드의 것이고 보드가 답한다. 전에는 읽은 멤버십에는 보드의 값이, 추가의 응답에는 빈 값이 들어 있었다 | 권한 계산에 쓰지 않는다. 같은 멤버십이 길에 따라 달리 보였다 | `fvoci_model::BoardMember` |
| `BoardRole`에 "없음"이 없다(viewer, commenter, editor, admin). 가져오기는 아카이브의 `minimumRole`이 빈 값이거나 이 서버가 모르는 값이면 기본값으로 둔다(원본이 쓴 아카이브는 빈 값이다) | 타입이 잘못된 상태를 담지 못하게 | `fvoci_model::BoardRole`, `app/import.rs`의 `with_a_minimum_role` |

**스냅샷.** 500 → 514단계. 시나리오 `members`에 14단계: 빈 값·`admin` 거절, 최소 역할보다 낮게 추가된 멤버가 그 역할대로 막히는 것, 다시 참여해 올라가는 것, 더 높은 멤버와 관리자가 그대로인 것. 기존 단계 가운데 62단계의 본문이 바뀌었다: 보드의 `minimumRole`이 `""`에서 `"editor"`로, 멤버십의 `minimumRole`이 없어졌다. 상태 코드가 바뀐 단계는 없고 권한 표 636칸도 그대로다. 테스트: API `boards::joining_again_raises_…`, `a_member_below_the_minimum_role_has_the_role_they_were_given` 등, 스토어 `boards::join_board_…`, `the_schema_refuses_a_minimum_role_that_is_none`, `joining_at_once_makes_one_membership`.

**표시 설정에서 바뀐 것.** (2026-11-10 22:31 KST)

| 바뀐 것 | 왜 | 어디 |
|---|---|---|
| **설정은 계정의 것이고 닫힌 집합이다.** `GET /users/me/settings`가 일곱 설정을 모두 답한다(저장된 값이 없으면 기본값): `timeZone`(`"auto"` 또는 IANA 이름), `weekStart`(auto·sunday·monday·saturday), `hourCycle`(auto·h12·h23), `theme`(auto·light·dark), `cardOpen`(center·side·full, 기본 center), `density`(auto·compact·comfortable), `haptics`(불리언, 기본 true). 뒤의 넷은 사용자가 구현 중에 더했다 | 설계 5.8의 C | `fvoci_model::{UserSettings, UserSettingsPatch}`(`crates/model/src/settings.rs`) |
| `PATCH /users/me/settings`: 보낸 멤버만 바꾸고 모든 설정을 답한다. `null`은 기본값으로 되돌린다. 모르는 멤버(`locale`, `timezone`)와 그 설정이 가질 수 없는 값은 400이고 `field`가 그 멤버다. 한 멤버라도 틀리면 아무것도 저장되지 않는다. 값은 대소문자를 구별한다 | 같은 곳. 계약이 설정을 전부 말한다 | `api/users.rs` |
| 시간대 이름은 바이너리에 넣은 IANA DB로 본다(`chrono-tz` 0.10). 옛 별칭(`Asia/Calcutta`)은 받아서 받은 그대로 저장한다. `"auto"`는 풀지 않는다 | 설계 5.8의 B. 배치마다 답이 같다 | `fvoci_model::TimeZone` |
| **화면 상태의 주머니에 한도가 있다.** 지금의 `config` 라우트(`GET /users/me/config`, `PUT /users/{id}/config`)가 그 주머니다. 키는 `[A-Za-z0-9_.-]` 1–64자, 값은 4 KiB까지, 사용자당 100개. 넘으면 400(`field: updatedFields`, 개수는 `limit: 100`). 개수는 변경과 같은 트랜잭션에서 센다. 목록은 이름순이다(전에는 "건드리지 않은 것 먼저") | 설계 5.8. 전에는 아무 이름, 아무 크기, 아무 개수가 들어갔다 | `UserPreferencesPatch::validate`, `StoreError::TooManyPreferences` |
| 저장: 둘 다 `preferences` 테이블, `category`가 `settings`와 `client`다(전에는 하나, `fvoci`). 같은 이름의 설정과 주머니 항목은 서로 다른 것이다. 이 빌드가 모르는 설정 이름이나 읽지 못하는 값의 행은 건너뛰고 그 설정은 기본값이다 | 설계 5.8. 다른 빌드가 쓴 값이 설정 전체를 깨뜨리지 않는다 | `Store::{get_user_settings, patch_user_settings}` |
| 오류 본문의 `limit`는 바이트(413)뿐 아니라 개수에도 쓴다 | 주머니의 한도를 알려 준다 | `AppError::with_limit` |

**스냅샷.** 470 → 500단계. 새 시나리오 `settings`(30단계)가 기본값, 일곱 설정의 변경과 되돌리기, 설정마다의 거절 문구, 주머니의 한도를 고정한다. 기존 단계 가운데 4단계의 본문이 바뀌었다(주머니 항목의 `category`가 `fvoci`에서 `client`로). 권한 표 636칸은 그대로다. 테스트: 모델 단위 `settings::tests` 8개, API `users::settings_*`와 `client_state_*` 3개, 스토어 2개.

**공유 토큰에서 바뀐 것.** (2026-11-10 22:20 KST)

| 바뀐 것 | 왜 | 어디 |
|---|---|---|
| **읽기 토큰은 서버가 만든다.** `POST /boards/{id}/sharing`의 본문은 `{"enabled": bool}`과 선택으로 `"regenerate": bool`이다. `token`이 있으면 400(`field: token`). `enabled`가 없거나 불리언이 아니면 400(`field: enabled`). 그 밖의 멤버(`id`, `modifiedBy` 등)는 보지 않는다. `null`인 멤버는 없는 멤버다 | 설계 3.8. 전에는 클라이언트가 정한 한 글자 토큰, 여러 보드에 같은 토큰, 빈 토큰이 저장됐다(S8 보안 결정 대기가 닫혔다) | `api/sharing.rs`의 `sharing_change` |
| 켤 때 토큰이 없으면 256비트 난수(43자)를 만든다. 이미 공유 중이면 토큰이 그대로다. `regenerate: true`면 새 토큰이고 옛 링크는 죽는다 | 설계 3.8 | `Store::set_sharing`, `SharingChange` |
| **끄면 토큰을 지운다(안 A).** 다시 켜면 새 토큰이고 껐던 링크는 되살아나지 않는다 | 설계 3.8의 권고 | 같은 곳 |
| 응답과 `GET`은 `{"enabled", "token", "updateAt"}`이다. `token`은 공유 중일 때만, `updateAt`은 한 번이라도 바뀐 적이 있을 때만 있다. 공유한 적 없는 보드는 `{"enabled": false}`. `POST`의 응답이 빈 객체에서 이 상태로 바뀌었다. `id`, `modifiedBy`, `update_at`(밑줄 표기)은 응답에 없다 | 설계 3.8. 토큰을 서버가 만드니 응답이 알려 줘야 한다 | `fvoci_model::Sharing` |
| 읽고 쓰는 것이 한 트랜잭션이다(보드 줄 잠금). 동시에 켠 요청들은 모두 저장된 하나의 토큰을 답받는다 | 각자 만든 토큰으로 서로 덮어쓰면 먼저 답받은 링크가 죽는다 | `Store::set_sharing` |
| 스키마: `sharing(board_id PK → boards CASCADE, enabled, token UNIQUE, modified_by, update_at)`. 토큰이 없는 것은 `NULL`이다 | 설계 3.8 | `crates/store/schema/` |

**스냅샷.** 455 → 470단계. 시나리오 `sharing`이 17 → 32단계: 요청의 토큰 거절, `enabled` 누락, 다시 켰을 때 토큰 유지, `regenerate`와 옛 토큰의 무효, 끈 뒤의 상태와 다시 켰을 때의 새 토큰. 서로 다른 토큰인지는 값이 정규화돼 스냅샷에 보이지 않으므로 비교 결과를 note 단계로 기록한다. 기존 단계 가운데 11단계의 본문이 바뀌었다(공유 응답의 모양, 읽기 토큰이 든 요청 경로). 권한 표 636칸은 그대로다. 테스트: API `sharing::*` 12개(다시 썼다), 스토어 `sharing::*` 4개(경합 하나 포함).

**관리자에서 바뀐 것.** (2026-11-10 22:05 KST)

| 바뀐 것 | 왜 | 어디 |
|---|---|---|
| **처음 가입한 사용자가 인스턴스 관리자다.** `users.is_admin`, 와이어 `isAdmin`(`GET /users/me`와 사용자를 답하는 모든 곳. 누가 관리자인지는 비밀이 아니다). 요청이 보낸 `isAdmin`은 쓰지 않는다 | 설계 3.6 | `fvoci_model::User::is_admin` |
| "처음"은 저장과 같은 트랜잭션에서 정한다. 한 줄짜리 테이블 `instance`의 `bootstrapped`를 켠 쪽이 첫 사용자다. 빈 인스턴스에 동시에 온 가입 가운데 하나만 관리자가 된다 | 사용자 수를 세어 정하면 둘이 함께 0을 본다 | `Store::{create_user, create_first_user, is_bootstrapped}` |
| **토큰 없는 가입은 첫 사용자만 된다.** 그 판정도 같은 트랜잭션이다. 전에는 "사용자가 0명인가"를 먼저 읽어서, 동시에 온 토큰 없는 가입이 모두 들어갈 수 있었다 | 가입 토큰의 뜻 | `Admission`, `StoreError::NotFirstUser` → 403 `signup_token_required` |
| `PUT /users/{id}/admin` `{"isAdmin": bool}`: 관리자만(아니면 403 `admin_required`). 없는 사용자는 404. 마지막 관리자는 해제할 수 없다(409 `last_admin`). 확인과 쓰기가 한 트랜잭션이라 둘이 동시에 물러나면 하나는 거절된다. 비활성화된 관리자는 세지 않는다. 응답은 바뀐 사용자다 | 설계 3.6. 양도는 "상대를 지정하고 나를 해제"의 두 번이다 | `Store::set_user_admin`, `AdminChange`, `api/users.rs` |
| **가입 토큰은 관리자의 것이다.** `GET /teams`, `GET /teams/{team}`의 `signupToken`은 관리자에게만 들어 있고 다른 사용자에게는 멤버 자체가 없다(전에는 로그인한 누구에게나 나갔다). `POST /teams/{team}/regenerate_signup_token`은 관리자만 하고(전에는 누구나) 응답이 새 토큰이 든 팀이다(전에는 빈 객체) | 설계 3.6. 토큰을 가진 사람은 누구든 가입한다 | `api/teams.rs`, `api/auth.rs`, `api::access::require_admin` |
| 관리자 소켓에 `POST /api/v1/admin/users/{username}/admin`: 그 사용자를 관리자로 만든다. 관리자가 아무도 로그인할 수 없을 때의 복구 수단이다 | 설계 3.6 | `api/admin.rs` |

**스냅샷.** 426 → 455단계. 새 시나리오 `admins`(29단계)가 첫 사용자, 가입 토큰의 조회와 재발급(역할별), 관리자 지정·해제의 거절과 순서를 고정한다. 기존 단계 가운데 8단계의 본문이 바뀌었다: 사용자 JSON에 `isAdmin`이 생겼고, 관리자가 아닌 사용자가 읽은 팀에서 `signupToken`이 빠졌다. 권한 표 636칸은 그대로다(가입 토큰 재발급은 표에 없는 라우트다). 테스트: API `admins::*` 6개와 소켓 1개, 스토어 `users::` 5개(경합 둘 포함: 동시에 저장한 사용자 가운데 관리자는 하나, 동시에 물러난 관리자 둘 가운데 하나는 남는다).

**삭제와 복원에서 바뀐 것.** (2026-11-10 21:49 KST)

| 바뀐 것 | 왜 | 어디 |
|---|---|---|
| **history 테이블이 없다.** `blocks_history`, `boards_history`와 그 위의 함수(`get_block_history`, `get_board_history`, `get_subtree`, `get_block_history_descendants`), 수정마다 한 줄씩 쌓던 기록, 같은 밀리초의 충돌을 피하던 장치를 모두 지웠다 | 설계 5.3의 C. 수정 이력을 쓰는 기능이 없다 | `crates/store/src/{blocks,boards,db}.rs` |
| **휴지통.** 보드와 블록의 테이블에는 살아 있는 것만 있다. 지우면 `deleted_boards`·`deleted_blocks`로 통째(JSON) 옮기고, 복원하면 되옮긴다. 한 번의 삭제로 함께 지워진 블록은 같은 `deletion_id`를 갖고 함께 돌아온다. 블록을 지우면 **아래의 모든 깊이**가 함께 간다(전에는 바로 아래 한 단계만) | 설계 5.3. 외래 키가 예외 없이 선다 | `crates/store/src/trash.rs` |
| 보드를 지우면 블록 전부, 멤버십, 파일 기록이 휴지통으로 가고, 공유, 사용자가 넣어 둔 카테고리 자리, 블록의 구독은 끝난다. 복원하면 보드, 그때 함께 지워진 블록, 파일 기록, 멤버십이 돌아온다. 공유, 카테고리 자리, 구독은 돌아오지 않는다. 사용자 행이 그사이 없어진 멤버십은 돌아오지 않고(비활성화된 사용자의 것은 돌아온다), 만든 사람의 행이 없어진 보드·블록은 만든 사람이 빈 채로 돌아온다 | 설계 5.3. **설계와 다른 점:** `deleted_boards`에 `files`(파일 기록)와 `deletion_id`, `team_id` 열을 더 뒀다. 파일 기록이 보드를 따라 지워지므로(CASCADE) 복원하려면 담아 둬야 한다 | `Store::{delete_board, undelete_board, get_deleted_board}` |
| **휴지통에 없는 것의 복원은 404다.** 삭제되지 않은 블록·보드의 undelete(전에는 200), 있었던 적 없는 것, 보드가 휴지통에 있는 블록, 놓여 있던 블록이 없는 블록 | 설계 5.3의 "없어지는 지금의 동작" | `App::{undelete_block, undelete_board}` |
| 보드 복원의 권한은 휴지통에 담긴 그때의 멤버십으로 본다. 삭제 권한이 있던 멤버만 되살린다. 다른 멤버는 403, 멤버가 아니던 사람과 휴지통에 없는 보드는 404 | 멤버십이 보드와 함께 휴지통에 간다. 전에는 지워진 보드의 멤버십이 남아 있었다(보존한 quirk가 닫혔다) | `api/boards.rs`의 `handle_undelete_board` |
| **보존 기간.** 휴지통의 것은 `trash_retention_days`(기본 30, 1 이상) 뒤에 영구히 지운다. 한 시간마다 돈다. 보드가 영구히 없어진 블록도 그때 지운다 | 결정(5.10) | `Store::purge_trash`, `lib.rs`의 `start_trash_purge` |
| **외래 키.** 두 DB 모두 강제한다(SQLite는 연결마다 `foreign_keys`를 명시해 켠다). 보드 → 팀, 블록 → 보드·부모 블록(같은 보드의 블록이어야 한다: `(board_id, parent_id)` 복합 키), 멤버십·공유·카테고리 자리·파일 기록 → 보드(CASCADE), 구독 → 블록·사용자(CASCADE), 세션·설정·카테고리·멤버십 → 사용자(CASCADE), 만든 사람·고친 사람 → 사용자. **설계와 다른 점:** 블록의 보드·부모 참조는 `RESTRICT`가 아니라 동작을 적지 않았다(`NO ACTION`). `RESTRICT`는 문장 안에서 바로 검사해서 부모와 자식을 한 문장으로 지울 수 없다. 스토어를 거치지 않는 삭제를 DB가 막는 것은 같다 | 설계 3.9·5.3 | `crates/store/schema/` |
| **부모.** 보드에 바로 달린 블록은 부모가 없다(`parentId`가 빈 값). 보드 ID를 부모로 보내면 비워서 저장하고 그렇게 답한다. `POST /boards/{id}/cards`도 그렇다. 부모는 같은 보드에 있는 블록이어야 한다: 없거나 다른 보드의 블록이면 400(`parentId`), 둘의 답은 같다. 수정이 블록을 자기 아래로 옮기면 400이다. 한 요청 안에서는 자식이 부모보다 먼저 와도 된다 | 외래 키. 다른 보드의 카드를 부모로 내세우던 것이 저장되지 않는다 | `blocks::insert_block`, `BlockError::{ParentNotOnBoard, ParentBelowBlock}` |
| **만든 시각은 서버가 정한다.** 새 블록의 `createAt`은 저장한 시각이고 응답에도 그 값이 나온다. 전에는 저장은 지금으로 하고 응답은 요청의 값을 돌려줬다(카드 생성의 응답과 저장 값이 달랐던 quirk가 닫혔다). 블록 목록은 만든 시각, 다음 ID의 순서다 | 목록 순서의 기준이다 | `blocks::insert_block` |
| **구독과 카테고리는 정말로 지운다.** 구독 해제는 줄을 지운다(다시 구독하면 새 구독이다). 구독된 상태에서 다시 구독하면 있는 구독을 답한다(만든 시각은 그대로, 알림 시각만 지금. 전에는 응답만 새 구독처럼 보였다). 블록이 지워지면 구독도 끝나고 복원해도 돌아오지 않는다. 카테고리 삭제는 줄을 지우고, 응답과 WebSocket 메시지는 지우기 직전의 모습에 `deleteAt`을 채운 것이다 | 설계 5.3 | `store/{subscriptions,categories}.rs`, `app/category.rs` |
| 파일 정보 JSON에서 `delete_at`, `archived`가 빠졌다(`id, create_at, name, extension, size`). 이미지 블록을 지워도 파일 기록은 남는다(보드와 함께 가고 함께 돌아온다). 파일 자체의 수거는 다음 묶음(5.7)이다 | 파일 기록에 삭제 표시가 없다 | `fvoci_model::FileInfo` |
| **멤버십 변경이 원자적이다.** 추가는 "없으면 넣고, 있으면 있는 것을 답한다"가 한 문장이다. 역할 변경과 제거는 "다른 관리자가 있는가"의 확인과 쓰기가 한 트랜잭션에서 보드 줄을 잠그고 한다. 관리자 둘이 동시에 물러나면 하나는 409 `last_admin`이다 | S5에서 미뤄 둔 경합 둘(멤버 추가, 마지막 관리자) | `Store::{add_member, change_member, remove_member}`, `MemberChange` |
| **보드마다 쓰기는 한 번에 하나다.** 블록의 쓰기·삭제·복원, 보드의 삭제, 멤버십 변경은 읽기 전에 보드 줄을 잠근다(PostgreSQL의 `FOR NO KEY UPDATE`. SQLite는 쓰는 트랜잭션이 처음부터 DB를 잡는다). 여러 보드에 걸친 쓰기는 ID 순서로 잠근다 | 하위 에이전트의 경합 테스트가 PostgreSQL에서 찾은 셋: 같은 블록의 동시 삭제가 500, 동시 복원이 500, 카드를 지우는 동안 그 아래에 넣은 블록이 어디에도 남지 않음(데이터 손실). 읽은 것과 지우는 것이 서로 다른 스냅샷을 봤다 | `boards::{lock_board, lock_boards}` |
| 보드가 지워지면 멤버였던 사람에게 알린다. 멤버십이 보드와 함께 가므로 지우기 전에 읽은 멤버에게 보낸다 | 전에는 지워진 보드의 멤버십이 남아 있어서 지운 뒤에 찾았다 | `ws.rs`의 `broadcast_board_delete`, `broadcast_block_delete_with_board` |
| `DELETE /boards-and-blocks`가 나열한 블록은 그 보드들의 블록인지만 확인한다. 보드가 모든 블록을 한 번의 삭제로 휴지통에 데려간다. "부모 다음에 나열한 블록은 못 찾아 실패"하던 quirk가 닫혔다 | 보드가 돌아올 때 함께 돌아와야 한다 | `store/boards_and_blocks.rs` |

**스냅샷.** 385 → 426단계. 새 시나리오 `trash`(41단계)가 블록 삭제의 실행 취소(깊은 자식, 자식으로 되살리기, 따로 지운 자식), 구독이 끝나고 돌아오지 않는 것, 보드 삭제와 복원(멤버·블록이 돌아오는 것, 역할별 403·404)을 고정한다. 기존 385단계 가운데 26단계가 바뀌었다. 상태 코드는 하나(`blocks`의 `undelete: again`이 200에서 404로)이고, 나머지 25단계는 본문만이다: 새 블록의 `createAt`이 요청의 `1`이 아니라 시각이고, 보드에 바로 달린 블록의 `parentId`가 비고, 파일 정보의 두 멤버가 빠졌다. 권한 표 636칸은 그대로다.

**테스트.** 스토어 184개(두 DB), API 448개. 스토어 테스트의 변환과 새 테스트는 하위 에이전트 셋에게 맡겼다(모두 Fable 5.2로 지정했고 각자 그 모델로 돌았다고 보고했다). 에이전트가 찾아 고친 것: 위의 경합 셋, `duplicate_board`가 부모보다 오래된 블록에서 실패하던 것(만든 시각 순서로 넣다가 부모가 아직 없었다. 부모 먼저 넣는다), 만든 사람의 행이 없어진 것을 되살릴 때의 500, 수정으로 부모의 순환을 만들 수 있던 것, 여러 보드에 걸친 쓰기의 교착 가능성. 두 스키마 파일이 열 타입 말고는 같다는 것을 테스트가 본다(`setup::the_two_baseline_schemas_differ_in_the_column_types_only`). 지운 테스트: history 자체의 테스트 아홉(스토어), 기록 없는 사용자의 세션 테스트 하나(API. 세션이 사용자를 외래 키로 가리켜서 그런 세션이 없다).

### K. 구현 6단계: 접두어 `/api/v1` (2026-11-11 00:33 KST)

설계 5.5의 판 번호 결정이다. 커밋 하나다.

| 바뀐 것 | 왜 | 어디 |
|---|---|---|
| 모든 API가 `/api/v1` 아래에 있다. 관리자 소켓의 라우트도 `/api/v1/admin/...`다 | 설계 5.5. 원본의 `/api/v2`를 쓰면 원본 클라이언트가 반쯤 되는 듯하다가 깨진다 | `lib.rs`의 `.nest("/api/v1", api)`, `api/admin.rs` |
| `/api/v2/...`는 무엇을 물어도 404 `not_found`(JSON)다. 웹 앱의 페이지로 떨어지지 않는다 | `/api` 아래의 모르는 경로는 전부터 JSON 404였다(`handle_fallback`). 새로 만든 동작이 아니라 고정한 동작이다 | `tests/api/paths.rs`, 회귀 `system` |

**스냅샷.** 554 → 558단계. 기존 554단계는 요청 줄의 접두어만 바뀌었다: 바꾸기 전 스냅샷의 `/api/v2`를 `/api/v1`으로 치환한 것과 새 스냅샷을 비교하면(`target/s6/golden_diff.py`) 바뀐 단계가 0이고 532줄이 그 치환이다. 새 단계 넷은 `system`의 `old prefix: client config`, `old prefix: login`, `old prefix: the prefix itself`, `the api root`이고 모두 404다. 권한 표는 경로를 담지 않아 그대로다(636칸).

**문서.** `docs/fvoci-cdn.md`의 경로를 바꿨다. 이 문서는 S6 이전의 인증(`Authorization: Bearer`, `FVOCIAUTHTOKEN` 쿠키, `access_token`)과 `trust_forwarded_for`를 설명하고 있어, 계약 단계 끝의 문서 정리에서 지금의 동작에 맞춘다. `website/`는 원본 제품의 문서라 손대지 않았다.

### L. 구현 7a단계: 경로와 이름 (2026-11-11 00:49 KST)

설계 1.5와 2.4가 "OpenAPI를 내면서 한 번에 정리"하라고 한 이름의 불규칙이다. 계약에 한 번 적히면 고치는 값이 커지므로 계약을 내기 전에 따로 커밋했다. 상태 코드(7b)와 목록 순서(7c)는 다음 커밋이다.

**경로.** 옛 경로는 모두 지워졌고 회귀 시나리오가 그 응답을 고정한다.

| 전 | 지금 | 비고 |
|---|---|---|
| `GET /clientConfig` | `GET /client-config` | |
| `POST /teams/{team}/regenerate_signup_token` | `POST /teams/{team}/signup-token` | |
| `POST /users/{user}/changepassword` | `POST /users/me/password` | 남의 비밀번호는 경로로 가리킬 수도 없다. 전의 "남의 ID면 403" 분기가 없어졌다 |
| `GET /users/me/config`, `PUT /users/{user}/config` | `GET`·`PATCH /users/me/client-state` | 본문이 고치는 것만 담으므로 `PATCH`다. 답은 `{이름: 값}` 객체다(전에는 `{user_id, category, name, value}`의 목록) |
| `POST /teams/{team}/{board}/files` | `POST /boards/{board}/files` | 파일은 보드의 것이다(S6 3단계). 팀은 경로에서 아무 일도 하지 않았다 |
| `GET /files/teams/{team}/{board}/{file}`, `.../info` | `GET /boards/{board}/files/{file}`, `.../info` | |
| `PUT /teams/{team}/categories/reorder` | `PUT /teams/{team}/category-order` | `reorder`가 카테고리 ID 자리의 예약어가 아니게 됐다 |
| `PUT /teams/{team}/categories/{category}/boards/reorder` | `PUT /teams/{team}/categories/{category}/board-order` | |

**질의 매개변수.** 모두 camelCase다: `blockId`, `parentId`, `perPage`, `disableNotify`, `readToken`, `sourceBoardId`(전: `block_id`, `parent_id`, `per_page`, `disable_notify`, `read_token`, `sourceBoardID`). 옛 표기는 모르는 매개변수라 무시된다(회귀 `blocks`의 `list: the old name of the parameter is no filter`, `sharing`의 `board with the token under the old name of the parameter`가 401).

**JSON 멤버.** 모두 camelCase이고 `Id`로 끝난다.

| 타입 | 전 | 지금 |
|---|---|---|
| 블록 일괄 수정 | `block_ids`, `block_patches` | `blockIds`, `blockPatches` |
| 보드·블록 묶음 수정/삭제 | `boardIDs`, `blockIDs` | `boardIds`, `blockIds` |
| 카테고리 | `userID`, `teamID`, 보드 항목의 `boardID` | `userId`, `teamId`, `boardId` |
| 사용자 | `create_at`, `update_at`, `delete_at` | `createAt`, `updateAt`, `deleteAt` |
| 파일 정보 | `create_at` | `createAt` |
| 구독 | `notifiedAt`이 0이면 생략 | 늘 보낸다 |
| 온보딩 답 | `teamID`, `boardID` | `teamId`, `boardId` |
| 웹소켓 `UPDATE_BOARD_CATEGORY` | `blockCategories`, 항목 `boardID`·`categoryID` | `boardCategories`, 항목 `boardId`·`categoryId` |
| 웹소켓 카테고리 보드 순서 | `CategoryId`, `BoardOrder` | `categoryId`, `boardOrder` |
| 웹소켓 클라이언트 설정 | `clientconfig` | `clientConfig` |

오류 본문의 `field`도 새 표기를 가리킨다(`userId`, `teamId`, `blockIds`, `perPage`). 오류 본문 자체의 `retry_after`, `request_id`는 설계 1.2가 정한 모양이라 그대로다.

**내 판단으로 한 것.** 설계가 하나씩 정하지 않은 것이다. 모두 serde 속성 한 줄이나 라우트 한 줄로 되돌릴 수 있다.

| 한 것 | 근거 |
|---|---|
| 저장하지 않는 멤버를 응답에서 뺐다: 사용자의 `nickname`, `firstname`, `lastname`, `is_bot`, `is_guest`, `roles`, `permissions`; 보드 멤버의 `roles`, `synthetic`; 카테고리의 `sorting`; 클라이언트 설정의 `telemetry`, `telemetryid` | 기준 스키마(3단계)에 그 열이 없어 늘 빈 값이었다. 계약에 적으면 웹 앱이 뜻 없는 멤버의 타입을 갖게 된다. Rust 타입의 필드는 남아 있다(`#[serde(skip)]`). 받을 때는 무시한다 |
| 카테고리 목록이 실제 `sortOrder`를 보낸다 | 전에는 원본을 따라 목록에서만 늘 0이었다. 호환을 끝냈으니 이유가 없다 |
| 로그인 요청의 `type`과 `mfa_token`을 없앴다 | 로그인 방식이 하나다. `type`이 `normal`이 아니면 400이던 검사가 없어져, `type`을 보내도 무시된다 |
| 사용자의 시각 멤버를 늘 보낸다 | 전에는 0이면 생략했다. 계약에서 "늘 있다"로 적기 위해서다 |

**스냅샷.** 558 → 571단계. 바꾸기 전 스냅샷과 비교해 기존 558단계에서 사라진 단계는 없고, 상태 코드가 바뀐 기존 단계는 둘이다.

| 단계 | 전 | 지금 | 왜 |
|---|---|---|---|
| `auth`의 `login: wrong type` | 400 | 200 | `type`을 읽지 않는다. 자격이 맞으니 로그인된다 |
| `auth`의 `login: empty body` | 400 `invalid login type` | 401 `incorrect login` | 같은 이유. 이름도 비밀번호도 없는 로그인은 틀린 로그인이다 |

나머지 변화는 위 표의 것뿐이다: 요청 줄 72개가 새 경로·새 매개변수로 바뀌었고(스크립트로 전·후를 짝지어 셌다), 본문에서는 옛 멤버가 빠지고 새 표기가 들어왔으며 `sortOrder` 17곳이 0에서 실제 값(10, 20, 30)이 됐다. `auth`의 `update another user's config`는 전에도 404(`user not found`)였고 지금은 경로가 없어 404(`route not found`)다. 새 단계 13개:

| 시나리오 | 단계 | 답 |
|---|---|---|
| `system` | `old path: client config` | 404 |
| `auth` | `old path: update my config`, `old path: my config`, `old path: change password` | 404 |
| `auth` | `my config after the old paths` | 200, 옛 경로가 아무것도 바꾸지 않았다 |
| `admins` | `old path: renew the token` | 404 |
| `files` | `old path: upload`, `old path: download`, `old path: info` | 404 |
| `categories` | `old path: reorder categories` | 404. 이제 ID가 `reorder`인 카테고리의 수정으로 읽히고 그런 카테고리는 없다 |
| `categories` | `old path: reorder boards` | 405. `.../categories/{category}/boards/{board}`의 경로와 겹치고 그 라우트에 `PUT`이 없다 |
| `blocks` | `list: the old name of the parameter is no filter` | 200, 거르지 않은 목록 |
| `sharing` | `board with the token under the old name of the parameter` | 401 |

권한 표 두 장은 바뀌지 않았다(636칸). SQLite와 PostgreSQL 모두 새 스냅샷과 일치한다.

**테스트.** API 테스트(`tests/api`)의 적응은 하위 에이전트(Claude Fable 5.2)가 했고 주 세션이 결과를 확인했다. 480 → 482개. 테스트를 지우지 않고 고정하는 것을 바꿨다: 남의 비밀번호 변경이 403이던 테스트는 "옛 경로는 자기 ID로도 남의 ID로도 404이고 비밀번호가 바뀌지 않는다"가 됐고, `reorder`가 예약어라는 테스트는 "여느 ID와 같다"가 됐다. 새 테스트는 옛 경로 아홉 개의 404 표(`paths.rs`)와 옛 표기의 질의 매개변수가 아무 일도 하지 않는다는 것(`blocks.rs`)이다. 에이전트가 서버에서 찾아 고친 것 둘: 카테고리 생성의 팀 불일치가 옛 표기(`teamID mismatch`)에 `field`도 없었고, 블록 일괄 수정의 길이 불일치에 `field`가 없었다. 주제가 없어져 뺀 것은 `visibility.rs`의 `PUT /users/{user}/config` 두 줄이다(사용자를 가리키는 경로가 없다. 옛 경로의 404는 `paths.rs`와 `users.rs`가 고정한다). `disableNotify`는 API로 보이는 효과가 없어 옛 표기와 구별하는 테스트를 두지 못했다.

스토어 테스트 하나(`a_board_comes_back_from_the_trash_with_every_member_it_had`)의 기대가 바뀌었다. 휴지통은 멤버십을 JSON으로 두는데 그 JSON에서 `roles`와 `synthetic`이 빠졌다. 둘은 저장한 적이 없는 값이라 잃는 것이 없다.

**남긴 불규칙.** 이번에 고치지 않았고 `docs/contract-pending.md`와 최종 보고의 남은 문제에 적는다: `/content-blocks/{block}/moveto/{where}/{dst}`(본문 저장 5.7과 함께 바뀐다), 아카이브 경로와 줄 형식(템플릿 변환 3.10), ID 목록을 받는 조회가 `POST`인 것(`POST /users`, `POST /teams/{team}/users`), `/subscriptions/{block}/{subscriber}`의 모양, 동작을 경로 끝에 붙인 라우트(`hide`, `unhide`, `join`, `leave`, `duplicate`, `undelete`, `onboard`), 구독자 종류 `channel`(서버는 `user`만 받는다), 늘 0인 `deleteAt`, 팀의 `title`·`settings`, `/ping`의 `db_type`.

### M. 구현 7c단계: 목록의 순서 (2026-11-11 01:08 KST)

설계 5.10의 결정이다. 기본은 만든 시각, 같으면 ID. 뜻에 맞는 순서가 따로 있는 목록은 그 순서를 쓰고 같을 때 이 기본으로 떨어진다. 순서는 스토어의 쿼리에 있다(핸들러가 다시 정렬하지 않는다). 계약(7d)의 라우트 설명에 같은 내용을 적는다.

| 라우트 | 순서 | 전 |
|---|---|---|
| `GET /teams` | 만든 시각, ID | 같음 |
| `GET /teams/{team}/boards` | 만든 시각, ID | 없음 |
| `GET /teams/{team}/boards/search`, `GET /boards/search` | 만든 시각, ID | 없음 |
| `GET /teams/{team}/templates` | 팀의 템플릿(만든 시각, ID) 다음에 내장 템플릿(만든 시각, ID) | 없음 |
| `GET /boards/{board}/members` | 사용자 ID. ID가 UUIDv7이라 계정을 만든 순서다 | 없음 |
| `GET /users/me/memberships` | 보드 ID. 보드를 만든 순서다 | 없음 |
| `GET /boards/{board}/blocks`, `GET /boards/{board}/cards` | `createAt`, ID. 카드의 쪽은 이 순서를 자른다 | 같음 |
| `GET /teams/{team}/users`, `POST /teams/{team}/users`, `POST /users` | 만든 시각, ID. ID 목록으로 물어도 물은 순서가 아니라 이 순서다. 검색은 맞는 사용자 가운데 가장 오래된 열 명이다 | 없음(검색의 열 명도 정해지지 않았다) |
| `GET /teams/{team}/categories` | 카테고리는 `sortOrder`, 같으면 만든 시각, ID. 카테고리 안의 보드는 사용자가 정한 순서, 같으면 보드 ID | 카테고리는 `sortOrder`, 이름, ID. 보드는 같을 때 순서 없음 |
| `GET /subscriptions/{subscriber}` | 만든 시각, 블록 ID | 같음 |

- **멤버십에는 시각이 없다.** `board_members`에 참여한 시각의 열이 없어 "참여한 순서"로는 줄 수 없다. 그래서 ID 순서다. 참여 순서가 필요해지면 열을 더하는 마이그레이션이 든다(남은 문제).
- **카테고리의 같을 때 기준을 이름에서 만든 시각으로 바꿨다(내 판단).** 이름의 순서는 DB의 정렬 규칙(collation)에 달려 SQLite와 PostgreSQL이 다를 수 있다. 같은 `sortOrder`는 스토어를 거치면 생기지 않는다(만들 때마다 다른 것들을 민다). 사용자 목록을 이름순으로 하지 않은 것도 같은 이유다.
- 알림을 받을 구독자의 목록(라우트가 아니다)도 같을 때 구독자 ID로 정했다.

**테스트.** `crates/store/tests/store/order.rs`(6개)가 규칙을 고정한다. 스토어로 넣은 뒤 스토어 밖에서 만든 시각과 정렬 값을 고쳐, 바라는 순서가 ID의 순서와도 이름의 순서와도 다르게 만든다. 스토어 테스트는 215 → 221개.

**회귀 하네스의 정렬을 없앴다.** 하네스는 객체의 목록을 정렬해 기록했고 그래서 순서가 틀려도 통과했다(`docs/handoff.md`). 이제 온 순서대로 기록한다. 예외로 남긴 라우트는 없다. 정렬을 없앤 채 SQLite에서 세 번, PostgreSQL에서 두 번 기록한 스냅샷 다섯 벌이 모두 같았다(`diff -rq`). 순서를 정하기 전에 같은 실험을 했을 때는 SQLite만으로도 실행마다 달랐고 두 DB가 서로 달랐다(F절). 템플릿 제목과 옛 형식 가져오기 뒤의 보드 제목을 정렬해 적던 관찰 둘도 온 순서대로 적는다. 남긴 정렬은 객체의 멤버 이름(`ping`, 템플릿)과 아카이브 ZIP의 항목 이름(저장 파일 이름이 난수다)이다.

**스냅샷.** 571단계 그대로. 56단계가 바뀌었고 바뀐 것은 목록 안의 순서와 그에 따라 매겨지는 `<id:N>` 번호뿐이다(전·후를 정렬하고 번호를 지워 비교하면 다른 단계가 0). 시나리오별로는 `trash` 19, `cards` 9, `blocks` 5, `archive` 4, `categories` 4, `search` 4, `files` 3, `members` 3, `boards` 2, `templates` 2, `boards_and_blocks` 1. 권한 표는 그대로다(636칸).

### N. 구현 7b단계: 201과 204 (2026-11-11 01:23 KST)

설계 1.5의 결정 B다: 결과가 없으면 204(본문 없음), 새로 만든 것은 201과 그 객체, 그 밖은 200. 전에는 결과가 없는 작업이 200과 `{}`로 답했다. 라우트마다의 적용은 설계가 하나씩 적지 않아 아래 표대로 내가 정했다.

| 답 | 라우트 |
|---|---|
| 201과 만든 것 | `POST /boards`, `POST /boards/{board}/duplicate`, `POST /boards/{board}/blocks`, `POST /boards/{board}/blocks/{block}/duplicate`, `POST /boards/{board}/cards`, `POST /teams/{team}/categories`, `POST /subscriptions`, `POST /boards/{board}/files`, `POST /boards-and-blocks`, `POST /register` |
| 204 | `POST /login`, `POST /logout`, `POST /users/me/password`(셋 다 `Set-Cookie`는 그대로), `DELETE /boards/{board}`, 블록의 `DELETE`·`PATCH`(하나, 여럿), `DELETE /teams/{team}/categories/{category}`, 카테고리에 보드 넣기와 `hide`·`unhide`, `POST /boards/{board}/leave`, `DELETE /boards/{board}/members/{user}`, `DELETE /subscriptions/...`, `DELETE /boards-and-blocks`, 콘텐츠 블록 옮기기, `POST /teams/{team}/archive/import`, 관리자 소켓의 두 라우트 |
| 200(그대로) | 모든 `GET`, 고친 것을 돌려주는 수정(`PATCH /boards/{board}`, `PATCH /cards/{card}`, 카테고리 수정과 순서, 멤버 수정, 설정), 이미 있을 수 있는 것을 정하는 요청(`POST /boards/{board}/members`, `join`, `sharing`), 복원(`undelete`), 온보딩, 가입 토큰 갱신 |

본문이 바뀐 곳:

| 라우트 | 전 | 지금 | 왜 |
|---|---|---|---|
| `POST /register` | 200 `{}` | 201과 만든 사용자 | 만든 것을 돌려준다는 규칙. 가입은 로그인시키지 않는다(쿠키 없음, 회귀 `auth`의 `register`가 고정) |
| `POST /boards/{board}/undelete` | 200 `{}` | 200과 되살린 보드 | 블록 복원이 블록을 돌려주는 것과 같게. 값이 손에 있었다 |
| `DELETE /teams/{team}/categories/{category}` | 200과 지운 카테고리 | 204 | 다른 `DELETE`와 같게 |

판단이 갈릴 수 있는 곳(남은 문제에도 적는다): 빈 목록을 넣는 `POST /boards/{board}/blocks`는 201과 `[]`다. 이미 있는 구독을 다시 만드는 `POST /subscriptions`도 201이다(라우트마다 성공 코드를 하나로 뒀다).

**스냅샷.** 571 → 573단계. 새 단계는 `auth`의 `register`(201, 사용자, `Set-Cookie` 없음)와 `login: the registered user`(204)다. 성공한 가입을 기록한 단계가 전에는 없었다. 기존 단계에서 200 → 201이 49개, 200 → 204가 34개이고 204는 본문과 `Content-Type`이 없다. 상태가 그대로인데 바뀐 단계는 9개다: 보드 복원 둘(본문이 보드가 됐다)과, `auth`에 사용자가 하나 늘어 `<id:N>` 번호가 밀린 일곱. 오류 응답은 하나도 바뀌지 않았다.

권한 표 두 장은 636칸 그대로이고 그 가운데 108칸의 값이 바뀌었다: 200 → 204가 58칸, 200 → 201이 50칸. 거절 칸(401, 403, 404)은 하나도 바뀌지 않았다(칸 단위로 비교했다).

**API 테스트.** 482 → 484건(하위 에이전트, Fable 5.2, 테스트 디렉터리만 고쳤다). 성공을 확인하는 자리 약 220곳이 `check_created`(201)와 `check_no_content`로 갈렸고, `check_no_content`는 상태 204, 빈 본문, `Content-Type` 없음 셋을 함께 본다. 새 테스트 둘: `a_registration_answers_the_user_that_was_made`(가입의 답이 멤버 여섯 개의 사용자이고 `GET /users/me`와 같으며 `Set-Cookie`가 없다), `an_answer_without_a_result_has_no_body_and_no_content_type`(블록 수정·삭제, 보드 삭제, 로그인, 로그아웃). 보드 복원은 200과 저장된 보드를, 카테고리 삭제는 204와 스토어에서 사라진 것을 고정한다. 카테고리 삭제가 답으로 주던 `deleteAt`은 이제 WebSocket의 `UPDATE_CATEGORY`에서만 보인다. 사라진 단언은 "카테고리에 보드 넣기의 답이 `application/json`이다" 하나다(204에는 본문이 없다).

`docs/fvoci-cdn.md`를 지금 동작에 맞췄다(쿠키 이름, `Origin`·`X-Requested-With` 규칙, `readToken`, `trusted_proxies`, `allowed_origins`, 로그인 204, 새 파일 경로).

### O. 구현 7d단계: 계약 `contracts/openapi.json` (2026-11-11 02:07 KST)

설계 5.5의 결정 C다. 계약은 OpenAPI 3.1.0 문서 `contracts/openapi.json`이고 서버의 라우트 정의에서 나온다. 도구는 utoipa 6.0.0과 utoipa-axum 0.3.0이다(사용자 결정, `7002d24c`. 함께 들어온 크레이트는 utoipa-gen 6.0.1, pastey 0.2.3. `NOTICE`를 다시 만들었고 cargo-deny가 통과한다).

**어떻게 만드는가.**

- 핸들러마다 `#[utoipa::path(...)]`가 경로, 이름(`operationId`), 매개변수, 본문, 답을 적는다. 핸들러의 문서 주석 첫 줄이 요약이고 나머지가 설명이다. `routes()`는 `routes!(...)`로 핸들러를 넣으므로 **실제 라우터와 계약이 같은 정의에서 나온다**(`api/mod.rs`의 `routes()` 하나를 `router()`와 `contract::document()`가 함께 쓴다). 주석을 달지 않은 핸들러는 라우터에 들어갈 길이 없다. 계약 밖에 남긴 것은 `/ws`, 정적 파일, 관리자 소켓의 두 라우트다.
- 경로 매개변수의 이름을 계약의 표기(`{boardId}`)로 맞췄다. axum의 경로 문자열에서 이름만 바뀐 것이라 바깥에서는 보이지 않는다. 구독의 두 경로는 라우터에서 첫 자리의 이름이 하나(`{blockOrSubscriberId}`)여야 해서 문서를 만든 뒤 `{blockId}`, `{subscriberId}`로 고쳐 적는다(`contract::RENAMED_PATHS`).
- 스키마는 와이어 타입(`fvoci-model`)의 `ToSchema`에서 나온다. 이 타입들은 너그럽게 읽으려고 멤버마다 기본값을 두는데, 그대로 내면 모든 멤버가 "없을 수 있음"이 된다. 그래서 문서를 만든 뒤 JSON 단계에서 고친다(`api/contract.rs`): 기본값을 지우고, 표 `SCHEMAS`가 스키마마다 늘 있는 멤버를 정한다(답으로 나가는 엔티티는 전부, 패치는 없음, `Sharing`의 `token`·`updateAt`과 `Team`의 `signupToken`은 빠질 수 있음). 표에 없는 스키마가 생기거나 표에만 있는 이름이 남으면 문서를 만드는 일이 실패한다.
- 만들기와 고치기의 요청은 답과 모양이 달라 문서 전용 타입으로 따로 적었다(`api/contract/bodies.rs`의 `NewBoard`, `NewBlock`, `NewBoardsAndBlocks`, `NewCard`, `NewCategory`, `CategoryUpdate`, `NewMember`, `MemberRoles`, `SharingRequest`, `NewSubscription`, `AdminChange`, `SettingsChange`, `FileForm`). 서버는 이 타입으로 해석하지 않는다. **손으로 쓴 것이라 해석하는 쪽과 어긋날 수 있다**(남은 문제).
- 멤버를 정하지 않은 객체(`fields`, `properties`, `cardProperties`의 항목, 팀의 `settings`)는 `additionalProperties: true`로 적는다. 아무 말도 없으면 타입 생성기가 "늘 빈 객체"로 읽는다.
- 인증은 문서 전체의 기본이 `sessionCookie` 또는 `bearerToken`이다. 세션이 필요 없는 다섯(`login`, `register`, `getClientConfig`, `hello`, `ping`)은 `security: [{}]`, 읽기 토큰으로도 되는 넷(`getBoard`, `getBlocks`, `getFile`, `getFileInfo`)은 셋 가운데 하나다. CSRF 헤더와 `Origin` 규칙은 머리말에 글로 적었다.
- 오류는 조작마다 4xx를 늘어놓지 않고 모든 조작의 `default`가 `ErrorBody`다. 429와 `Retry-After`만 `login`, `register`, `changePassword`에 따로 적었다.
- 출력은 멤버 이름순, 두 칸 들여쓰기, 끝에 줄바꿈이다. 같은 서버는 늘 같은 바이트를 낸다.

**수치.** 경로 56개, 조작 73개(`/api/v1` 아래 71개와 그 밖의 `/hello`, `/ping`), 스키마 51개. 메서드별로 GET 27, POST 26, PATCH 7, PUT 7, DELETE 6. 파일은 5,500줄 남짓이다.

**낡았는지 검사.** 서버 바이너리의 `--print-openapi`가 설정을 읽기 전에 문서를 찍고 끝난다. `cargo xtask openapi`는 그 출력과 파일을 비교해 다르면 첫 번째로 다른 줄을 대며 실패하고, `--write`가 파일을 고쳐 쓴다. `cargo xtask check`에 `contract` 단계로 들어갔다. 파일이 없을 때, 파일의 한 줄을 바꿨을 때 실패하고 되돌리면 통과하는 것을 확인했다.

**목록의 순서.** M절의 표가 조작의 설명(또는 답의 설명)에 그대로 있다. 계약 모듈의 테스트 `every_list_says_its_order`가 목록을 답하는 조작 21개마다 순서를 말하는 문구가 있는지와, 목록을 답하는 조작이 표에 빠짐없이 있는지를 본다. 순서 자체는 M절의 스토어 테스트와 회귀 스냅샷이 고정한다.

**테스트.** 계약 모듈 11개(문서가 만들어진다, 모든 조작의 `default`가 `ErrorBody`, 세션 없는 조작의 표기, 답은 모든 멤버·패치는 없음, 구독 경로의 이름, 목록의 순서, 기본값 제거, 열린 객체, 없는 멤버를 필수로 둘 수 없음, 멤버 이름순), xtask 4개(첫 번째로 다른 줄).

**바깥 도구로 본 것.** `openapi-typescript` 7.13.0이 이 파일에서 타입 4,654줄을 냈다(확인용으로 `target/` 아래에만 썼다. 타입 생성은 웹 트랙의 일이다). `@redocly/cli` 1의 `lint`는 "valid"이고 경고 70건이 남는다. 모두 `operation-4xx-response`(조작에 4xx 응답이 없다)이고 `default`로 적기로 한 결과다.

**응답은 바뀌지 않았다.** 회귀 스냅샷 573단계와 권한 표 636칸이 그대로 통과한다(스냅샷 파일의 변경 없음). `onboard`와 `uploadFile`의 답을 `json!`에서 타입(`OnboardingStart`, `UploadedFile`)으로 옮겼고 멤버의 순서까지 같게 뒀다.

**주석을 달며 고친 문서 주석.** 코드와 달랐던 것들이다: 카테고리 보드 순서 바꾸기의 오류(내부 오류가 아니라 404·400), 블록 삭제의 범위(바로 아래가 아니라 아래 전부), 팀의 ID가 `0`이라는 말.

**실수 하나.** 스키마를 시험하던 임시 테스트 파일을 지우면서 `crates/model/tests` 디렉터리를 통째로 지웠다(2026-11-11 01:46 KST). 커밋된 파일 다섯 개였고 그 사이 고친 것이 없어 `git checkout HEAD -- crates/model/tests`로 되돌렸다(02:00 KST쯤). 그 사이의 `cargo check`와 단위 테스트는 이 다섯 파일 없이 돌았고, 이 절의 최종 `cargo xtask check`와 `cargo xtask test`는 되돌린 뒤에 돌렸다.

### P. 마지막 리뷰와 반영 (2026-11-11 02:55 KST)

`248abd88`까지를 네 갈래로 검토했다. 모두 `general-purpose` 하위 에이전트이고 모델을 Fable 5.2로 지정했으며, 네 보고 모두 자기 모델을 Fable 5.2(`claude-fable-5-2`)라고 적었다. 넷 다 읽기만 했고 `cargo`는 돌리지 않았다. 아래의 "확인"은 이 세션이 코드를 직접 읽거나 테스트로 본 것이고, 그렇지 않은 것은 "보고대로"라고 적는다.

| 갈래 | 무엇을 | 지적 |
|---|---|---|
| A1 | rust-review(스킬을 불러 따름): 인증 전달, 세션, 로그인 제한 | Must 1, Should 5, Consider 11 |
| A2 | 같은 범위의 보안 관점 | Must 1(A1과 같은 것), Should 5, Consider 9 |
| A3 | rust-review(스킬을 불러 따름): 6단계와 7a–7d(`4a4feb69..248abd88`) | Must 1, Should 8, Consider 8 |
| A4 | 계약 파일과 코드의 대조(조작 73개 전부) | 틀림 13, 빠짐 25, 표현 5, `contract-pending.md` 3 |

**반영한 것.**

- **계정이 있는지를 제한이 알려 주던 것(A1·A2의 Must).** 로그인 제한의 계정 키가 조회 결과에 달려 있었다(있는 계정은 ID, 없는 계정은 적은 이름). `alice `(끝에 빈칸)로 실패를 쌓은 뒤 `alice`가 기다려야 하는지를 보면 계정이 있는지 알 수 있었다. 이제 키는 요청이 적은 이름(사용자 이름과 이메일, 각각)에서만 나오고, 조회보다 먼저 묻는다. 성공은 계정을 찾은 그 이름의 기록만 지운다(자기 계정으로 로그인하며 남의 이메일을 함께 적어 그 기록을 지울 수 없다). 값: 한 계정을 이름으로도 이메일로도 찌르면 실패 한도가 따로 센다. 테스트 `who_has_to_wait_does_not_tell_which_names_are_accounts`(API), 리미터 단위 테스트 둘, 회귀 단계 둘.
- **비밀번호 바꾸기가 제한 밖이던 것(A1·A2의 Should).** 세션을 가진 쪽이 옛 비밀번호를 한도 없이 찍을 수 있었다. `changePassword`가 그 계정 이름의 로그인 실패와 같은 기록을 쓴다. 틀리면 실패로 세고, 넘으면 429와 `Retry-After`다. 테스트 `the_old_password_is_not_guessed_at_through_a_session`, 회귀 시나리오 `password_limits`(10단계).
- **WebSocket 핸드셰이크가 세션 사용을 기록하던 것(A1의 Should).** 핸드셰이크의 답은 쿠키를 갱신하지 못하는데 사용 시각은 기록돼서, 다음 요청이 갱신을 건너뛰고 쿠키가 세션보다 먼저 끝날 수 있었다. 기록 없이 찾는 `Authenticator::find_session`을 두고 핸드셰이크와 연결 재확인이 그것을 쓴다. 테스트는 `the_cookie_is_renewed_when_the_use_of_the_session_is_recorded`에 더했고, 예전 코드로 돌리면 실패하는 것을 확인했다.
- **`X-Forwarded-For`(A1의 Should·Consider).** 글자로 읽히지 않는 바이트가 있으면 헤더 줄 전체를 버리던 것을, 쉼표로 나눈 항목 단위로 읽어 그 항목에서 멈추게 했다. `주소:포트`, `[v6]:포트` 항목을 받는다. `::ffff:10.0.0.0/104` 같은 IPv4 대응 범위를 그 IPv4 범위로 읽는다(전에는 어느 주소와도 맞지 않았다). `Forwarded` 헤더는 읽지 않는다고 주석에 적었다.
- **기다림 설정의 상한(A1의 Should).** 아주 큰 값이 리미터의 잠금 안에서 `Instant` 덧셈을 넘쳐 패닉이 났다. 세 설정(`login_pair_base_delay_seconds`, `login_pair_max_delay_seconds`, `login_account_lock_seconds`)을 10년 이하로 검사하고, 리미터도 같은 값으로 자른다.
- **리미터가 가득 찼을 때(A1의 Consider).** 아무 십분의 일이 아니라 가장 오래 건드리지 않은 십분의 일을 버린다(설계 5.2의 말대로).
- **프록시 뒤인데 `trusted_proxies`가 빈 경우(A2의 Should).** `serverRoot`가 https이고 `usessl`이 꺼져 있고 `trusted_proxies`가 비면 시작할 때 경고한다. 모든 클라이언트가 프록시의 주소 하나로 보여 로그인 제한이 모두에게 한꺼번에 걸리기 때문이다.
- **없는 계정의 해시(A1의 Consider).** 대신 쓸 해시를 만들지 못했을 때 빈 문자열과 견주어 금방 끝나던 것을, 같은 일을 다른 길로 하게 했다. 비밀번호 작업이 끝나지 못한 까닭과 WebSocket이 세션을 읽지 못한 까닭을 로그에 남긴다.
- **통과만 하던 테스트(A1의 Should).** `a_message_does_not_sign_a_connection_in`이 서버가 메시지를 읽기 전에 알림을 한 번 보내고 끝나서, `AUTH`가 로그인을 시켜도 통과했다. 같은 사용자의 연결이 알림을 받는 것을 옆에서 확인하며 다섯 번 되풀이한다.
- **한 요청으로 만든 블록의 순서(A3의 Should).** 새 ID를 `HashSet`의 순서로 나눠 줘서, 같은 밀리초에 저장된 블록의 목록 순서가 실행마다 달랐다. 고치며 더 찾은 것: 블록마다 시각을 따로 찍고 부모부터 저장해서, 밀리초가 어디서 넘어가느냐에 따라서도 순서가 달라졌다. 이제 ID는 요청의 순서로 만들고 한 요청의 블록은 시각 하나를 쓴다. 목록(`createAt`, ID 순)이 요청의 순서가 된다. 테스트 `post_blocks_without_identifiers_and_in_the_order_of_the_request`, 모델 테스트 둘, 회귀 단계 넷.
- **`id` 없는 블록(A4).** 계약은 `NewBlock.id`를 선택이라 했는데, `id` 없는 블록 둘이 ID 하나를 나눠 받고 부모 없는 블록이 그 ID를 부모로 받아 400이 났다. 빈 ID를 참조로 치지 않게 고쳤다.
- **`createBoardsAndBlocks`의 거절(A3의 Should).** 옛 철자(`UpdateAt`, `BoardID`)와 `field` 없음을 `createBlocks`와 같게 맞췄다.
- **계약의 틀린 말(A3의 Must, A4의 틀림 13).** 설정의 낱말 다섯 집합이 `Auto`, `Light`처럼 대문자로 적혀 있었다(서버는 소문자만 보내고 받는다. 매크로가 쓰는 `rename`을 파생이 읽지 못했다. 스키마를 직접 쓴다). `updatedProperties`는 값을 더하지 않고 전체를 바꾼다. `getFile`은 모든 파일을 `application/octet-stream`과 내려받기로 보내고 412·416은 `ErrorBody`가 아니다. `field=property_name`은 보드 자신의 `properties`의 멤버 이름을 본다. `duplicateBoard`는 멤버가 한다. 편집자가 `schemeAdmin`을 청하면 403이다. 템플릿은 기본 카테고리에 넣지 않는다. 429에는 `reason`이 없다. 모르는 멤버를 거절하는 조작이 넷 있다. `fileId`는 UUID가 아니다. `OPTIONS`도 헤더 검사를 받지 않는다.
- **계약에 빠져 있던 것(A4의 빠짐 25, 표현 5).** 조작의 설명에 넣었다: 거절의 상태와 `reason`(`last_admin`, `admin_required`, 틀린 옛 비밀번호의 400), 입력의 조건, 지운 보드와 함께 끝나는 것(공유, 사이드바의 자리, 구독), 복사본의 제목과 종류, 가져오기가 전부 아니면 전무가 아닌 것, 검색이 맞추는 방식, 온보딩이 남기는 클라이언트 상태. 서버의 결함에 가까운 것은 조작의 설명이 아니라 `docs/contract-pending.md` 8절에 모았다.
- **`Origin: null`(A1의 Consider).** 서버가 페이지에 `Referrer-Policy: no-referrer`를 붙이므로, `cors`가 아닌 방식의 바꾸는 요청에는 브라우저가 `Origin: null`을 적고 서버가 거절한다. Fetch 표준(2026-10-19판, 2026-11-11 02:41 KST에 받아 `target/s6/research/raw/fetch-standard.html`에 둠)의 "append a request `Origin` header"에서 확인했고 계약의 머리말에 적었다. 브라우저로 직접 해 보지는 않았다.
- **계약을 만드는 단계(A3의 Should).** 경로 아래에 직접 적힌 스키마에도 기본값 지우기와 열린 객체 처리를 한다. `additionalProperties` 안의 객체도 연다. 경로에 없는 경로 매개변수를 적은 조작과, 문서에 없는 보안 방식을 적은 조작을 거절한다. 테스트: 세션이 필요 없는 조작 다섯과 읽기 토큰 조작 넷이 정확히 그것뿐인지, 필수 멤버의 표가 서버가 실제로 쓰는 멤버와 같은지(기본값을 직렬화해 견준다), 닫힌 낱말 집합이 서버가 쓰는 낱말인지, 계약에 맞지 않는 조작이 까닭과 함께 거절되는지.
- **그 밖(A3).** `.gitattributes`에 `contracts/openapi.json text eol=lf`. `cargo xtask openapi`의 차이 메시지가 줄을 다듬지 않고 보인다. 관리자 소켓의 경로가 `PREFIX`를 쓴다. 엉뚱한 상수에 붙은 문서 주석, 쓰지 않는 `Serialize`·`Deserialize` 파생, 낡은 주석 여럿.

**남긴 것과 까닭.**

| 지적 | 남긴 까닭 |
|---|---|
| 계정 전체 잠금을 잠금 도구로 쓸 수 있다(주소 17개쯤이면 한 계정을 계속 잠근다, A2) | 설계 5.2가 승인한 수(계정 100회, 15분)의 성질이다. 기기 쿠키 같은 대안은 설계 결정이다 |
| IPv6 `/64` 여럿으로 해시 대기열을 채울 수 있다(A2, 산수는 보고대로) | 더 넓은 범위의 층이나 전체 예산이 필요하다. 설계 결정이다. CDN·WAF의 제한이 그 앞의 겹이다(`docs/fvoci-cdn.md`) |
| 새 인스턴스에서 처음 가입한 사람이 관리자다(A2) | 설계 3.6의 결정이다 |
| 쿠키 갱신이 더 새 쿠키를 덮을 수 있다(A1) | 비밀번호를 바꾼 답과 갱신이 겹치는 좁은 틈이고, 덮이면 다시 로그인하면 된다. 갱신에 세션의 토큰을 견주는 일은 다음 묶음으로 둔다 |
| 평문 HTTP에서 같은 이름의 쿠키가 여럿이면 첫 것을 쓴다(A1) | HTTPS에서는 `__Host-` 접두사가 막는다. 평문 HTTP는 개발과 내부망용이다 |
| `Result<_, String>` 둘(`token.rs`, `client_address.rs`), `lib.rs`의 `unwrap_or_default()`(A1) | 시작할 때 한 번 읽는 설정의 오류이고 메시지가 곧 사용자에게 가는 글이다. 앱 코드라 그대로 둔다 |
| 사용자 조회의 두 번째 시도가 내는 시간 차(A2), 관리자 소켓의 bind와 chmod 사이(A2, 보고도 미확인), 사용자 이름에 `@`를 허용(A2), `email_taken`이 가입 토큰을 가진 사람에게 이메일의 존재를 알림(A2) | 확인하지 못했거나 제품 결정이다. 열린 문제로 넘긴다 |
| CSP·`frame-ancestors`·HSTS가 없다, 모든 인터페이스에 bind(A2) | 정적 파일을 서버가 주지 않게 되는 웹 트랙과 배포 문서(`docs/fvoci-cdn.md`)의 몫이다 |
| 세션이 끝난 뒤 WebSocket이 한 시간까지 남는다, 로그인이 바뀌는 쿠키의 옛 세션을 끝내지 않는다(A2) | 앞의 것은 J절에 적은 것과 같다. 뒤의 것은 다음 묶음으로 둔다 |
| `allowed_origins`에 https가 있고 `serverRoot`가 http면 쿠키가 `Secure`가 아니다(A2) | `serverRoot`가 서버에 닿는 주소라는 전제다. 설정 검사에 넣을 후보다 |
| `AsWritten` 스키마도 기본값을 지운다(A3) | 지금 기본값을 일부러 적은 스키마가 없고, 계약의 테스트가 어디에도 기본값이 없음을 고정한다 |
| 쿼리 매개변수의 이름, 상태, 본문을 주석과 코드에 두 번 적는다(A3) | 응답은 회귀 스냅샷이, 매개변수는 API 테스트가 고정한다. 한 정의로 모으는 일은 요청 스키마를 손으로 쓴 것(O절)과 함께 다음에 본다 |
| `sorted()`가 지금은 아무것도 바꾸지 않는다(A3) | `serde_json`의 `preserve_order`를 의존성이 켜는 날을 위한 것이라고 주석에 적고 뒀다 |
| 테스트 보강 몇(A1: 다중 부분 요청의 헤더 검사, 세션 경쟁 테스트의 단언, 가입이 시도로 세지 않는다는 단언) | 동작은 다른 테스트가 고정한다. 다음에 본다 |
| 카드가 아닌 모양의 카드 블록이 `getCards`를 500으로 만든다, 파일 없는 이미지 블록이 내보내기를 500으로 만든다, SQLite의 `property_name` 검색(A4) | 이 묶음 전부터 있던 동작이다. `docs/contract-pending.md` 8절에 적었고 다음 묶음에서 고칠 후보다 |

**응답의 변화.** 회귀 스냅샷 573 → 589단계(시나리오 20 → 21), 권한 표 636칸 그대로. 바뀐 기록은 한 단계다: `createBoardsAndBlocks`가 다른 보드의 블록을 거절하는 메시지가 `invalid BoardID … (not exists in the created boards)`에서 `invalid boardId … (no board of the request has that id)`로 바뀌고 `field: boardId`가 붙었다. 새 단계 16: `blocks` 4(ID 없는 블록, 자식을 부모보다 먼저 적은 요청, 그 목록), `login_limits` 2(끝에 빈칸을 붙인 이름), `password_limits` 10(새 시나리오). 계약 파일은 설명이 바뀌었고 조작과 스키마의 수(경로 56, 조작 73, 스키마 51)는 그대로다. `getFile`에 412와 416이 더해졌고 설정의 낱말 집합이 소문자가 됐다.

**검증(2026-11-11 02:52 KST).** `cargo xtask check` 일곱 단계 PASS. `cargo xtask test`: SQLite 991건, PostgreSQL 883건, S3 1건, 실패 0(SQLite 62.9초, PostgreSQL 102.9초). API 테스트 487건, 회귀 32건, 서버 단위 125건, xtask 21건.

### Q. `hello`와 `ping`도 `/api/v1` 아래로 (2026-11-11 03:08 KST)

6단계(K절)는 `/hello`와 `/ping`을 상태 확인용이라 보고 접두어 밖에 뒀다. 그런데 둘은 계약에 조작으로 올라 있고, 설계 2절의 경로 표에는 API 밖의 주소로 `/ws`와 `/static`만 있다. "모든 API가 `/api/v1` 아래"라는 기준에 맞지 않아 옮겼다.

| 바뀐 것 | 어디 |
|---|---|
| `GET /api/v1/hello`, `GET /api/v1/ping`. 세션이 필요 없는 라우트 묶음(로그인, 가입과 같은 묶음)에 넣었다 | `api/mod.rs`의 `routes()`, `api/system.rs` |
| 접두어 밖의 라우터(`system_router`)를 없앴다. 계약도 접두어 아래의 라우트 하나에서만 만든다 | `lib.rs`, `api/contract.rs` |
| 예전 주소 `/hello`, `/ping`은 다른 모든 주소처럼 웹앱의 화면 주소다(설계 2절의 "그 밖의 GET·HEAD"). 404가 아니라 웹앱의 페이지가 나간다 | `tests/api/paths.rs` |

**스냅샷.** 589 → 591단계, 권한 표 636칸 그대로. 바뀐 기록 하나: `system`의 `hello`(`GET /hello`)가 `text/plain`의 `Hello`에서 웹앱의 페이지(`text/html`)가 됐다. 새 단계 둘: `ping: at the root`(웹앱의 페이지), `hello: under the prefix`(`Hello`). `ping`의 멤버를 보는 관찰 단계는 새 주소에서 읽는다.

**계약.** 경로 56개가 모두 `/api/v1` 아래다. 조작 73, 스키마 51은 그대로다.

**고치며 본 것.** 기준 경로에 끝 슬래시를 준 서버의 테스트(`startup_refuses_a_malformed_base_path`의 끝)가 `/boards//hello`를 물어 웹앱의 페이지 200으로 통과하고 있었다. API의 답(`Hello`)을 보게 고쳤다.

**알아 둘 것.** 바깥의 상태 확인(로드 밸런서 등)이 `/hello`나 `/ping`을 보고 있었다면 주소를 바꿔야 한다. 저장소 안에는 그런 설정이 없다(`docker/`, `docs/fvoci-cdn.md`에 없음).

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


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
| (이 커밋) | **서버가 만드는 공유 토큰(3.8).** 아래 "공유 토큰에서 바뀐 것" | 455 → 470단계(시나리오 `sharing`에 15단계). 기존 단계 가운데 11단계, 본문만 |

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
| 관리자 소켓에 `POST /api/v2/admin/users/{username}/admin`: 그 사용자를 관리자로 만든다. 관리자가 아무도 로그인할 수 없을 때의 복구 수단이다 | 설계 3.6 | `api/admin.rs` |

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


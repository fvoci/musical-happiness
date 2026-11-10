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

| 도구 | 위치 | 용도 |
|---|---|---|
| 오라클 | `target/oracle/go-server`, 실행은 `scripts/oracle-run.sh <name> <port> [single-user 토큰] [dbtype] [dbconfig]` | 원본 서버를 Go 1.21.8, 태그 `json1 sqlite3`로 빌드한 바이너리. 차등 비교, 스키마 비교, DB 왕복에 쓴다 |
| PostgreSQL | 컨테이너 `hafgufa-pg` (127.0.0.1:15433, PostgreSQL 18.6) | 스토어·서버 테스트의 PostgreSQL 실행. `FVOCI_STORE_TEST_DB_TYPE=postgres`, `FVOCI_STORE_TEST_CONN_STRING=postgres://fvoci:fvoci-test@localhost:15433/fvoci?sslmode=disable` |
| 원본 웹 번들 | `webapp/pack` | `npm ci && npm run pack` (Node 26.13.0에서 빌드됨) |
| Cypress | `scripts/e2e-cypress.sh <label> <server>` | 원본 스펙 6개(테스트 11개)를 임의의 서버 바이너리에 실행. 결과는 `target/e2e/<label>/` |

| 포트 실행 | `scripts/fvoci-run.sh <name> <port> [single-user 토큰] [dbtype] [dbconfig] [filespath]` | 오라클 실행 스크립트와 같은 인자로 Rust 서버를 띄운다 |
| 차등 비교 | `scripts/differential/run.py --left <원본 URL> --right <포트 URL>` | 같은 요청 시퀀스(시나리오 15개)를 두 서버에 보내 상태 코드와 JSON을 비교한다. ID·시각·비밀값은 정규화한다 |
| 권한 표 | `scripts/differential/permissions.py --left … --right …` | 라우트 × 역할 × 보드 종류별 상태 코드를 비교한다 |
| DB 왕복 | `scripts/differential/roundtrip.sh sqlite` / `postgres <URL>` | 한 서버가 쓴 DB와 파일을 다른 서버로 읽고 쓴다 |
| S3 호환 서버 | 컨테이너 `hafgufa-s3` (SeaweedFS, 127.0.0.1:18333, 버킷 `fvoci-test`) | S3 파일 백엔드 테스트 |
| 라이선스 | `cargo deny check`(`deny.toml`), `cargo about generate …`(`about.toml`, `about.hbs`) | 의존성 라이선스·권고 검사, `NOTICE` 생성. 도구는 `target/tools/bin`에 설치했다 |

등록된 의도적 차이는 `scripts/differential/expected.py`에 있다. 빌드 산출물, 오라클의 Go 빌드 캐시, 테스트 DB, E2E 실행 디렉터리는 모두 `target/` 아래에 두며 git에서 무시한다.

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
| 1 | **사용자 설정(preferences)의 카테고리 값.** 원본은 `preferences` 테이블의 `category` 컬럼에 원본 제품 이름을 값으로 쓴다(`server/services/store/sqlstore/user.go:273,297,378`). 계획은 "DB에는 옛 이름이 없다"고 전제했지만 이 한 곳에는 있다. `crates/` 아래에 옛 이름을 두지 않는다는 조건과 Go 서버와의 DB 호환이 여기서 충돌한다 | 포트는 `fvoci`를 쓴다. 그래서 원본이 만든 DB를 포트로 열면 사용자 설정(온보딩 진행 상태 등)이 비어 보이고, 반대 방향도 같다. 데이터는 지워지지 않는다. 호환이 더 중요하면 상수 하나를 옛 값으로 바꾸면 되지만 그러면 `crates/` 검색에 1건이 남는다 | S2 |
| 2 | **viewer·commenter의 카드 단건 조회.** 원본은 `GET /cards/{id}`에 `ManageBoardCards`를 요구해 viewer와 commenter가 403을 받는다(`server/api/cards.go:363`) | **결정됨(2026-11-10): 고침.** `ViewBoard`로 바꿨다. `crates/server/PORTING.md` 의도적 차이 45 | S4 |
| 3 | **보안 결정 대기 항목.** 고치면 정상 클라이언트의 동작이나 와이어 형식이 바뀌는 보안 관련 항목은 `crates/server/PORTING.md`의 "보안 결정 대기" 표(S1~)에 모았다 | 전부 원본대로 | S3–S4 |
| 4 | **의존성 보안 권고.** 번들 SQLite의 RUSTSEC-2026-0459(DB 파일과 크기가 다른 `-wal` 파일을 함께 열면 초기화되지 않은 메모리를 돌려줄 수 있다). 최신 sqlx 0.9.0이 수정된 `libsqlite3-sys` 0.39를 받지 않는다 | `deny.toml`에 사유와 함께 예외로 등록. 서버는 운영자가 지정한 DB 파일만 연다. sqlx가 올라가면 예외를 지운다 | S5 |

## 서버 트랙 결과 (S0–S5)

| 단계 | 결과 |
|---|---|
| S0 | 워크스페이스, MIT 라이선스, 오라클 빌드, 원본 번들 빌드, Cypress 기준선 11/11 |
| S1 | `fvoci-model`: 와이어 타입. 테스트 69개 |
| S2 | `fvoci-store`: SQLite + PostgreSQL. 테스트 121개, 양쪽 DB에서 통과 |
| S3–S4 | `fvoci-server`: standalone API 전체, WebSocket, 정적 서빙, 파일(로컬·S3), 아카이브, 템플릿, 온보딩, 웹훅, 세션 정리, 관리자 소켓, TLS. 테스트 512개, 양쪽 DB에서 통과. 영역을 병렬로 만들어 공유 파일이 겹치므로 커밋은 하나다 |
| S5 | 차등 비교 382단계·권한 표 636칸·DB 왕복(SQLite, PostgreSQL)·Cypress 11/11 통과. Dockerfile, CI 워크플로, CDN 문서, `NOTICE`, `deny.toml` |

검증 명령과 결과, 미검증 항목은 `crates/server/PORTING.md`의 "검증" 절에 있다.

## 플러그인 저장소의 수정 사항 점검

원본에서 갈라진 플러그인 저장소가 분리 뒤에 넣은 보안·검증 수정 15건(서버 쪽)을 원본 standalone 경로에 대해 하나씩 확인했다. 원본에 같은 결함이 있는지는 오라클에서 재현해 판단했고, 처리 결과는 `crates/server/PORTING.md`의 보안 수정 표, 의도적 차이, 보안 결정 대기 표에 PR 번호와 함께 적었다.

| PR | 결과 |
|---|---|
| #114, #179, #247, #268 (파일 소유) | 부분 수정. 새로 저장하는 파일은 보드에 묶고 다른 보드를 통한 읽기·복사·내보내기를 거절. `sourceBoardID`의 권한 검사 추가. 원본 배치 파일과 쓰기 검증은 결정 대기 S16 |
| #135 (댓글 삭제) | 수정(사용자 결정) |
| #162 (카드 단건 조회) | 수정(사용자 결정, 의도적 차이 45) |
| #207 (멤버 역할) | admin 부여는 수정. 최소 역할 강제는 원본 웹 앱의 결과와 달라 결정 대기 S15 |
| #209 (`channelId`) | 이미 고친 묶음 라우트 검사와 같다 |
| #243, #216 (속성 검증) | 읽기 쪽만 수정(의도적 차이 46). 쓰기 검증은 결정 대기 S14 |
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


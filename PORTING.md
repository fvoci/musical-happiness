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
| `fvoci-server` | `crates/server` | `server/api`, `server/app`, `server/auth`, `server/ws`, `server/web`, `server/services/*` | S3–S4 |

## 검증 도구

| 도구 | 위치 | 용도 |
|---|---|---|
| 오라클 | `target/oracle/go-server`, 실행은 `scripts/oracle-run.sh <name> <port> [single-user 토큰] [dbtype] [dbconfig]` | 원본 서버를 Go 1.21.8, 태그 `json1 sqlite3`로 빌드한 바이너리. 차등 비교, 스키마 비교, DB 왕복에 쓴다 |
| PostgreSQL | 컨테이너 `hafgufa-pg` (127.0.0.1:15433, PostgreSQL 18.6) | 스토어·서버 테스트의 PostgreSQL 실행. `FVOCI_STORE_TEST_DB_TYPE=postgres`, `FVOCI_STORE_TEST_CONN_STRING=postgres://fvoci:fvoci-test@localhost:15433/fvoci?sslmode=disable` |
| 원본 웹 번들 | `webapp/pack` | `npm ci && npm run pack` (Node 26.13.0에서 빌드됨) |
| Cypress | `scripts/e2e-cypress.sh <label> <server>` | 원본 스펙 6개(테스트 11개)를 임의의 서버 바이너리에 실행. 결과는 `target/e2e/<label>/` |

빌드 산출물, 오라클의 Go 빌드 캐시, 테스트 DB, E2E 실행 디렉터리는 모두 `target/` 아래에 두며 git에서 무시한다.

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

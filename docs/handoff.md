# 서버 트랙 인수인계

고친 때: 2026-11-10 19:41 KST. 브랜치 `fvoci/hafgufa`, 푸시한 것은 없다. 앞선 판(S5 시점, 401줄)은 `git show 14b4f37b:docs/handoff-s5.md`로 본다.

이 문서는 저장소의 다른 파일에 적혀 있지 않은 것만 담는다: 검증 명령과 걸리는 시간, 스냅샷을 갱신하는 절차, 이 기기의 컨테이너와 서버의 상태, 걸렸던 함정, 리뷰를 맡긴 방식, 커밋되지 않는 조사 자료의 자리.

| 알고 싶은 것 | 있는 곳 |
|---|---|
| S6에서 한 일(A–F), 스냅샷이 어떻게 바뀌었나, 리뷰의 지적과 처리, 남긴 것과 이유 | `PORTING.md`의 "S6: 서버 정리" |
| 결정된 설계(1–4절), 확인을 기다리는 보완 안(5절 a–h), 구현 순서 | `docs/s6-design.md` |
| 보존한 quirk와 결정 대기 항목의 지금 동작 | `crates/server/PORTING.md` |
| 스토어, 모델 | `crates/store/PORTING.md`, `crates/model/PORTING.md` |
| 웹앱 트랙의 계획, 편집기 스파이크, 날짜 모델 | 저장소 밖 `~/.claude/plans/fvoci-web-svelte.md`, 워크트리 kokanee의 `apps/web/docs/spike-editor.md`·`apps/web/PORTING.md` (이 세션에서는 읽기만 했다) |
| 원본 서버(오라클)를 다시 빌드하는 법, 차등 비교 스크립트, 개발 서버 스크립트의 전문 | 앞선 판의 1절 "레거시", 3절, 4절. 스크립트 자체는 태그 `parity-s5` |

다음에 할 일은 `docs/s6-design.md`의 "구현 순서" 3단계부터이고, 5절의 확인을 받은 뒤다.

## 1. 검증

```bash
cargo xtask check    # 포맷, clippy, 단위 테스트, 문서, 이름, 라이선스·권고. 외부 서비스 없음
cargo xtask test     # 컨테이너 준비 → SQLite 전체 → PostgreSQL(store, server) → S3 저장소 테스트
```

걸리는 시간(이 기기, 빌드 캐시가 있을 때, 2026-11-10에 잰 값):

| 무엇 | 시간 |
|---|---|
| `cargo xtask check` | 소스를 고친 뒤 30–40초(clippy 9–10초, 단위 테스트 7–20초, 문서 4초, cargo-deny 1초). 고친 것이 없으면 13초 |
| `cargo xtask test` | 2분 15초–2분 40초: SQLite 50–66초, PostgreSQL 80–90초, S3 1초 미만 |
| 회귀 테스트만(`cargo test -p fvoci-server --test regression`) | SQLite 36–41초, PostgreSQL 48–53초 |
| API 테스트만(`--test api`, 457건) | SQLite 11초, PostgreSQL 28초 |

- 지금의 기대 결과: check 여섯 단계 PASS. test는 SQLite 794건, PostgreSQL 701건, S3 1건, 실패 0. 수는 커밋마다 달라지므로 `PORTING.md`보다 실행 결과를 믿는다.
- 건너뛴 단계는 요약에 `SKIP`으로 나온다(`--skip-licenses`, `--database sqlite|postgres`). `SKIP`이 있는 실행을 전체 통과로 보고하지 않는다.
- `cargo xtask test`는 `FVOCI_REGRESSION_UPDATE`가 설정돼 있으면 시작하지 않는다(아래).
- 환경 변수로 다른 PostgreSQL·S3·컨테이너 이름·포트를 주는 법은 `xtask/src/test.rs`와 `xtask/src/containers.rs`의 머리 주석에 있다.
- 커밋마다 clippy를 통과시킨다. 워크스페이스 린트가 엄격해서(`too_many_lines`, `as_conversions`, `print_stderr`, `similar_names` 등) 테스트 코드에서 자주 걸린다. `cargo doc`도 경고가 오류라, 없는 항목을 가리키는 문서 링크 하나로 check가 실패한다.

### 회귀 스냅샷을 갱신하는 절차

스냅샷은 `crates/server/tests/regression/golden/`에 있고 SQLite와 PostgreSQL이 같은 파일을 쓴다. 응답을 의도해서 바꿨을 때만 갱신한다.

1. 갱신하기 **전에** 비교로 한 번 돌려 무엇이 어긋나는지 읽는다: `cargo test -p fvoci-server --test regression`. 실패 메시지가 단계 이름, 요청, 다른 자리를 보여 준다. 바꾸려던 것만 어긋났는지 여기서 본다.
2. 기록한다: `FVOCI_REGRESSION_UPDATE=1 cargo test -p fvoci-server --test regression`. 다시 쓴 파일마다 `recorded <파일>: CHANGED|unchanged|NEW (not compared)` 한 줄이 나온다. 이 실행은 무엇을 받든 통과하므로 검증이 아니다.
3. `git diff -- crates/server/tests/regression/golden`을 **읽는다.** 단계별 요약은 `python3 target/s6/golden_diff.py <전 디렉터리> <뒤 디렉터리> [status|all]`로 뽑는다(전 것은 `git show <커밋>:<경로>`로 꺼낸다. 5절).
4. 변수 없이 다시 돌려 통과를 확인하고, PostgreSQL에서도 돌린다(`cargo xtask test`). 한 DB에서 기록한 것이 다른 DB에서 어긋나면 그 응답의 순서나 값이 DB에 달린 것이다.
5. 코드와 **같은 커밋**에 넣고, 무엇이 왜 바뀌었는지 `PORTING.md`에 적는다.

주의:

- 변수는 정확히 `1`이어야 한다. 다른 값(`0`, `true`)은 오류이고 `CI`에서는 기록을 거부한다. 셸에 `export`해 두지 않는다.
- 하네스는 객체의 목록을 정렬해서 기록한다. 그래서 **순서가 틀려도 통과한다.** 정렬을 끄고 돌려 본 결과(2026-11-10): SQLite만으로도 실행마다 달라지는 목록이 있고(멤버 목록, 보드 검색, 아카이브의 줄), PostgreSQL에서는 보드 목록·보드 검색·카드 목록의 순서가 SQLite와 다르다. 이 라우트들에는 정해진 순서가 없다. 순서를 계약으로 정한 뒤(OpenAPI 단계)에 정렬을 기본에서 빼야 한다.
- 타임스탬프는 모두 `<ts>`, ID는 `<id:N>`, `request_id`는 `<request-id>`로 바뀐다. 응답 본문을 **문자열로** 기록하는 관찰 단계는 정규화를 지나지 않는다. 한 번 그래서 요청마다 다른 `request_id`가 스냅샷에 들어가 다음 실행이 실패했다(`cards`의 "list: bad page". JSON으로 기록하게 고쳤다).
- 권한 표의 준비 요청은 성공해야 한다(실패하면 단언으로 멈춘다). 칸이 엉뚱한 이유의 상태 코드를 기록하지 않게 한 것이다.
- 없앤 라우트(채널, 통계, 컴플라이언스, 메타데이터)의 단계와 권한 표의 행은 남겨 두었다. 404를 고정한다.

### xtask에 넣지 않은 것

```bash
target/tools/bin/cargo-about generate --manifest-path crates/server/Cargo.toml -o NOTICE about.hbs   # 의존성이 바뀌었을 때
DOCKER_BUILDKIT=1 docker build -f docker/fvoci/Dockerfile -t fvoci-server:s5 .                      # 이미지
```

S6에서 의존성을 더하지 않았다(`uuid`를 서버 크레이트가 직접 쓰게 된 것뿐이고 이미 잠금 파일에 있었다). 이미지는 A단계 뒤로 다시 빌드하지 않았다.

## 2. 이 기기의 상태 (2026-11-10 19:41 KST)

| 무엇 | 상태 |
|---|---|
| 컨테이너 `hafgufa-pg` | 떠 있다. `127.0.0.1:15433`, 사용자 `fvoci`, 비밀번호 `fvoci-test`, DB `fvoci`(테스트 전용 값). 패닉으로 끝난 테스트가 남긴 `t_*` 스키마가 118개 있다. 해는 없고, 치우려면 컨테이너를 새로 만든다 |
| 컨테이너 `hafgufa-s3` | 떠 있다. `127.0.0.1:18333`, 인증 없음, 버킷 `fvoci-test` |
| 개발 서버 8002(Rust + 원본 React 번들), 8001(원본 Go) | **멈췄다(2026-11-10 19:41 KST).** 원본 화면을 눌러 보던 용도이고 더 쓸 일이 없다. 호환을 끝낸 뒤의 서버는 원본 번들과 맞지 않는다(오류 본문, 404 규칙, 없앤 라우트). 데이터와 스크립트는 `target/dev/`에 그대로 있다. 다시 띄우는 법은 앞선 판의 4절 |
| 포트 8000 | 다른 작업(`supabase-envoy`)의 것이다. 건드리지 않았다 |
| Docker 이미지 `fvoci-server:s5` (208MB) | 남아 있다. S5의 서버다 |
| `target/` | 34GB. 대부분 `target/debug`. `/tmp`가 RAM이라 큰 것은 모두 여기 둔다 |
| 다른 작업의 컨테이너(`focalboard-port-*`, `supabase-*`), 형제 워크트리(steelhead, kokanee), 저장소 루트의 `.claude/worktrees/agent-*` 12개 | 건드리지 않았다 |
| 태그 `parity-s5` | 로컬에만 있다 |

두 컨테이너에는 재시작 정책이 없어 재부팅하면 멈춘다. `cargo xtask test`가 멈춘 것은 시작하고 없는 것은 만든다. 같은 이름의 컨테이너가 다른 포트에 게시돼 있으면 이제 거절한다(앞에는 그 컨테이너 안의 준비만 확인하고 기대한 포트로 접속했다).

## 3. 걸렸던 함정 (S6)

앞선 판 5절의 것(`/tmp`는 RAM, `.gitignore`가 무시하는 디렉터리 이름, 셸의 `cd`, 시간 제한 없는 `curl` 등)은 그대로 유효하다. 아래는 S6에서 새로 걸린 것이다.

요청 본문과 한도

- **413을 바로 답하면 클라이언트가 못 받는다.** 본문을 쓰는 중에 연결이 닫혀 broken pipe가 된다(4 MiB+1바이트 요청 9번 중 1번). 거절한 본문을 8 MiB·5초까지 읽어 버린 뒤 답한다(`api/limits.rs`의 `discard`). 업로드 한도를 넣을 때 같은 일이 생긴다. 단, `Expect: 100-continue`가 있으면 읽지 않는다(읽는 것이 "보내라"가 된다).
- **axum의 추출기 한도(2 MB)는 자기 형식의 평문 413을 낸다.** 우리 한도 아래에서 먼저 걸리므로 라우터 묶음마다 `DefaultBodyLimit::disable()`과 우리 미들웨어를 함께 둔다. 새 라우터 묶음을 만들면 둘 다 붙인다(관리 소켓을 한 번 빠뜨렸다).
- **한도 미들웨어는 세션 검사보다 먼저 돈다.** 그래서 토큰이 없는 요청은 어디서나 64 KiB이고, 토큰이 있는데 본문이 64 KiB를 넘으면 미들웨어가 세션을 먼저 확인한다(없는 세션이면 본문을 읽지 않고 401). 본문이 없는 라우트에 본문을 보내면 한도까지는 읽는다.

오류 응답과 404 규칙

- **새 라우트는 `tests/api/visibility.rs`의 표에 넣는다.** "숨겨진 것"과 "없는 것"의 답이 상태·코드·문구까지 같아야 한다. 문구에 요청의 ID를 되풀이하면 표의 비교가 실패한다(그렇게 boards-and-blocks의 문구를 고쳤다).
- **친절한 409가 구멍이 된다.** 카테고리를 만들 때 겹치는 ID에 409를 답하게 했다가, 그 답이 남의 카테고리 ID의 존재를 알려 준다는 것을 리뷰가 찾았다. 클라이언트가 ID를 정하는 자리는 모두 같은 문제다.
- **저장소의 실패를 "없음"이나 "권한 없음"으로 읽지 않는다.** `has_permission_to_board`는 이제 `Result<bool>`이다. `.is_ok()`, `Err(_) =>`로 뭉개면 DB가 잠깐 실패할 때 모든 보드가 404가 된다.
- **`AppError::context`는 로그로만 간다.** 4xx의 문구는 바뀌지 않는다. 클라이언트에게 할 말은 생성자에 적는다.
- **레코드 본문은 `decode_object`로 읽는다.** `decode_json`은 배열을 멤버 순서대로 읽어 버린다(`[]`가 빈 패치가 됐다). 목록이 본문인 라우트만 `decode_json`이다.

작업 방식

- **파이썬으로 여러 파일을 고치는 스크립트는 `assert` 하나에 통째로 멈춘다.** 앞의 파일은 써졌고 뒤는 안 써진 상태가 된다. 실패 뒤에는 어느 편집이 들어갔는지 다시 본다. `cargo fmt`가 줄을 다시 나누므로 다음 스크립트의 찾을 문자열은 포맷한 뒤의 글에서 잡는다.
- **한 작업 트리의 변경을 두 커밋으로 나눌 때**: 뒤 커밋의 파일을 `target/s6/final/`에 복사해 두고, 작업 트리를 앞 커밋의 모양으로 되돌려 검증하고 커밋한 뒤, 복사본을 되돌려 놓고 다시 검증해 커밋했다. `git stash`는 쓰지 않는다(스택이 워크트리끼리 공유된다).
- **수는 다시 센다.** 스냅샷 요약에서 "상태가 바뀐 단계"를 눈으로 더해 77로 적었고 스크립트로 세니 76이었다. 커밋 `9e920dcb`의 메시지에는 77이 남아 있다.
- **시각은 `date`로 확인한다.** 커밋 메시지에 짐작한 시각을 적었다가 고쳤다.
- **닫힌 로컬 포트로의 연결은 거절되지 않고 매달린다.** 포트 탐침에는 제한 시간을 둔다(xtask의 탐침에 2초를 넣었다).

## 4. 리뷰를 맡긴 방식

`beautiful-rust:rust-review` 스킬을 하위 에이전트에게 맡겼다. 결과와 처리는 `PORTING.md`의 S6 F절에 있다. 방식만 적는다.

- 에이전트 둘, 둘 다 `general-purpose`에 모델을 `fable`로 지정했다(Fable 5.2. 다른 모델로 넘어간 부분은 없다). 보고 끝에 자기 모델과 스킬을 읽었는지 적게 했다.
- 범위를 겹치지 않게 나눴다: 하나는 커밋 둘(`git diff 119e51fe..37ca5527`), 하나는 앞서 리뷰하지 않은 커밋들을 "지금의 코드로"(뒤 커밋이 고친 줄을 지적하지 않게).
- 지침에 넣은 것: 읽기 전용(파일, git 상태, 컨테이너, 프로세스를 바꾸지 말 것. `cargo fmt`는 `--check`만), 저장소가 무엇인지 두 문장, 의도를 적은 설계 문서의 절 번호, 일반 점검표 말고 **특히 봐 달라는 것**의 목록(404 규칙의 구멍, 한도 미들웨어의 우회, 리다이렉트, 낡은 주석), 출력 형식(Must fix / Should fix / Consider, `경로:줄`, 구체적인 입력과 잘못된 결과, 고치는 법. 읽어서 확인한 것만, 아니면 "unverified").
- 내 결론은 주지 않았다. "이 부분은 괜찮다"고 적으면 그대로 돌아온다.
- 각각 9–12분, 토큰 26만–29만. 백그라운드로 돌리고 그동안 문서를 썼다.

주의점:

- **리뷰어는 테스트를 돌리지 않았다.** 지적마다 코드를 읽거나 테스트를 써서 확인한 뒤에 고쳤다. 대부분 맞았다. 권한 표의 준비 요청에 단언을 넣으라는 지적은 그대로 넣자 멤버가 아닌 호출자에서 실패했다(그 호출자는 준비가 실패하는 것이 맞다).
- **사용자 결정과 부딪히는 지적이 있다.** 추가 때의 viewer와 join 때의 editor를 맞추라는 지적은 사용자가 정한 값을 바꾸는 일이라 고치지 않고 보고에 올렸다.
- **"남긴다"의 이유는 확인해서 적는다.** 하네스의 정렬을 남긴 이유("순서가 정해지지 않은 목록이 있다")는 정렬을 끄고 두 DB에서 돌려 본 뒤에야 근거가 됐다(1절).
- 리뷰어는 이 세션의 제약(건드리면 안 되는 컨테이너와 형제 디렉터리)을 모른다. 지침에 다시 적는다.
- 줄 번호는 지침에 적은 커밋 기준이다. 그 뒤에 고쳤으면 어긋난다.
- 리뷰가 찾은 것 가운데 둘은 이번 변경이 새로 만든 결함이었다(위의 409, 세션 조회의 실패가 401이 되는 것). 응답 규칙을 바꾸는 단계에서는 리뷰를 건너뛰지 않는다.

## 5. `target/s6/`의 조사 자료 (커밋되지 않는다)

`target/`을 지우면 사라진다. 다음 단계에 쓰는 것만 적는다.

| 파일 | 무엇 | 언제 쓰나 |
|---|---|---|
| `research/facts.md` | 보완 a–f의 근거가 된 바깥 문서의 인용문(출처 주소와 받은 시각 포함) | 설계 5.1–5.6을 구현할 때. 설계 문서는 요약만 싣는다 |
| `research/raw/` (59개, 5.6MB) | 받은 원문: 쿠키 규격 초안 6265bis, RFC 6455·7239, WHATWG WebSockets·Fetch, OWASP 문서 넷과 ASVS V7, NIST 800-63B, SQLite·PostgreSQL의 외래 키와 부분 색인, S3·R2의 멀티파트, Cloudflare와 CloudFront의 한도, tus, IETF 재개 업로드 초안, `chrono-tz`·`jiff`의 레지스트리 정보 | 인용문의 앞뒤를 볼 때. 다시 받으면 판이 달라졌을 수 있다(초안 둘은 RFC가 아니다) |
| `gz/` | 형제 저장소 GroundZero의 `contracts/openapi.json`과 xtask 일부 | OpenAPI의 도구를 세울 때(설계 5.5). 코드를 복사하지 않고 구성만 참고한다 |
| `golden_diff.py` | 스냅샷 두 벌을 단계별로 비교해 상태 코드의 전이를 센다 | 스냅샷을 갱신한 뒤의 요약(1절) |
| `tpl/` | 내장 템플릿 아카이브를 푼 것 | 템플릿 산출물을 받을 때의 대조(설계 5.7). 주인 없는 블록과 끊어진 참조를 센 스크립트는 남기지 않았다. 세는 법은 설계 5.7의 표에 적은 정의 그대로다 |
| `xtask-test-final.log`, `xtask-test-review.log`, `xtask-test-step1.log` | 전체 테스트의 출력 | 수를 대조할 때 |
| `baseline-differential.txt`, `baseline-permissions.txt`, `crosscheck.txt` | A단계에서 Python 차등 비교와 새 회귀 테스트를 대조한 기록 | 볼 일이 거의 없다 |
| `fvoci-server-measure` (284MB) | 요청 크기를 로그에 남기게 임시로 고친 빌드(설계 4.1의 측정) | 지워도 된다 |
| `final/`, `golden-119/`, `golden-head/`, `golden-before/`, `golden-backup/`, `section5-*.md`, `step1-fail*.txt`, `msg.txt`, `s4/`, `py/` | 작업 중의 사본 | 지워도 된다 |

그 밖에 `target/reference/`(플러그인 저장소 PR의 사본과 탐색 스크립트), `target/agent-brief.md`(S3–S5에서 에이전트에게 준 지침), `target/oracle/`(원본 서버의 바이너리와 Go 캐시, 714MB), `target/tools/bin`(`cargo-deny`, `cargo-about`)이 남아 있다. `target/`을 지우면 `cargo-deny`를 다시 설치해야 check의 마지막 단계가 돈다: `CARGO_TARGET_DIR=$PWD/target/tools/build cargo install cargo-deny --locked --root $PWD/target/tools`.

## 6. 다음 세션이 먼저 볼 것

- `docs/s6-design.md` 5절(보완 a–h)은 확인을 기다린다. 받으면 "구현 순서" 3단계부터다.
- 웹앱 세션에 전할 것: 날짜 값의 시각 표기가 그쪽 안(RFC 3339 문자열)과 다르다(UTC 밀리초 정수, 설계 5.7). 본문의 공유 테스트 벡터는 서버 쪽이 처음 만든다(같은 절).
- 사용자가 정할 것으로 보고에 올린 것: single-user 모드를 둘지, 추가의 viewer와 join의 editor를 맞출지.
- 알려진 채로 남긴 것(`PORTING.md` S6 F절): 멤버 추가와 마지막 관리자 확인의 동시 요청 경쟁, 회귀 하네스의 목록 정렬, 순서가 정해지지 않은 목록 라우트.

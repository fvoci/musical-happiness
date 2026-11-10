# 서버 트랙 S5 인수인계 (2026-11-10)

이 문서는 저장소의 다른 파일에 적혀 있지 않은 것만 담는다: 검증을 다시 돌리는 정확한 명령, 이 기기의 컨테이너와 도구, 커밋되지 않는 개발 서버 스크립트, 작업하면서 걸렸던 함정, 치우지 않고 남겨 둔 것.

무엇을 만들었고 무엇이 원본과 다른지는 여기에 되풀이하지 않는다.

| 알고 싶은 것 | 있는 곳 |
|---|---|
| 단계별 결과, 도구 목록, 사용자 결정 항목, 작성 모델, 플러그인 저장소 점검 요약 | `PORTING.md` (루트) |
| 서버의 API·테스트 매핑, 의도적 차이 1–65, 보안 수정 표, 보안 결정 대기 S1–S18, 보존한 quirk, 리뷰 처리, 검증 결과와 미검증 | `crates/server/PORTING.md` |
| 스토어, 모델 | `crates/store/PORTING.md`, `crates/model/PORTING.md` |
| 차등 비교에서 "의도적으로 다름"으로 등록한 항목과 사유 | `scripts/differential/expected.py` |
| 플러그인 저장소 PR별 점검 결과 | `~/.claude/plans/fvoci-plugin-boards-checklist.md` (저장소 밖) |
| 전체 계획(웹앱 트랙 W1 이후 포함) | `~/.claude/plans/focalboard-rust-woolly-candy.md` (저장소 밖) |
| CDN 배포 안내 | `docs/fvoci-cdn.md` |

기준점: 태그 `parity-s5` = `cc8a422e`(S5 커밋). 이 문서의 커밋은 그 뒤에 온다. 푸시한 것은 없다.

## 1. 검증을 다시 돌리는 순서

S6(2026-11-10)에서 손으로 치던 명령을 `cargo xtask`로 바꿨다. 워크트리 안 어디서 실행해도 된다.

```bash
cargo xtask check    # 포맷, clippy, 단위 테스트, 문서, 이름, 라이선스·권고. 외부 서비스를 띄우지 않는다
cargo xtask test     # 컨테이너 준비 → SQLite 전체 → PostgreSQL(store, server) → S3 저장소 테스트
```

| 명령 | 하는 일 (순서대로, 하나가 실패해도 끝까지 돌리고 마지막에 요약) |
|---|---|
| `cargo xtask check` | `cargo fmt --all --check` → `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → `cargo test --workspace --lib --bins --locked` → `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` → 이름 검사(`crates/`, `xtask/`에 원본의 두 이름이 0건) → `cargo-deny check` |
| `cargo xtask check --skip-licenses` | 위에서 `cargo-deny`만 뺀다(권고 DB를 네트워크에서 읽는다. CI는 따로 job으로 돌린다) |
| `cargo xtask test` | `cargo test --workspace --locked`(SQLite) → `hafgufa-pg` 준비 뒤 `cargo test --locked -p fvoci-store -p fvoci-server`(PostgreSQL) → `hafgufa-s3`와 버킷 준비 뒤 `cargo test --locked -p fvoci-server --test storage -- --ignored` |
| `cargo xtask test --database sqlite` | SQLite만. 컨테이너가 필요 없다 |
| `cargo xtask test --database postgres` | PostgreSQL만 |

컨테이너 준비: 떠 있으면 그대로 쓰고, 멈춰 있으면 `docker start`, 없으면 2절과 같은 설정으로 만든다(`postgres:18`, `chrislusf/seaweedfs:latest`, 127.0.0.1에만 게시). 받을 준비가 될 때까지 최대 90초 기다린다. **지우지는 않는다.** 환경 변수로 바꿀 수 있는 것:

| 환경 변수 | 뜻 |
|---|---|
| `FVOCI_STORE_TEST_CONN_STRING` | 주어지면 그 PostgreSQL을 쓰고 컨테이너를 건드리지 않는다(CI) |
| `FVOCI_S3_TEST_ENDPOINT` + `FVOCI_S3_TEST_BUCKET` | 주어지면 그 S3를 쓴다. 선택: `FVOCI_S3_TEST_ACCESS_KEY`, `FVOCI_S3_TEST_SECRET_KEY`, `FVOCI_S3_TEST_REGION`(기본 `us-east-1`), `FVOCI_S3_TEST_SSL` |
| `FVOCI_TEST_PG_CONTAINER`, `FVOCI_TEST_PG_PORT` | 컨테이너 이름과 호스트 포트(기본 `hafgufa-pg`, 15433). 한 기기의 다른 체크아웃과 겹치지 않게 할 때 |
| `FVOCI_TEST_S3_CONTAINER`, `FVOCI_TEST_S3_PORT` | 같은 것(기본 `hafgufa-s3`, 18333) |
| `FVOCI_REGRESSION_UPDATE=1` | 회귀 테스트의 스냅샷을 비교하지 않고 다시 쓴다(아래) |

기대하는 결과(2026-11-10, S6의 A 직후)는 `PORTING.md`의 "S6" 절에 있다.

### 회귀 테스트 (차등 비교와 권한 표를 대체)

`crates/server/tests/regression/`가 `scripts/differential/`의 시나리오 15개(382단계)와 권한 표(636칸)를 대체한다. Go 서버가 필요 없고 `cargo test --workspace`에 들어 있다. 테스트마다 서버를 따로 띄우며, SQLite와 PostgreSQL이 같은 스냅샷을 쓴다.

- 스냅샷: `crates/server/tests/regression/golden/<시나리오>.json`(단계마다 이름, 요청, 상태 코드, 정규화한 본문), `permissions_private.txt`·`permissions_open.txt`(라우트 × 호출자별 상태 코드 표).
- 기대값은 태그 `parity-s5`의 Rust 서버가 낸 응답이다.
- **응답을 의도해서 바꿀 때:** `FVOCI_REGRESSION_UPDATE=1 cargo test -p fvoci-server --test regression`으로 스냅샷을 다시 쓰고, `git diff`로 바뀐 응답을 확인한 뒤 **같은 커밋에** 넣는다. 갱신 방법은 이것 하나다.
- 스냅샷 라이브러리를 쓰지 않고 직접 비교하는 이유는 `PORTING.md`의 "S6" 절에 적었다.

### xtask에 넣지 않은 것

```bash
# 제3자 고지 다시 만들기 (의존성이 바뀌었을 때)
target/tools/bin/cargo-about generate --manifest-path crates/server/Cargo.toml -o NOTICE about.hbs
# 이미지
DOCKER_BUILDKIT=1 docker build -f docker/fvoci/Dockerfile -t fvoci-server:s5 .
```

`cargo-deny`와 `cargo-about`은 `target/tools/bin`에 있다(없으면 5절의 설치 명령). `cargo xtask check`는 `target/tools/bin/cargo-deny`가 있으면 그것을, 없으면 경로의 `cargo-deny`를 쓴다.

### 레거시: 원본 서버(오라클)가 필요한 검증

`parity-s5`까지 쓰던 것이다. 호환을 끝냈으므로 이후의 변경은 이 검증을 통과하지 않는다(통과할 필요가 없다). 스크립트와 오라클은 아직 지우지 않았다. `parity-s5`를 체크아웃해 다시 확인할 때만 쓴다.

```bash
# 차등 비교: 원본 대 포트 (382단계)
cargo build -p fvoci-server            # fvoci-run.sh는 기본으로 target/debug/fvoci-server를 띄운다
P1=$(scripts/oracle-run.sh diff-left 18102 2>/dev/null | tail -1)
P2=$(scripts/fvoci-run.sh diff-right 18103 | tail -1)
python3 scripts/differential/run.py --left http://localhost:18102 --right http://localhost:18103
kill $P1 $P2

# 권한 표 (636칸. 원본 둘로 돌렸을 때 전체 2분 3초)
P1=$(scripts/oracle-run.sh perm-left 18104 2>/dev/null | tail -1)
P2=$(scripts/fvoci-run.sh perm-right 18105 | tail -1)
python3 scripts/differential/permissions.py --left http://localhost:18104 --right http://localhost:18105
kill $P1 $P2

# DB 왕복 (포트 18131–18133을 쓴다)
scripts/differential/roundtrip.sh sqlite
scripts/differential/roundtrip.sh postgres 'postgres://fvoci:fvoci-test@localhost:15433'

# 브라우저 종단: 원본 Cypress 스펙을 Rust 서버에 (약 1분 30초, 포트 18088)
scripts/e2e-cypress.sh rust-s5 target/debug/fvoci-server
```

`parity-s5`에서의 결과: 차등 비교 382단계에 예상 밖 0건·등록 21건, 권한 표 636칸에 예상 밖 0건·등록 11건, 왕복은 방향마다 관찰 22건에 차이 0건, Cypress 11/11.

알아 둘 것:

- `oracle-run.sh`와 `fvoci-run.sh`는 호출할 때마다 `target/oracle/runs/<이름>/`, `target/fvoci/runs/<이름>/`을 지우고 새 DB로 띄운다. PID를 마지막 줄에 찍으므로 `tail -1`로 받는다. 오라클 쪽은 뜨기 전 `curl` 실패가 stderr로 나오니 `2>/dev/null`을 붙인다. 릴리스 빌드로 비교하려면 `FVOCI_SERVER_BIN=target/release/fvoci-server`.
- `run.py`는 `--only <시나리오>`(여러 번 가능)와 `--verbose`를, `permissions.py`는 `--only "<라우트 이름 일부>"`와 `--table`(원본 쪽 표 전체 출력)을 받는다. 두 서버 모두 **빈 DB**여야 한다.
- `roundtrip.sh postgres`는 컨테이너 `hafgufa-pg` 안에서 `psql`로 데이터베이스 `roundtrip_a`, `roundtrip_b`를 지우고 다시 만든다.

## 2. 컨테이너

이 작업이 띄운 것은 둘뿐이다. 둘 다 재시작 정책이 없어 기기를 재부팅하면 멈춘다. `cargo xtask test`가 멈춘 것은 다시 시작하고 없는 것은 만든다(1절). 아래는 손으로 할 때의 명령이다.

| 컨테이너 | 이미지 | 주소 | 접속 정보 |
|---|---|---|---|
| `hafgufa-pg` | `postgres:latest` (18.6) | `127.0.0.1:15433` | 사용자 `fvoci`, 비밀번호 `fvoci-test`, DB `fvoci`. 테스트 전용 값이다 |
| `hafgufa-s3` | `chrislusf/seaweedfs:latest` | `127.0.0.1:18333` | 인증 없음, 버킷 `fvoci-test` |

```bash
# 멈춰 있으면
docker start hafgufa-pg hafgufa-s3

# 지워졌으면 처음부터
docker run -d --name hafgufa-pg -e POSTGRES_USER=fvoci -e POSTGRES_PASSWORD=fvoci-test \
  -e POSTGRES_DB=fvoci -p 127.0.0.1:15433:5432 postgres:18
docker run -d --name hafgufa-s3 -p 127.0.0.1:18333:8333 chrislusf/seaweedfs:latest \
  server -s3 -s3.port=8333
curl -s -o /dev/null -w '%{http_code}\n' -X PUT http://localhost:18333/fvoci-test    # 버킷 생성, 200
```

- 스토어·서버 테스트는 테스트마다 `t_<id>` 스키마를 만들고 끝나면 지운다. 테스트가 패닉으로 끝나면 스키마가 남는다. 지금 `fvoci` DB에 그런 스키마가 21개 남아 있다. 해는 없고, 치우려면 컨테이너를 새로 만들면 된다.
- 같은 기기에 **다른 작업의 컨테이너**가 떠 있다: `focalboard-port-pg`/`-mysql`/`-maria`(steelhead 세션), `supabase-*`. 건드리지 않는다. 호스트 포트 **8000은 `supabase-envoy`**가 쓴다.

## 3. 오라클(원본 Go 서버) 다시 빌드하기

```bash
mkdir -p target/oracle/gocache target/oracle/gotmp
GO=~/go/pkg/mod/golang.org/toolchain@v0.0.1-go1.21.8.linux-amd64/bin/go
W=$PWD
GOTOOLCHAIN=local CGO_ENABLED=1 GOCACHE=$W/target/oracle/gocache GOTMPDIR=$W/target/oracle/gotmp \
  $GO build -C server -tags 'json1 sqlite3' \
  -ldflags '-X "github.com/mattermost/focalboard/server/model.Edition=dev"' \
  -o $W/target/oracle/go-server ./main
```

- 걸리는 시간(이 기기에서 잼): 빌드 캐시가 있으면 **3초**, 빌드 캐시가 비어 있으면 **35초**(벽시계, CPU 약 150초). 모듈 소스(`~/go/pkg/mod`, 962MB)가 이미 받아져 있을 때의 값이다. 새 기기에서는 모듈과 Go 1.21.8 도구 모음을 내려받는 시간이 더 든다(재지 않았다).
- cgo라서 C 컴파일러가 필요하다(여기서는 gcc 15.2). SQLite 쪽에서 컴파일 경고가 몇 줄 나오지만 빌드는 된다.
- 같은 명령으로 다시 빌드한 결과는 지금 쓰는 `target/oracle/go-server`와 바이트까지 같았다.
- 개발 서버의 Go 쪽(4절)이 이 바이너리로 떠 있다. 덮어쓰기 전에 `target/dev/server.sh stop go`.
- 원본 React 번들(`webapp/pack`)을 다시 만들 때: `cd webapp && npm ci && npm run pack`. S0 때 `npm ci` 39초, webpack 24초가 걸렸다(Node 26.13.0, 로그는 `target/oracle/npm-*.log`). Cypress도 이 `node_modules`에 들어 있다.

## 4. 개발 서버 (손으로 눌러 보는 용도)

`target/dev/`는 git이 무시한다. `target/`을 지우면 스크립트도 사라지므로 내용을 아래에 둔다. 다시 만들 때는 `target/dev/server.sh`, `target/dev/check.py`로 저장하고 `chmod +x target/dev/server.sh`.

| | 주소 | 바이너리 | 데이터 |
|---|---|---|---|
| Rust + 원본 React 번들 | http://localhost:8002 | `target/release/fvoci-server` (`cargo build --release -p fvoci-server`, 1분 15초) | `target/dev/rust/` |
| 원본 Go + 같은 번들 | http://localhost:8001 | `target/oracle/go-server` | `target/dev/go/` |

```bash
target/dev/server.sh status
target/dev/server.sh start rust      # DB를 유지한 채 띄움
target/dev/server.sh stop rust
target/dev/server.sh fresh rust      # DB·업로드·로그를 지우고 처음부터 (rust 자리에 go)
python3 target/dev/check.py rust 8002   # 계정 dev를 만들고 기본 동작과 /ws를 확인 (Go는: go 8001)
```

- 계정: `dev` / `dev-password-1`. `fresh` 뒤에는 `check.py`를 다시 돌려야 생긴다. 두 번째 사용자부터는 가입에 초대 토큰이 필요하다(웹 앱의 초대 링크).
- 일반 로그인 모드, 로컬 파일 저장소, 공개 공유 켬, 모든 인터페이스에서 수신. Windows 브라우저에서 `localhost`가 안 되면 WSL 주소(`hostname -I`의 첫 값, 작성 시점에는 `192.168.0.15`)로 연다.
- Docker 이미지 `fvoci-server:s5`에는 React 번들이 없다(자리표시 페이지만 있다). 그래서 릴리스 빌드를 직접 띄운다.
- 서버는 `setsid nohup`으로 띄워 세션이 끝나도 남는다. PID는 `target/dev/<이름>/server.pid`.

`target/dev/server.sh`:

```bash
#!/usr/bin/env bash
# Development servers for clicking through the port by hand. Not tracked by git.
#
#   target/dev/server.sh start rust|go     start (keeps the database)
#   target/dev/server.sh stop  rust|go     stop
#   target/dev/server.sh fresh rust|go     stop, delete database and uploads, start
#   target/dev/server.sh status
#
# rust: the Rust server (release build) with the original React bundle, port 8002
# go:   the original Go server (the oracle) with the same bundle, port 8001
# Everything a server writes is under target/dev/<name>/ (database, files/, server.log).
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
declare -A port=([rust]=8002 [go]=8001)
declare -A binary=([rust]="$root/target/release/fvoci-server" [go]="$root/target/oracle/go-server")

running() { [ -f "$1/server.pid" ] && kill -0 "$(cat "$1/server.pid")" 2>/dev/null; }

stop() {
    local dir="$root/target/dev/$1"
    if running "$dir"; then
        kill "$(cat "$dir/server.pid")"
        for _ in $(seq 1 50); do running "$dir" || break; sleep 0.2; done
        echo "$1: stopped"
    else
        echo "$1: not running"
    fi
    rm -f "$dir/server.pid"
}

start() {
    local name=$1 dir="$root/target/dev/$1" p=${port[$1]}
    if running "$dir"; then echo "$name: already running (pid $(cat "$dir/server.pid"))"; return; fi
    if ss -ltnH | awk '{print $4}' | grep -qE ":$p\$"; then
        echo "$name: port $p is in use by another process; not starting" >&2; exit 1
    fi
    mkdir -p "$dir/files"
    cat >"$dir/config.json" <<JSON
{
    "serverRoot": "http://localhost:$p",
    "port": $p,
    "dbtype": "sqlite3",
    "dbconfig": "$dir/data.db?_busy_timeout=5000",
    "useSSL": false,
    "webpath": "$root/webapp/pack",
    "filesdriver": "local",
    "filespath": "$dir/files",
    "telemetry": false,
    "session_expire_time": 2592000,
    "session_refresh_time": 18000,
    "localOnly": false,
    "enablePublicSharedBoards": true
}
JSON
    cd "$dir"
    setsid nohup "${binary[$name]}" --config "$dir/config.json" >>"$dir/server.log" 2>&1 </dev/null &
    echo $! >"$dir/server.pid"
    cd "$root"
    for _ in $(seq 1 100); do
        if curl -fsS -m 1 -o /dev/null "http://localhost:$p/hello" 2>/dev/null; then
            echo "$name: started on http://localhost:$p (pid $(cat "$dir/server.pid"), log $dir/server.log)"
            return
        fi
        running "$dir" || { echo "$name: exited early; see $dir/server.log" >&2; exit 2; }
        sleep 0.2
    done
    echo "$name: did not answer on port $p; see $dir/server.log" >&2; exit 3
}

case "${1:-}" in
start) start "${2:?rust or go}" ;;
stop) stop "${2:?rust or go}" ;;
fresh)
    stop "${2:?rust or go}"
    rm -rf "$root/target/dev/$2/data.db"* "$root/target/dev/$2/files" "$root/target/dev/$2/server.log"
    start "$2" ;;
status)
    for name in rust go; do
        dir="$root/target/dev/$name"
        if running "$dir"; then echo "$name: running on http://localhost:${port[$name]} (pid $(cat "$dir/server.pid"))"; else echo "$name: stopped"; fi
    done ;;
*) sed -n 2,12p "$0"; exit 2 ;;
esac
```

`target/dev/check.py`:

```python
import base64, json, os, socket, struct, sys, urllib.request, urllib.error
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', '..', 'scripts', 'differential'))
name, port = sys.argv[1], int(sys.argv[2])
base = f"http://localhost:{port}"
H = {"X-Requested-With": "XMLHttpRequest", "Content-Type": "application/json"}
def call(method, path, body=None, token=None, api=True):
    headers = dict(H) if api else {}
    if token: headers["Authorization"] = f"Bearer {token}"
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(base + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=20) as r: return r.status, r.read(), r.headers.get("Content-Type", "")
    except urllib.error.HTTPError as e: return e.code, e.read(), e.headers.get("Content-Type", "")
st, body, ct = call("GET", "/", api=False)
print(f"[{name}] first page: {st} {ct}, {len(body)} bytes, React bundle: {b'main.js' in body or b'static/' in body}")
st, body, _ = call("POST", "/api/v2/register", {"username": "dev", "email": "dev@example.com", "password": "dev-password-1"})
print(f"[{name}] register dev: {st} {body.decode()[:80]}")
st, body, _ = call("POST", "/api/v2/login", {"type": "normal", "username": "dev", "password": "dev-password-1"})
token = json.loads(body).get("token", "")
print(f"[{name}] login: {st}, token of {len(token)} characters")
st, body, _ = call("GET", "/api/v2/users/me", token=token)
print(f"[{name}] me: {st} username={json.loads(body).get('username')}")
st, body, _ = call("GET", "/api/v2/teams/0/templates", token=token)
print(f"[{name}] built-in templates: {st}, {len(json.loads(body))} boards")
ws_messages = []
if name == "rust":
    # A minimal websocket client: handshake, AUTH, SUBSCRIBE_TEAM, then read one frame.
    sock = socket.create_connection(("localhost", port), timeout=10)
    key = base64.b64encode(os.urandom(16)).decode()
    sock.sendall((f"GET /ws HTTP/1.1\r\nHost: localhost:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                  f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n").encode())
    reply = b""
    while b"\r\n\r\n" not in reply: reply += sock.recv(4096)
    print(f"[{name}] /ws handshake: {reply.split(b'\r\n')[0].decode()}")
    def send(obj):
        payload = json.dumps(obj).encode(); mask = os.urandom(4)
        head = bytes([0x81]) + (bytes([0x80 | len(payload)]) if len(payload) < 126 else bytes([0x80 | 126]) + struct.pack(">H", len(payload)))
        sock.sendall(head + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))
    def read_text():
        while True:
            h = sock.recv(2)
            op, n = h[0] & 0x0F, h[1] & 0x7F
            if n == 126: n = struct.unpack(">H", sock.recv(2))[0]
            elif n == 127: n = struct.unpack(">Q", sock.recv(8))[0]
            data = b""
            while len(data) < n: data += sock.recv(n - len(data))
            if op == 1: return json.loads(data)
    send({"action": "AUTH", "token": token}); send({"action": "SUBSCRIBE_TEAM", "teamId": "0"})
    import time; time.sleep(0.5)
st, body, _ = call("POST", "/api/v2/boards", {"teamId": "0", "type": "P", "title": "Dev check board", "properties": {}, "cardProperties": []}, token=token)
board = json.loads(body)
print(f"[{name}] create board: {st} id={board.get('id')} title={board.get('title')!r}")
st, body, _ = call("GET", f"/api/v2/boards/{board.get('id')}", token=token)
print(f"[{name}] get board: {st} title={json.loads(body).get('title')!r}")
st, body, _ = call("GET", "/api/v2/teams/0/categories", token=token)
cats = json.loads(body)
print(f"[{name}] sidebar: {st}, categories={[c['name'] for c in cats]}, boards filed={sum(len(c['boardMetadata']) for c in cats)}")
# A board without any block makes the web app try to join it and end on its
# "board not found" page, so the check does not leave its board behind.
st, _, _ = call("DELETE", f"/api/v2/boards/{board.get('id')}", token=token)
print(f"[{name}] delete the check board: {st}")
if name == "rust":
    sock.settimeout(5)
    try:
        seen = []
        for _ in range(3):
            m = read_text(); seen.append(m.get("action"))
            if m.get("action") == "UPDATE_BOARD": break
        print(f"[{name}] /ws messages after creating the board: {seen}")
    except Exception as e:
        print(f"[{name}] /ws: no message within 5 s ({e})")
    sock.close()
```

## 5. 걸렸던 함정

빌드와 디스크

- **`/tmp`는 RAM(tmpfs)이다.** 빌드 산출물, 테스트 DB, Go 캐시는 전부 워크트리 `target/` 아래에 둔다. `cargo install`도 기본으로 `/tmp`에서 빌드하므로 `CARGO_TARGET_DIR=$PWD/target/tools/build cargo install … --root $PWD/target/tools`처럼 돌린다.
- **하위 에이전트마다 빌드 디렉터리를 따로 주면 디스크를 많이 쓴다.** 에이전트 6개가 `target/agents/<이름>`에 30GB를 썼다(끝난 뒤 지웠다). 지금 `target/`은 24GB이고 그중 21GB가 `target/debug`다.
- **`cargo-about`은 `--features cli`를 줘야 실행 파일이 설치된다.** `cargo-deny`와 함께 `target/tools/bin`에 있다. `target/`을 지우면 다시 설치해야 한다: `CARGO_TARGET_DIR=$PWD/target/tools/build cargo install cargo-deny --locked --root $PWD/target/tools`, `… cargo install cargo-about --locked --features cli --root $PWD/target/tools`.
- **저장소 `.gitignore`가 `files`, `bin`, `debug`, `dist`, `pack`, `package`, `logs`, `coverage`라는 이름의 디렉터리를 무시한다.** 그런 이름의 모듈 디렉터리나 테스트 픽스처 디렉터리를 만들면 커밋에서 빠진다(그래서 `storage/`다).
- **개발 프로필에서 `bcrypt`와 `blowfish`만 최적화한다**(루트 `Cargo.toml`). 빼면 로그인하는 테스트마다 몇 초씩 걸린다.
- **Docker**: Debian 12 기반에서는 `aws-lc-sys`의 사전 빌드 객체 때문에 링크가 실패한다(Debian 13을 쓴다). 실행 이미지에 `ca-certificates`가 없으면 웹훅이나 S3의 HTTPS가 안 된다. 빌드 컨텍스트는 `docker/fvoci/Dockerfile.dockerignore`가 거른다(루트의 `.dockerignore`는 원본 것이라 건드리지 않았다).

셸과 포트

- **이 하네스의 Bash는 `cd`가 다음 호출까지 남는다.** 명령마다 `cd /home/kinesis/atlas/focalboard-rs/hafgufa &&`를 앞에 붙였다. 한 번 `crates/server/src/api`로 들어간 채 다음 명령이 돌았다.
- **시간 제한 없는 `curl`은 멈출 수 있다.** 컨테이너가 죽은 뒤 Docker가 포트만 잡고 있던 상황에서 `curl`이 30분 넘게 매달려 있었다. 확인용 `curl`에는 `-m <초>`를 붙인다.
- **쓰는 포트**: 15433(PostgreSQL), 18333(S3), 18088(Cypress), 18101–18109(오라클·포트 비교와 탐색), 18131–18133(DB 왕복), 18200(이미지 스모크 테스트), 8001·8002(개발 서버). 하위 에이전트에게는 18210–18269를 10개씩 나눠 줬다.

테스트 격리

- **SQLite 테스트 DB는 `target/tmp/` 아래 파일, PostgreSQL은 테스트마다 스키마**다(`fvoci_store::testing::TestDatabase`). 관리자 소켓 테스트만 소켓 경로 길이 제한 때문에 시스템 임시 디렉터리를 쓴다(파일이 작다).
- **비교 스크립트(Python)에서 걸린 것**: 헤더 이름의 대소문자(Go는 `Content-Type`, Rust는 `content-type`으로 보낸다), `urllib`은 리다이렉트를 따라가고 `Content-Length`보다 짧은 본문에서 예외를 던진다. `reqwest`와 `urllib` 모두 `%2e%2e`를 `..`로 풀어 경로를 정리해 버리므로, 경로 탈출 시험에는 `..%2F`를 쓴다.
- **시나리오 안에서 "다른 사용자의 상태를 바꾸는 요청"은 맨 뒤에 둔다.** 원본은 거절(500)하면서도 써 버리는 경우가 있어(남의 카테고리, 없는 카테고리), 그 뒤의 모든 단계가 연쇄로 달라 보인다.
- **블록이 하나도 없는 보드를 API로 만들어 두면 원본 웹 앱이 로그인 직후 `error?id=board-not-found`로 간다.** 웹 앱은 블록이 0개면 "멤버가 아니다"로 보고 참여를 시도하는데 비공개 보드는 거절된다. 서버 문제가 아니다. 확인 스크립트는 만든 보드를 지우고 끝난다.
- **원본 웹 앱은 `sourceBoardID`를 보내지 않고, 남의 댓글 삭제 메뉴를 보여 주지 않는다.** 보안 수정이 정상 클라이언트에 닿는지 판단할 때 `webapp/src`에서 호출부를 찾아보는 것이 빨랐다.

하위 에이전트

- **격리 워크트리는 내 HEAD가 아니라 `main`에서 만들어진다.** 에이전트에게 기준 커밋을 알려 주고 첫 일로 `git reset --hard <커밋>`을 시켰다. 결과는 에이전트가 자기 워크트리에 커밋하게 하고 `git cherry-pick`으로 가져왔다.
- **스크래치패드 디렉터리는 에이전트끼리 공유된다.** 한 에이전트의 탐색 스크립트를 다른 에이전트가 덮어썼다. 이름으로 하위 디렉터리를 나누게 한다.
- **에이전트에게 준 공통 지침서는 `target/agent-brief.md`에 있다**(커밋되지 않는다). 영역별 소유 파일, 금지어, 오라클 사용법, 보고 형식을 담았다.
- **보안 내용이 많은 과제에서 에이전트의 모델이 도중에 바뀌었다**(파일·아카이브 영역, 루트 `PORTING.md` "작성 모델"). 그런 영역은 병합 뒤 직접 읽고 따로 리뷰를 돌렸다.
- **리뷰 에이전트는 읽기 전용으로, 모델을 명시해서 띄웠다.** 두 번 모두 테스트가 잡지 못한 결함을 찾았다.

## 6. 남아 있는 것

치우지 않고 둔 것이다. 지워도 되는지는 다음 세션에서 판단한다.

| 무엇 | 어디 | 메모 |
|---|---|---|
| 에이전트 워크트리 10개 | `/home/kinesis/atlas/focalboard-rs/.claude/worktrees/agent-*` | hafgufa 밖이라 건드리지 않았다. 내용은 전부 `fvoci/hafgufa`에 병합됐다. `git worktree remove <경로>`로 지운다 |
| 그 브랜치 10개 | `worktree-agent-<id>` | 워크트리를 지운 뒤 `git branch -D` |
| 개발 서버 2개 | 포트 8002(Rust), 8001(Go) | `target/dev/server.sh stop rust`, `… stop go` |
| 컨테이너 2개 | `hafgufa-pg`, `hafgufa-s3` | 2절. 볼륨은 Docker의 익명 볼륨이다 |
| Docker 이미지 | `fvoci-server:s5` (208MB) | 스모크 테스트용. `docker rmi fvoci-server:s5` |
| PostgreSQL의 잔여물 | `t_*` 스키마 21개, DB `roundtrip_a`·`roundtrip_b` | 해 없음 |
| 실행 기록 | `target/oracle/runs/*`, `target/fvoci/runs/*`, `target/e2e/*`, `target/roundtrip/*`, `target/s5-*.txt` | 마지막 검증의 로그와 DB |
| 플러그인 PR 사본 | `target/reference/prs/*.diff`, `*.txt` | 읽기용. 탐색 스크립트 `target/reference/probe_*.py`도 여기 있다 |
| 도구 | `target/tools/bin/cargo-deny`, `cargo-about` (빌드 캐시 포함 878MB) | |
| 태그 | `parity-s5` (로컬) | 푸시하지 않았다 |

이 세션이 띄운 백그라운드 셸은 남아 있지 않다. 개발 서버 둘은 세션에서 분리된 프로세스다.

## 7. 다음에 할 일로 넘긴 것

- 결정 대기 항목(S1–S6, S8, S9, S11, S12, S14의 쓰기 검증, S15–S18, 결정 항목 1·4)은 사용자가 따로 지시하기로 했다. 그때까지 원본 동작이다.
- 사용자가 개발 서버로 직접 눌러 보는 중이다. 브라우저로 자동 확인하지 못한 항목: 템플릿 선택, 칸반의 열 사이 드래그, 테이블·갤러리·캘린더 전환, 이미지 첨부, 두 탭 실시간 반영, 공유 링크, 아카이브 내보내기·가져오기.
- 웹앱 트랙(W1 이후)은 시작하지 않았다.

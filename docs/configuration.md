# 서버의 설정

적은 때: 2026-11-11 05:05 KST, 고친 때: 18:56 KST. 서버(`fvoci-server`)가 읽는 설정의 전부다. 코드는 `crates/server/src/config.rs`다.

이 문서는 코드와 함께 검사된다. `config.rs`의 테스트 `the_configuration_document_lists_every_key`가 다음을 견준다.

- 코드가 받는 키와 아래 "키" 표들에 적힌 키가 같은지.
- 키마다 기본값이 같은지.
- 환경 변수로 바꿀 수 있는 키가 같은지.
- "없앤 키" 표가 코드의 목록과 같은지.

키를 더하거나 바꾸면 같은 커밋에서 이 문서를 고친다.

## 읽는 법

설정은 세 곳에서 온다. 뒤의 것이 앞의 것을 이긴다.

1. **설정 파일.** JSON 객체 하나다. 자리는 명령줄의 `--config`로 주며 기본은 `./config.json`이다.
   - 키는 대소문자를 가리지 않는다(`serverRoot`와 `serverroot`는 같다). `filess3config` 안의 키도 같다. 같은 키를 두 번 주면(대소문자만 다른 것도, 모르는 키도) 서버가 시작하지 않는다. 오류는 키 이름과 그 자리만 말하고 값은 말하지 않는다. `filess3config` 밖의 객체(`featureflags` 등) 안은 보지 않는다.
   - 모르는 키는 말없이 무시한다(원본 서버의 설정 파일을 그대로 쓸 수 있게).
   - 값의 타입이 틀리면 서버가 시작하지 않는다(`filess3config`가 객체도 `null`도 아닌 것도).
2. **환경 변수.** `FVOCI_`에 키를 대문자로 이은 이름이다(예: `FVOCI_SESSION_IDLE_DAYS`. `filess3config` 안의 키는 점을 `_`로 써서 `FVOCI_FILESS3CONFIG_BUCKET`). 아래 표의 "환경 변수" 칸에 이름이 있는 키만 바꿀 수 있다.
   - 수는 정수로 읽는다.
   - 참·거짓은 `true`·`1`·`t`·`yes`·`on`과 `false`·`0`·`f`·`no`·`off`로 읽는다.
   - 목록은 쉼표로 나눈다.
3. **명령줄 인자.** `--port`, `--dbtype`, `--dbconfig` 셋이다(아래 "명령줄 인자"). 빈 값이나 0은 무시한다.

읽은 뒤에 서버는 값을 검사하고, 받을 수 없는 값이 있으면 시작하지 않는다. 받는 범위는 표의 "타입" 칸과 "뜻과 범위" 칸에 있다.

"기본값" 칸은 JSON으로 적는다. 키를 빼면 그 값이다.

## 키: 서버의 주소와 화면

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `serverRoot` | `FVOCI_SERVERROOT` | 문자열 | `"http://localhost:8000"` | 사용자가 서버에 닿는 주소다. 경로가 있으면 웹앱이 그 경로를 자기 주소의 기준으로 쓴다. 서버가 실제로 그 경로 아래에서 답하게 하는 것은 `FVOCI_HTTP_SERVER_BASEPATH`(아래 "그 밖의 환경 변수")이므로 둘을 같게 준다. 경로를 떼어 넘기는 프록시 뒤라면 그 변수는 비워 둔다. 바꾸는 요청과 WebSocket이 받아들이는 출처(`Origin`)가 이 주소에서 나온다. `https://`로 시작하면 세션 쿠키가 `Secure`가 되고 이름이 `__Host-fvoci_session`이 된다. 출처로 읽을 수 없는 주소면 시작하지 않는다 |
| `port` | `FVOCI_PORT` | 정수(1–65535) | `8000` | 들을 포트. 0이면 시작하지 않는다 |
| `localOnly` | `FVOCI_LOCALONLY` | 참·거짓 | `false` | 참이면 루프백(`localhost`)에서만 듣는다. 거짓이면 모든 인터페이스(IPv6, 안 되면 IPv4)에서 듣는다 |
| `useSSL` | `FVOCI_USESSL` | 참·거짓 | `false` | 참이면 서버가 직접 HTTPS를 연다. 인증서는 작업 디렉터리의 `./cert/cert.pem`과 `./cert/key.pem`이다 |
| `webpath` | `FVOCI_WEBPATH` | 문자열 | `"./pack"` | 웹앱의 빌드 산출물이 든 디렉터리(`index.html`과 `static/`) |
| `allowed_origins` | `FVOCI_ALLOWED_ORIGINS` | 문자열 목록 | `[]` | `serverRoot`의 출처 말고도 바꾸는 요청과 WebSocket을 받아들일 출처. 개발할 때 웹앱을 다른 포트에서 띄우는 경우에 쓴다. 출처로 읽을 수 없는 값이 있으면 시작하지 않는다 |
| `websocket_ping_seconds` | `FVOCI_WEBSOCKET_PING_SECONDS` | 정수(0 이상) | `30` | WebSocket마다 Ping을 보내는 간격(초). 0이면 보내지 않는다. 한가한 연결을 프록시가 끊지 않게 한다. Ping마다 연결의 세션도 다시 보고 끝났으면 닫는다(0이면 1분마다). 브라우저의 Pong은 연결이 보낸 것으로 센다: 90초 동안 아무것도 보내지 않은 연결은 닫는다(설정이 아니다. `contracts/websocket.md` 3절의 "죽은 연결"). 0이거나 30보다 크면 타이머가 멈춘 페이지가 90초마다 닫히므로 서버가 시작할 때 경고한다 |
| `public_link_connections_per_board` | `FVOCI_PUBLIC_LINK_CONNECTIONS_PER_BOARD` | 정수(1 이상) | `200` | 공개 링크로 한 보드를 함께 읽는 연결의 수. 링크의 읽기 토큰으로 블록을 따라가거나(`SUBSCRIBE_BLOCKS`) 그 토큰으로 방문을 알리는(`SET_PRESENCE`) 연결을 하나로 센다(로그인한 사람의 연결도 링크로 읽으면 센다). 다 찼으면 새 연결의 구독은 받지 않고 `UNSUBSCRIBED_BLOCKS`(`reason` `public link connections full`)로 알리며, 방문은 받지 않아 보드의 "익명 n명"에 세지 않는다. 이미 읽는 연결과 보드의 구성원이 보통으로 여는 연결에는 닿지 않는다. 0은 시작 오류다 |
| `public_link_connections_per_address` | `FVOCI_PUBLIC_LINK_CONNECTIONS_PER_ADDRESS` | 정수(1 이상) | `10` | 위의 연결 가운데 한 주소에서 오는 것의 수. IPv6 주소는 /64로 센다. 주소는 요청 로그와 로그인 제한이 쓰는 것이다(`trusted_proxies` 뒤면 `X-Forwarded-For`). 넘으면 위와 같다. 0은 시작 오류다 |
| `compress_responses` | `FVOCI_COMPRESS_RESPONSES` | 참·거짓 | `true` | 참이면 API의 JSON 응답을 클라이언트의 `Accept-Encoding`에 따라 zstd, brotli, gzip으로 압축한다(32바이트 미만과 오류 응답은 그대로). 압축하는 프록시나 CDN 뒤라면 거짓으로 둔다. 내보내는 아카이브와 파일은 이것과 관계없이 압축하지 않는다(길이를 그대로 보낸다). 웹앱의 정적 파일은 빌드가 만든 `.zst`, `.br`, `.gz`를 고르므로 이것과 관계없다 |

## 키: 데이터베이스

한 데이터베이스는 서버 하나가 쓴다. 서버 여럿이 한 데이터베이스를 나눠 쓰는 배치는 지원하지 않는다: WebSocket의 알림, 로그인 제한, 열린 업로드의 한도가 서버의 메모리에서 셈을 하고, 끝나지 않은 업로드의 부분은 그 서버의 `tmp_path`에 있다. 서버는 시작할 때, 스키마를 고치거나 남은 것을 치우기 전에 그 데이터베이스의 "서버 잠금"을 쥔다. PostgreSQL은 풀 밖의 전용 연결이 쥐는 세션 advisory lock이고(키는 표가 든 스키마의 해시. 다른 스키마의 서버끼리는 막지 않는다), SQLite는 DB 파일 곁의 `<DB 파일>-server.lock`의 파일 잠금이다(심볼릭 링크는 따라가 실제 파일 곁에 둔다. 하드 링크는 다른 이름으로 치므로 한 DB에 하드 링크 둘로 서버 둘을 띄우지 않는다. 메모리의 DB는 잠그지 않는다). 다른 프로세스가 쥐고 있으면 `server_lock_wait_seconds`만큼 0.25초마다 다시 해 보다가(DB에 잠깐 닿지 못한 것도 다시 해 본다), 누가 쥐고 있는지(PostgreSQL은 그 연결의 `application_name`, 백엔드 pid, 연결된 때. SQLite는 잠금 파일에 적힌 것. 둘 다 `fvoci-server process <pid>`) 말하고 시작을 거절한다. 서버는 잠금을 5초마다 보고, 잃으면(전용 연결이 끊김, 10초 안에 답이 없음) 배경 작업을 멈추고 준비 상태(`GET /api/v1/ready`의 `database`)를 거짓으로 하고 내려간다. 잃은 것을 알기까지 최대 15초와 요청을 마저 보내는 10초 동안은 그 서버의 요청이 아직 돌 수 있다: advisory lock은 그 틈을 막지 못한다. 다만 잃은 것을 안 뒤로는 객체 하나를 지우기 직전마다 잠금을 다시 보므로 더 지우지 않는다. 끝낼 때는 DB를 닫은 뒤 잠금을 놓는다. 5초 안에 닫히지 않으면(빌려 간 연결의 트랜잭션이 아직 돌면) 잠금을 스스로 놓지 않는다: 서버 프로세스는 곧 끝나고, 잠금은 그 트랜잭션의 연결과 함께 끊긴다(같은 프로세스에서 계속 돈다면 연결이 다 돌아온 뒤에 놓는다). 그래서 대개 재시작하는 서버는 앞선 서버가 내려가는 대로 0.25초 안에 잠금을 얻는다. 앞선 서버의 기기가 죽어 연결이 끊긴 것을 모르는 때는 PostgreSQL이 그 연결을 TCP keepalive(10초 뒤부터 5초마다 세 번)로 30초 안팎에 끊는다. 잠금의 연결은 DB 서버의 세션 하나를 혼자 써야 한다: PgBouncer 같은 풀러는 세션 모드로만 거친다(트랜잭션 모드는 잠금이 다른 클라이언트에게 넘어간다). 새 서버가 준비된 뒤에 옛 서버를 내리는 배포(Kubernetes의 기본 rolling update)는 새 서버가 잠금을 얻지 못해 끝나지 않는다: 옛 서버를 먼저 내린다(`Recreate`).

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `dbtype` | `FVOCI_DBTYPE` | 문자열 | `"sqlite3"` | `sqlite3` 또는 `postgres` |
| `dbconfig` | `FVOCI_DBCONFIG` | 문자열 | `"./fvoci.db"` | 연결 문자열. SQLite는 파일의 경로, PostgreSQL은 `postgres://사용자:비밀번호@호스트:포트/DB?sslmode=disable` 꼴이다. 다른 테이블과 한 DB를 나눠 쓰려면 `search_path`로 이 서버만의 스키마를 준다. 비밀번호가 들어 있을 수 있어 로그에 남기지 않는다 |
| `dbpingattempts` | `FVOCI_DBPINGATTEMPTS` | 정수(0 이상) | `5` | 시작할 때 DB에 닿으려고 해 보는 횟수. 0이어도 한 번은 해 본다 |
| `server_lock_wait_seconds` | `FVOCI_SERVER_LOCK_WAIT_SECONDS` | 정수(0 이상) | `30` | 시작할 때 다른 프로세스가 쥔 서버 잠금(위)을 기다리는 시간(초). 지나면 시작을 거절한다. 0이면 기다리지 않는다. 재시작에서 앞선 서버가 내려가는 시간보다 길게 둔다 |
| `org_creation` | `FVOCI_ORG_CREATION` | 문자열 | `"first_user_only"` | 서버가 조직을 하나만 두는가(`first_user_only`), 여럿을 받는가(`open`). `open`은 `dbtype`이 `sqlite3`이면 시작하지 않는다(SQLite는 조직 하나만 담는다). PostgreSQL에서는 조직의 행을 행 단위 보안 정책이 갈라 둔다. 서버가 붙는 DB 역할이 슈퍼유저이거나 `BYPASSRLS`이면 그 정책이 걸리지 않는다: `open`이면 DB에 아무것도 만들기 전에 시작을 거부하고, `first_user_only`면 경고를 남기고 시작한다. 그 밖의 값은 시작 오류다. 가입(`register`)이 이 값을 따른다: `first_user_only`에서는 첫 사용자만 가입해 조직을 시작하고 그 뒤의 사람은 초대 링크로만 들어온다(403 `invitation_required`). `open`에서는 가입하는 사람마다 새 조직을 시작한다 |

## 키: 파일 저장소

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `filesdriver` | `FVOCI_FILESDRIVER` | 문자열 | `"local"` | `local`(디스크) 또는 `amazons3`(S3와 S3 호환 저장소). 그 밖의 값이면 시작하지 않는다 |
| `filespath` | `FVOCI_FILESPATH` | 문자열 | `"./files"` | `local`일 때 파일을 두는 디렉터리. 파일은 그 아래 `.tmp/writing`에 다 쓴 뒤 제자리로 옮긴다(같은 파일시스템이라 원자적이다. `tmp_path`와 상관없다). 이 DB의 서버만 쓴다: 관리 소켓의 파일 대조(`removeUnrecorded`)는 서버가 만드는 모양의 자리(`boards/…`)에서 이 DB가 이름하지 않는 파일을 지운다. 준비 상태(`GET /api/v1/ready`)는 5초에 한 번까지 `<filespath>/.fvoci-ready`를 같은 길로 써서 읽고 지워 보고, 디렉터리가 없으면 만든다 |
| `maxfilesize` | `FVOCI_MAXFILESIZE` | 정수(1 이상, 바이트) | `104857600` | 올릴 수 있는 파일의 가장 큰 크기(100 MiB). 0이나 음수는 시작 오류다(예전에는 0이 "한도 없음"이었다). `upload_part_bytes` × 10,000(S3 멀티파트의 부분 수 한도)을 넘어도 시작 오류다. 요청 하나로 올리는 파일(`uploadFile`)은 그에 더해 `upload_part_bytes`까지다. 사용자 한 명이 열린 업로드에 걸어 둘 수 있는 크기는 이 한도와 `max_import_bytes` 가운데 큰 것의 두 배다 |
| `filess3config.accesskeyid` | — | 문자열 | `""` | 액세스 키. **비워 두면 S3 클라이언트가 환경에서 자격을 읽는다**(`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN` 등. 아래 "S3의 예") |
| `filess3config.secretaccesskey` | — | 문자열 | `""` | 비밀 키. `accesskeyid`가 있을 때만 쓴다. 로그에 남기지 않는다 |
| `filess3config.bucket` | `FVOCI_FILESS3CONFIG_BUCKET` | 문자열 | `""` | 버킷. `amazons3`이면 비어 있을 수 없다. 자격에는 읽고 쓰고 지우는 것 말고 버킷을 나열하는 권한(`s3:ListBucket`)도 있어야 한다: 없는 파일을 물으면 그것 없이는 403이 오고, 준비 상태(`GET /api/v1/ready`)는 작은 파일(`<pathprefix>/.fvoci-ready`)을 써서 읽고 지워 본다(5초에 한 번까지). 그래서 자격은 그 키에도 쓰고 읽고 지울 수 있어야 한다. 버전 관리가 켜진 버킷에서는 검사마다 그 키에 옛 판과 지움 표시가 쌓이므로, 그 키에 옛 판을 하루 뒤 지우는 수명 주기 규칙(NoncurrentVersionExpiration, ExpiredObjectDeleteMarker)을 건다 |
| `filess3config.pathprefix` | `FVOCI_FILESS3CONFIG_PATHPREFIX` | 문자열 | `""` | 모든 객체 키 앞에 붙는 접두어. 앞뒤의 `/`는 떼고 쓴다. 버킷의 이 접두어 아래는 이 DB의 서버만 쓴다(`filespath`의 파일 대조와 같다) |
| `filess3config.region` | `FVOCI_FILESS3CONFIG_REGION` | 문자열 | `""` | 버킷의 리전. 비우면 S3 클라이언트의 기본값이다 |
| `filess3config.endpoint` | `FVOCI_FILESS3CONFIG_ENDPOINT` | 문자열 | `""` | AWS가 아닌 S3 호환 저장소의 호스트(포트 포함, 스킴 없이). 있으면 경로 방식(`호스트/버킷/키`)으로 부른다. 비우면 AWS다 |
| `filess3config.ssl` | `FVOCI_FILESS3CONFIG_SSL` | 참·거짓 | `false` | `endpoint`에 HTTPS로 닿는지. 거짓이면 HTTP도 허락한다 |
| `filess3config.signv2` | `FVOCI_FILESS3CONFIG_SIGNV2` | 참·거짓 | `false` | 옛 서명 방식 v2. **받지 않는다**: `filesdriver`가 `amazons3`인데 참이면 시작하지 않는다(요청은 v4로 서명한다) |
| `filess3config.sse` | `FVOCI_FILESS3CONFIG_SSE` | 참·거짓 | `false` | 참이면 저장하는 객체에 서버 쪽 암호화(SSE-S3, `AES256`)를 청한다 |
| `filess3config.timeout` | `FVOCI_FILESS3CONFIG_TIMEOUT` | 정수(밀리초) | `0` | S3 요청의 시간 제한. 0 이하면 클라이언트의 기본값이다 |

**S3의 예.** MinIO 같은 호환 저장소에 HTTP로 닿는다.

```json
{
  "filesdriver": "amazons3",
  "filess3config": {
    "bucket": "fvoci",
    "endpoint": "minio.internal:9000",
    "region": "us-east-1",
    "ssl": false,
    "pathprefix": "files"
  }
}
```

자격은 파일에 적지 않고 환경에서 준다.

```sh
AWS_ACCESS_KEY_ID=… AWS_SECRET_ACCESS_KEY=… fvoci-server --config config.json
```

`filess3config`의 키 가운데 자격(`accesskeyid`, `secretaccesskey`)이 아닌 것은 `FVOCI_FILESS3CONFIG_`에 키를 대문자로 이은 환경 변수로도 바꾼다(표의 "환경 변수" 칸). 컨테이너 하나를 여러 환경에 띄울 때 쓴다. 자격은 `FVOCI_*`로 받지 않는다(`FVOCI_FILESS3CONFIG_ACCESSKEYID`나 `…_SECRETACCESSKEY`가 공백만이 아니면 시작하지 않는다). `accesskeyid`를 비우고 위처럼 표준 `AWS_*` 변수로 준다.

청크로 올리는 파일(`createUpload`)은 S3에서 멀티파트 업로드가 된다. 세션을 열 때 최종 자리에 멀티파트 업로드를 시작하고, 부분마다 확인한 뒤 버킷으로 보내고, 완료할 때 객체로 만든다. 버리거나 끝난 세션은 서버가 중단(abort)하고, 그 자리에 파일의 기록이 없는 객체가 있으면(완료의 답을 잃었거나 기록하기 전에 서버가 멈춘 것) 그것도 지운다. 서버가 멈춰 세션이 남으면 다음 정리에서 그렇게 한다. 서버가 그 사이에 멈추면 중단하지 못한 멀티파트 업로드가 버킷에 남아 저장 요금이 나가므로(S3의 멀티파트 업로드에는 만료가 없다), 버킷에 수명 주기 규칙 `AbortIncompleteMultipartUpload`를 둔다. 세션은 24시간까지이므로 2일이면 넉넉하다. AWS CLI로는:

```sh
aws s3api put-bucket-lifecycle-configuration --bucket 버킷 --lifecycle-configuration \
  '{"Rules":[{"ID":"abort-incomplete-uploads","Status":"Enabled","Filter":{},"AbortIncompleteMultipartUpload":{"DaysAfterInitiation":2}}]}'
```

Cloudflare R2는 같은 규칙이 기본으로 7일이다. 다른 S3 호환 저장소는 그 문서를 본다.

## 키: 세션

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `session_idle_days` | `FVOCI_SESSION_IDLE_DAYS` | 정수(1 이상) | `14` | 쓰지 않은 채 이만큼의 날이 지나면 세션이 끝난다 |
| `session_max_days` | `FVOCI_SESSION_MAX_DAYS` | 정수(1 이상) | `90` | 로그인한 뒤 이만큼의 날이 지나면 얼마나 쓰든 세션이 끝난다 |

세션 쿠키에 따로 설정은 없다. 이름과 `Secure`는 `serverRoot`가 정한다(위).

## 키: 로그인 제한과 비밀번호 작업

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `login_pair_free_failures` | `FVOCI_LOGIN_PAIR_FREE_FAILURES` | 정수(0 이상) | `5` | 한 주소가 한 계정에 연달아 실패해도 기다리지 않는 횟수 |
| `login_pair_base_delay_seconds` | `FVOCI_LOGIN_PAIR_BASE_DELAY_SECONDS` | 정수(1 이상, 10년 이하) | `30` | 공짜가 아닌 첫 실패 뒤의 기다림(초). 실패마다 두 배가 된다 |
| `login_pair_max_delay_seconds` | `FVOCI_LOGIN_PAIR_MAX_DELAY_SECONDS` | 정수(1 이상, 10년 이하) | `3600` | 한 주소가 한 계정에서 기다리는 가장 긴 시간(초). `login_pair_base_delay_seconds`보다 작을 수 없다 |
| `login_account_max_failures` | `FVOCI_LOGIN_ACCOUNT_MAX_FAILURES` | 정수(1 이상) | `100` | 어디서든 연달아 이만큼 실패하면 계정을 잠근다 |
| `login_account_lock_seconds` | `FVOCI_LOGIN_ACCOUNT_LOCK_SECONDS` | 정수(1 이상, 10년 이하) | `3600` | 계정이 잠겨 있는 시간(초) |
| `login_address_attempts_per_minute` | `FVOCI_LOGIN_ADDRESS_ATTEMPTS_PER_MINUTE` | 정수(0 이상) | `20` | 한 주소가 1분에 해 볼 수 있는 로그인과 가입, 초대 링크의 미리보기와 수락. 한 주소의 시도는 한 번에 하나씩 지나간다. 0이면 주소로 세지 않는다(모든 사용자가 한 주소에서 오고 프록시도 없는 서버) |
| `password_hash_concurrency` | `FVOCI_PASSWORD_HASH_CONCURRENCY` | 정수(1 이상) | `4` | 한꺼번에 하는 비밀번호 해시와 확인. 하나에 19 MiB와 몇 밀리초의 처리기가 든다 |
| `password_hash_queue` | `FVOCI_PASSWORD_HASH_QUEUE` | 정수(0 이상) | `16` | 해시할 차례를 기다릴 수 있는 요청의 수 |
| `password_hash_queue_seconds` | `FVOCI_PASSWORD_HASH_QUEUE_SECONDS` | 정수(1 이상) | `5` | 차례를 기다리는 시간(초). 넘으면 다시 해 보라고 답한다(429) |

## 키: 프록시

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `trusted_proxies` | `FVOCI_TRUSTED_PROXIES` | 문자열 목록 | `[]` | 서버 앞의 역방향 프록시의 주소나 주소 범위(`10.0.0.0/8` 꼴). 이 주소에서 온 요청만 `X-Forwarded-For`가 말하는 주소에서 온 것으로 본다. 비우면 클라이언트가 서버에 바로 닿는다고 본다. 읽을 수 없는 값이 있으면 시작하지 않는다. `serverRoot`가 https이고 `useSSL`이 거짓인데 이 목록이 비면 시작할 때 경고한다(모든 클라이언트가 프록시 하나로 보여 로그인 제한이 모두에게 한꺼번에 걸리고, 공개 링크로 한 보드를 읽는 연결도 `public_link_connections_per_address`개까지만 받는다) |

## 키: 한도와 휴지통, 임시 파일

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `max_request_bytes` | `FVOCI_MAX_REQUEST_BYTES` | 정수(1 이상, 바이트) | `4194304` | JSON을 받는 API 요청 본문의 가장 큰 크기(4 MiB). 넘으면 413. 파일 올리기(`uploadFile`과 `putUploadPart`)에는 쓰지 않는다. 아카이브 가져오기(`importArchive`)는 업로드를 이름으로만 받는 JSON이라 이것을 따른다. 세션 없는 요청(64 KiB)과 WebSocket 메시지(64 KiB. Binary 프레임은 `collab_update_bytes`에 32바이트를 더한 것이 더 크면 그것까지)의 한도는 설정이 아니다 |
| `trash_retention_days` | `FVOCI_TRASH_RETENTION_DAYS` | 정수(1 이상) | `30` | 지운 보드와 블록이 휴지통에 남아 되살릴 수 있는 날. 지나면 영구히 지운다. 어느 블록도(휴지통의 것까지) 가리키지 않는 파일도 그렇게 된 지 이만큼 지나면 지운다 |
| `upload_part_bytes` | `FVOCI_UPLOAD_PART_BYTES` | 정수(5242880–5368709120, 바이트) | `8388608` | 청크로 올릴 때(`createUpload`) 마지막을 뺀 부분 하나의 크기(8 MiB). S3 멀티파트가 받는 부분이 5 MiB에서 5 GiB라서 그 밖이면 시작하지 않는다. 서버 앞의 프록시가 받는 요청 본문보다 작아야 한다(Cloudflare의 Free와 Pro는 100 MB). `filesdriver`가 `amazons3`이어도 부분은 `tmp_path`의 파일로 받아 확인한다. 버킷에 보내는 동안만 메모리에 있고, 그 메모리는 이 크기의 8배까지다(기본 64 MiB). 넘으면 다음 부분은 디스크에서 기다린다 |
| `max_pending_upload_bytes` | `FVOCI_MAX_PENDING_UPLOAD_BYTES` | 정수(`maxfilesize`와 `max_import_bytes` 이상, 바이트) | `2147483648` | 서버의 열린 업로드(`createUpload`, `createImportUpload`) 모두가 걸어 둘 수 있는 크기의 합(2 GiB). 넘는 업로드를 열면 429이고 `reason`이 `server_uploads_pending`이다. `maxfilesize`보다 작으면 가장 큰 파일을 언제나 429로 거절하게 되므로 시작 오류다 |
| `max_import_bytes` | `FVOCI_MAX_IMPORT_BYTES` | 정수(1 이상, 바이트) | `268435456` | 가져올 수 있는 아카이브의 가장 큰 크기(256 MiB). 아카이브도 청크로 올리므로 `upload_part_bytes` × 10,000을 넘으면 시작 오류이고, `max_pending_upload_bytes`보다 커도 시작 오류다. 풀린 합계는 1 GiB까지다(고정). 메모리 과할당을 막은 기기(`vm.overcommit_memory=2`)에서는 항목 수를 꾸민 아카이브 하나가 서버를 멈출 수 있다(`docs/contract-pending.md` 5절) |
| `max_concurrent_exports` | `FVOCI_MAX_CONCURRENT_EXPORTS` | 정수(1 이상) | `2` | 서버가 한꺼번에 하는 내보내기(`exportBoardArchive`, `exportWorkspaceArchive`)의 수. 한 사용자는 하나씩이다(설정이 아니다). 내보내기는 아카이브를 다 보낼 때까지(클라이언트가 끊거나 `export_stall_seconds` 동안 아무것도 받지 않을 때까지) 자리를 잡는다. 넘으면 429이고 `reason`이 `server_exports_busy`(사용자의 것이 돌고 있으면 `export_running`)다. 0은 시작 오류다 |
| `export_stall_seconds` | `FVOCI_EXPORT_STALL_SECONDS` | 정수(1 이상) | `60` | 내보내기의 내려받기가 이 시간(초) 동안 한 바이트도 보내지 못하면 그만둔다: 임시 파일을 지우고 자리를 돌려주고 연결을 끊는다. 보낸 양은 서버의 버퍼(커널의 송신 버퍼와 HTTP 계층, 합쳐 몇 MiB)를 떠난 양이라, 클라이언트는 이 시간에 그만큼은 받아야 끊기지 않는다. 0은 시작 오류다 |
| `max_concurrent_imports` | `FVOCI_MAX_CONCURRENT_IMPORTS` | 정수(1 이상) | `2` | 서버가 한꺼번에 하는 가져오기(`importArchive`)의 수. 한 사용자는 하나씩이다. 넘으면 429이고 `reason`이 `server_imports_busy`(사용자의 것이 돌고 있으면 `import_running`)이며 업로드는 열린 채 남는다. 열린 업로드의 한도(`max_pending_upload_bytes`)는 따로다. 0은 시작 오류다 |
| `max_comment_versions` | `FVOCI_MAX_COMMENT_VERSIONS` | 정수(1 이상) | `50` | 댓글마다 남기는 고치기 전 글(판)의 수. 맨 처음 쓴 글은 늘 남고 그 수에 든다. 넘으면 그 다음으로 오래된 판부터 버린다(`getCommentVersions`). 줄이면 그 뒤에 고쳐지는 댓글부터 줄어든다. 0은 시작 오류다 |
| `wiki_page_depth` | `FVOCI_WIKI_PAGE_DEPTH` | 정수(1 이상) | `10` | 위키의 페이지가 들어갈 수 있는 깊이(맨 위의 페이지가 1). 더 깊게 만들거나 옮기거나 되살리거나 복제하면 400이고 `reason`이 `page_too_deep`이다(만들기와 옮기기는 `errors[0].pointer`가 `/parentId`). 조직의 한도로 읽는다(`App::limits`). 0은 시작 오류다 |
| `wiki_pages` | `FVOCI_WIKI_PAGES` | 정수(1 이상) | `5000` | 위키 하나가 가질 수 있는 페이지의 수(휴지통의 것은 세지 않는다). 넘게 만들거나 되살리거나 복제하면 400이고 `reason`이 `too_many_pages`, `limit`이 이 값이다. 조직의 한도로 읽는다(`App::limits`). 0은 시작 오류다 |
| `tmp_path` | `FVOCI_TMP_PATH` | 문자열 | `""` | 서버의 임시 파일을 두는 디렉터리. 청크로 올리는 부분은 `filesdriver`가 `amazons3`이어도 여기에 받는다(S3로 보낸 뒤 지운다). 업로드에 필요한 자리는 열린 업로드가 걸어 둔 크기의 합까지다(`max_pending_upload_bytes`). 가져오는 중인 아카이브는 업로드가 끝난 것으로 세어 이 합에 들지 않고, 가져오는 동안 아카이브 하나와 부분 하나, 풀어 낸 파일 항목 하나(`maxfilesize`까지)의 자리를 차지한다. 동시에 가져오는 수는 `max_concurrent_imports`까지다. 내보내는 아카이브는 쓰고 보내는 동안 자리를 차지하고, 그 크기에는 한도가 없고 수는 `max_concurrent_exports`까지다. 끝나지 않은 업로드의 부분(`uploads/<uploadId>/`), 가져오는 중인 아카이브(`uploads/<uploadId>.importing-…/`)와 내보내는 중인 아카이브(`exports/`. 다 보내거나 그만두면 지운다)를 둔다. 그 아래 `uploads`와 `exports`는 서버의 것이다: 서버가 시작할 때 어느 업로드의 것도 아닌 것과 내보내다 남은 것을 지운다. 한 디렉터리를 두 서버가 함께 쓰지 않는다(`filespath`도 그렇다. 시작할 때 `filespath/.tmp/writing`을 비운다). 이 청소는 서버 잠금을 쥔 뒤에 하므로 같은 DB의 둘째 서버는 하지 못하지만, DB가 다른 두 서버가 한 디렉터리를 쓰는 것은 막지 않는다. 비우면 `filespath` 아래 `.tmp`다. 시스템의 임시 디렉터리는 메모리에 있을 수 있어 쓰지 않는다 |

## 키: 본문의 공동 편집

카드 본문을 함께 고치는 동안 그 Y 문서는 서버가 자기 실행 파일을 작업자 모드로 띄운 자식 프로세스(작업자)가 쥔다. 서버와 작업자는 표준 입출력의 프레임으로만 이야기하고, 작업자는 파일·소켓·DB를 열지 않는다. 작업자가 죽거나 시간 안에 답하지 않으면 서버가 죽이고 다시 띄우며, 문서는 저장된 마지막 상태에서 다시 열린다. 잇달아 죽으면 다시 띄우기 전에 0.1초부터 두 배씩, 10초까지 기다린다. 한 문서에 대한 요청이 15분 안에 두 번 작업자를 죽이면 그 문서는 10초 동안 닫히고(`document_unavailable`), 거듭될 때마다 두 배씩, 5분까지 닫힌다.

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `collab_workers` | `FVOCI_COLLAB_WORKERS` | 정수(1–64) | `1` | 띄우는 작업자의 수. 문서는 열 때 가장 적게 받은 작업자에게 간다 |
| `collab_documents_per_worker` | `FVOCI_COLLAB_DOCUMENTS_PER_WORKER` | 정수(1 이상) | `500` | 작업자 하나가 한꺼번에 쥐는 문서의 수. 그 밖에 열린 문서가 없는 카드의 기록을 카드에 쓰려고 잠깐 여는 문서는 서버 전체로 넷까지 따로 쥔다(모자라면 기다린다). 작업자는 문서들의 상태를 합쳐 `collab_worker_memory_bytes`의 8분의 1까지만 쥔다. 넘게 하는 열기와 업데이트는 거절한다 |
| `collab_state_bytes` | `FVOCI_COLLAB_STATE_BYTES` | 정수(2097152–1073741824, 바이트) | `8388608` | 문서 하나의 저장된 상태의 가장 큰 크기(8 MiB). 문서는 지운 것도 쥐고 있어 고칠수록 자란다. 넘게 하는 업데이트는 거절하고, 4분의 3에 이르면 서버가 카드의 본문에서 문서를 새로 만든다(편집자는 새로 받는다). 낮춰도 이미 저장된 문서는 받아들인 그대로 열리고, 넘는 문서는 본문을 카드에 쓴 뒤 새로 만든다 |
| `collab_update_bytes` | `FVOCI_COLLAB_UPDATE_BYTES` | 정수(1–`collab_state_bytes`, 바이트) | `65536` | 클라이언트가 보내는 업데이트 하나의 가장 큰 크기(64 KiB). 업데이트 하나에 드는 작업자의 시간은 주로 문서의 크기를 따른다(릴리스 빌드에서 1 MB 문서에 20 KiB 업데이트가 약 30 ms). 받아들일 때의 한도다: 이미 기록된 업데이트는 낮춘 뒤에도 다시 적용된다 |
| `collab_worker_timeout_ms` | `FVOCI_COLLAB_WORKER_TIMEOUT_MS` | 정수(1 이상, 밀리초) | `10000` | 작업자의 답을 기다리는 시간. 넘으면 그 요청은 거절되고 작업자는 죽였다가 다시 띄운다 |
| `collab_recheck_seconds` | `FVOCI_COLLAB_RECHECK_SECONDS` | 정수(1–3600, 초) | `30` | 본문을 함께 고치는 방(`contracts/websocket.md` 6절)에 든 연결마다 아직 들어 있어도 되는지, 쓰는지 읽기만 하는지를 다시 묻는 간격. 보드에서 빠지거나 역할이 내려간 사람은 보통 이 안에, 길어야 이것의 두 배(묻는 데 걸리는 시간이 한 간격까지다) 뒤에 내보내지거나 읽기 전용이 된다. 한 번 물을 때 방에 든 사용자 하나에 DB 읽기 다섯 번이고(같은 사용자가 같은 방식(비밀번호, 어느 공급자)으로 만든 세션의 연결 여럿은 한 번에 묻는다), 한 방에서 여덟 사용자씩 함께 묻는다. 저장소가 답하지 못했거나 한 간격 안에 답을 얻지 못한 사용자는 2초 뒤에 한 번 더 묻고, 그래도 답이 없으면 그 연결을 방에서 내보낸다. 조직이 비밀번호를 받지 않게 되면(`ssoRequired`) 그 공급자로 만들지 않은 세션의 연결은 이 간격을 기다리지 않고 곧바로 그 조직의 방에서 나간다 |
| `collab_documents_per_connection` | `FVOCI_COLLAB_DOCUMENTS_PER_CONNECTION` | 정수(1–1024) | `8` | WebSocket 연결 하나가 한꺼번에 들 수 있는 방의 수. 넘는 `JOIN_DOC`은 `DOC_REFUSED`이고 `reason`이 `too_many_documents_joined`다 |
| `collab_connections_per_document` | `FVOCI_COLLAB_CONNECTIONS_PER_DOCUMENT` | 정수(1–10000) | `50` | 본문 하나의 방에 들 수 있는 연결의 수(같은 사용자의 탭도 하나씩 센다). 넘는 `JOIN_DOC`은 `DOC_REFUSED`이고 `reason`이 `document_full`이다. 변경 하나는 방의 다른 연결 모두에게 간다 |
| `collab_connections_per_user` | `FVOCI_COLLAB_CONNECTIONS_PER_USER` | 정수(1–10000) | `5` | 본문 하나의 방에 든 연결 가운데 한 사용자의 것일 수 있는 수(탭과 기기마다 하나). 넘는 `JOIN_DOC`은 `DOC_REFUSED`이고 `reason`이 `too_many_connections_joined`다. 한 사용자가 방(`collab_connections_per_document`)을 혼자 채우지 못하게 한다 |
| `collab_frames_per_second` | `FVOCI_COLLAB_FRAMES_PER_SECOND` | 정수(10–1000000) | `50` | 연결 하나가 1초에 보낼 수 있는 방의 Binary 프레임(sync 메시지와 커서)의 수. 쉬고 난 뒤에는 이만큼을 한꺼번에 보낼 수 있고 그 뒤로 1초에 이만큼씩이다. 문서 전체로 답하는 `JOIN_DOC`과 sync step 1은 다섯으로, `LEAVE_DOC`은 하나로 센다. 서버가 그 연결의 일을 하느라 읽지 못한 동안 쌓인 것은 그 시간만큼 더 받는다(60초 치까지): 한도 안에서 보낸 클라이언트가 서버가 느려서 넘지는 않는다. 넘는 `JOIN_DOC`은 `DOC_REFUSED`, 넘는 프레임은 그 프레임의 방에서 내보내기(`DOC_LEFT`)이고 `reason`이 `too_many_frames`다. 커서 프레임만은 넘어도 버릴 뿐이다 |
| `collab_queue_messages` | `FVOCI_COLLAB_QUEUE_MESSAGES` | 정수(16–65536) | `512` | 연결 하나에 보내려고 기다리게 두는 방의 메시지의 수. 바이트로는 `collab_state_bytes`의 두 배까지다(설정이 아니다). 넘으면 그 연결을 그 방에서 내보내고(`DOC_LEFT`, `connection_too_slow`) 방의 다른 연결은 그대로 받는다. 구독의 메시지(512개, 넘으면 연결을 닫는다)와는 따로 센다 |
| `collab_body_versions` | `FVOCI_COLLAB_BODY_VERSIONS` | 정수(2–1000) | `50` | 카드 하나의 본문에서 남겨 두는 판의 수(본문을 통째로 쓰면 앞의 본문과 쓴 본문이 함께 남으므로 둘부터다). 새 판이 생겨 이 수를 넘으면 가장 오래된 판부터 지운다(판의 번호는 이어서 센다). 판 하나는 본문 전체이고, 판이 보여 주는 이미지의 파일은 그 판이 남아 있는 동안 지우지 않는다. 낮추면 카드마다 다음 판이 생길 때 넘는 판이 지워진다 |
| `collab_version_minutes` | `FVOCI_COLLAB_VERSION_MINUTES` | 정수(1–1440, 분) | `10` | 본문을 함께 고치는 동안 판을 남기는 간격. 어느 판에도 없는 첫 변경에서 이만큼 지난 뒤 본문이 카드에 쓰일 때(또는 그다음 변경을 받을 때) 판을 남긴다. 그 밖에 가장 새 판 뒤로 본문을 고친 사람이 모두 방을 떠날 때(고치지 않은 사람은 남아 있어도 된다), 본문을 통째로 쓸 때, 서버가 멈출 때 남기고, 죽은 서버가 남기지 못한 판은 다음 시작 때 남긴다. 본문이 가장 새 판과 같으면 남기지 않는다. 고치다가 멈춘 채 방에 머무는 동안에는 시간이 지나도 남기지 않고, 떠나거나 다시 고칠 때 남긴다 |
| `collab_worker_memory_bytes` | `FVOCI_COLLAB_WORKER_MEMORY_BYTES` | 정수(268435456 이상이고 `collab_state_bytes`의 8배 이상, 바이트) | `1073741824` | 작업자의 주소 공간 한도(1 GiB, 유닉스의 `RLIMIT_AS`). 유닉스에서는 그 밖에 코어 파일 0, 열린 파일 16, 자기 프로세스 그룹, 리눅스에서는 서버가 죽으면 함께 죽기가 걸린다. Windows에는 이 한도가 없다 |

## 키: 공유와 보이는 것

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `enablePublicSharedBoards` | `FVOCI_ENABLEPUBLICSHAREDBOARDS` | 참·거짓 | `false` | 참이면 보드를 읽기 토큰으로 공개 공유할 수 있다. 거짓이면 어느 읽기 토큰도 맞지 않는다 |
| `featureflags` | — | 문자열에서 문자열로의 객체 | `{}` | 웹앱에 넘기는 기능 표시(`getClientConfig`의 `featureFlags`) |
| `teammatenamedisplay` | `FVOCI_TEAMMATENAMEDISPLAY` | 문자열 | `"username"` | 웹앱이 다른 사용자의 이름을 보이는 방식(`getClientConfig`의 `teammateNameDisplay`) |
| `showemailaddress` | `FVOCI_SHOWEMAILADDRESS` | 참·거짓 | `false` | 사용자 목록에 이메일을 싣는지 |
| `showfullname` | `FVOCI_SHOWFULLNAME` | 참·거짓 | `false` | 사용자 목록에 이름과 성을 싣는지 |

## 키: 관리자 소켓과 웹훅

관리 소켓에서 `POST /api/v1/admin/users/{username}/password-reset`을 보내면 `url`에 재설정 링크를 받는다. 운영자가 사용자에게 전달한다. 링크는 256비트 비밀값을 URL 조각(`#` 뒤)에 담고 서버에는 그 해시만 남는다. 비밀번호가 있는 활성 계정만 발급받는다. `POST /api/v1/password-reset/complete`에 `token`과 새 `password`를 보내면 1시간 안에 한 번 쓸 수 있고, 비밀번호 변경과 모든 세션 삭제가 함께 저장된다. 열린 WebSocket도 닫힌다. 관리 경로는 HTTP 리스너에서 열리지 않는다. 메일이 닿지 않는 사람의 가입은 `POST /api/v1/admin/users/{username}/email-verification`이 링크처럼 확인한다(로그인시키지는 않는다; 확인을 기다리는 가입이 없으면 404). 구성원 링크로 한 가입이면 들어간 조직에 운영자의 기록(`email_verified_by_operator`)이 남고, 제 조직을 세우는 가입과 손님은 그 전에 속한 조직이 없어 기록이 없다.


| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `enableLocalMode` | `FVOCI_ENABLELOCALMODE` | 참·거짓 | `false` | 참이면 관리 API를 유닉스 소켓으로 연다(비밀번호 재설정, 관리자 지정 등) |
| `localModeSocketLocation` | `FVOCI_LOCALMODESOCKETLOCATION` | 문자열 | `"/var/tmp/fvoci_local.socket"` | 그 소켓의 경로 |
| `webhook_update` | `FVOCI_WEBHOOK_UPDATE` | 문자열 목록 | `[]` | 바뀐 블록마다 그 블록을 `POST`로 받는 주소들 |

## 키: 지표

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `prometheusaddress` | `FVOCI_PROMETHEUSADDRESS` | 문자열(`호스트:포트`) | `""` | Prometheus 지표를 내는 주소. 비어 있으면 지표를 모으지 않고 아무것도 듣지 않는다. 주소가 있으면 API와 **다른 리스너**를 그 주소에 열고 `GET /metrics`만 Prometheus의 글 형식으로 답한다(그 밖의 길은 404이고, 거절은 API와 같은 `application/problem+json`이며 그것이 가리키는 `/problems`의 글도 여기서 내준다). API의 리스너에는 지표의 길이 없다. 호스트는 이름, IPv4, 괄호 안의 IPv6(`[::1]:9092`)이고, 비우면(`:9092`) 모든 인터페이스이고, `localOnly`가 참이면 API처럼 루프백(`localhost`가 풀리는 주소 하나)이다. 포트 0은 빈 포트 아무것이고 고른 주소를 로그에 남긴다. 꼴이 틀리면, 그 주소에서 들을 수 없으면(이미 쓰임, 이 기기의 주소가 아님) 시작하지 않는다. 인증은 `prometheustoken`을 따르고, `usessl`과 관계없이 평문 HTTP다. 모니터링만 닿는 안쪽 주소(루프백, 사설망)에 묶는다. 요청 로그에 남기지 않는다. 긁을 때 DB를 세는 조회가 둘 있다: 열린 업로드는 5초에 한 번까지, 살아 있는 세션은 1분에 한 번까지(세션 표 전체를 훑는다). 긁기가 아무리 잦아도 DB에 묻는 것은 그만큼이다. 둘은 나란히 돌고 긁기는 각각 2초까지 기다리며, 같은 조회는 DB가 답할 때까지 동시에 하나다. 준비 상태(`GET /api/v1/ready`의 확인, 5초 동안 기억한다)도 긁을 때 보고 `fvoci_ready`로 낸다 |
| `prometheustoken` | `FVOCI_PROMETHEUSTOKEN` | 문자열 | `""` | 지표 리스너의 `GET /metrics`가 요구하는 bearer 토큰. 비어 있으면 인증이 없다. 값이 있으면 `Authorization: Bearer <토큰>`이 맞아야 하고, 없거나 틀리면 401(`WWW-Authenticate: Bearer realm="metrics"`, 틀린 것에는 `error="invalid_token"`)이며 그때는 아무것도 읽지 않는다. `Authorization` 머리글이 둘 이상이면 어느 것도 받지 않고 순서와 값에 상관없이 401(`error="invalid_request"`)이다. 환경 변수가 빈 값이면 파일의 값을 둔다. 글자열이 아닌 값(따옴표 없는 수)은 그 값을 되풀이하지 않는 오류로 시작하지 않는다. 설정돼 있으면 32자 이상이어야 하고, 짧으면(환경 변수로 준 것도) 키 이름과 '32자 이상'만 말하고 값도 그 길이도 말하지 않는 오류로 시작하지 않는다. 비교는 SHA-256 다이제스트를 일정 시간에 견준다. 글자는 RFC 6750의 것(영문자, 숫자, `-._~+/`, 끝의 `=`)만 받고, 아니면 시작하지 않는다. 로그, 오류 문구, `Debug` 출력에 나오지 않는다. 토큰이 있어도 평문 HTTP이므로 지표 리스너는 안쪽 주소에 둔다 |

## 키: 메일 보내기

메일은 선택 사항이다. 보내는 길은 `mail_transport`가 정한다: SMTP 릴레이(직접 설치), 또는 공급자의 HTTP API(Cloudflare Email Service, Amazon SES, OCI Email Delivery). 비우면 `smtp_host`가 있을 때 SMTP, 없으면 메일을 보내지 않는다. 고른 길이 읽지 않는 키(다른 공급자의 키, SMTP가 아닌 길의 `smtp_host`·`smtp_user`)가 있으면 그 키를 말하며 시작하지 않고, 고른 길에 필요한 키가 빠져도 그 키를 말하며 시작하지 않는다. 비밀 값(Cloudflare의 API 토큰, SES의 비밀 키, OCI 개인 키 파일의 내용)은 로그·오류·`Debug`에 나오지 않는다. SES에는 `SendEmail`(API v2)로 MIME 그대로(`Content.Raw`) 한 사람씩 보낸다. OCI에는 `SubmitEmail`(`/20220926/actions/submitEmail`)로 제목·평문·HTML을 한 사람씩 보내고, 요청은 API 키로 서명한다(HTTP Signatures, `rsa-sha256`). OCI의 메시지는 2 MB(머리글·base64 포함)까지다. OCI는 승인되지 않은 보내는 사람만이 아니라 없는 구획·정책도 `404 NotAuthorizedOrNotFound`로 답하고, Cloudflare는 틀린 계정 ID에 `404`로 답한다: 둘 다 모든 메일이 같은 답을 받는 설정의 잘못이라 길이 막힌 것으로 본다(아래). SES 샌드박스 계정은 확인된 주소에만, 하루 200통·초당 1통까지 보낸다(`mail_per_second` 기본 1이 맞는다). 공급자 쪽의 도메인·보내는 사람 승인은 운영자의 일이다: SES가 승인되지 않은 보내는 사람을 거절하면(`400`) 그 메일의 영구 실패이고 로그에는 응답 코드만 남는다. 메일이 켜지면 로그인 화면에서 비밀번호 재설정 메일을 요청하고, 사람 관리·손님 초대 화면에서 기존 초대 링크를 여러 주소로 보낸다. 초대 메일은 주소마다 따로 보낸다. 구성원 링크는 그대로 보내고 만료·사용 횟수를 바꾸지 않는다. 손님 링크는 한 사람이 한 번 쓰므로, 받는 사람마다 같은 보드·역할·만료의 한 번 쓰는 손님 링크(자식 링크)를 보낼 차례에 따로 만들어 보낸다. 자식 링크는 요청에 쓴 링크(부모)와 받는 주소를 적고, 만든 사람은 보낸 사람이다. 영구 실패했거나 포기한 메일의 링크는 거둔다(보낸 사람이 다시 보낼 수 있다). 시간 초과도 다시 시도하므로, 공급자가 늦게 받은 메일이 포기되면 받는 사람의 링크는 거둬져 있을 수 있다. 부모 링크는 그대로 남고 틀일 뿐이라, 쓰인 뒤에도 거두거나 만료되기 전에는 메일이 나간다. 부모 링크를 거두면 아직 쓰이지 않은 자식 링크도 함께 거둔다. 사람 관리 화면과 보드 공유 창의 손님 링크 목록은 자식 링크의 받는 주소를 보인다. 보내기 전마다 초대 권한과 링크를 다시 확인한다. 비밀번호 재설정 요청의 답은 계정 유무와 관계없이 같고, 계정 조회와 보내기는 배경 작업이다. 비밀번호 없는 계정에는 재설정 메일을 보내지 않는다. 요청은 출처 주소마다 로그인과 같은 설정(`login_address_attempts_per_minute`)으로 따로 세고, 이메일로는 세지 않는다: 남이 내 주소로 요청해도 나는 막히지 않는다. 한 계정에 가는 메일은 `mail_reset_per_account_hour`까지이고, 넘은 요청도 답은 같으며 아무것도 잠그지 않는다. 재설정을 마치면 그 계정(사용자 이름과 이메일)의 로그인 잠금과 재설정을 마친 주소의 기다림이 끝난다(다른 주소의 기다림은 남는다). 메일은 영어·한국어 평문과 HTML로 보낸다: 받는 사람의 언어 설정에 맞는 글이 있으면 그것, 없으면(`auto`, `ja` 같은 것) 보낸 사람의 설정과 요청한 화면의 언어, 그것도 없으면 영어다. `/ready`는 SMTP에 연결하지 않는다. 메일의 링크는 `serverroot`에서 나오므로 받는 사람이 닿는 공개 주소여야 한다(루프백 주소면 시작할 때 경고를 남긴다).

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `email_verification` | `FVOCI_EMAIL_VERIFICATION` | 불 | `false` | 켜면 가입은 계정을 만들지 않고(답은 202, 세션 없음) 가입한 주소로 확인 링크(`/verify-email#토큰`, 24시간, 한 번)를 보낸다. 링크를 열면 그때 계정과 조직·첫 워크스페이스를 만들고 그 브라우저를 로그인시킨다. 확인은 가입한 사람만 끝낸다: 가입한 브라우저(202가 준 `HttpOnly` 쿠키 `fvoci_signup`, https에서는 `__Host-fvoci_signup`, 7일, 세션이 아니다)에서는 링크만으로, 다른 곳에서는 가입 때 정한 비밀번호를 함께 내야 한다(없으면 403 `password_required`, 틀리면 로그인의 401과 같은 한도, 링크는 쓰이지 않는다). 그 전의 가입은 누구의 조회에도 나오지 않고 이름과 주소만 쥐며(같은 이름·주소의 가입은 409. 다만 가입한 브라우저에서 다시 가입하면 앞 가입을 버리고 받아, 주소를 고쳐 같은 사용자 이름으로 바로 가입할 수 있고 초대 링크의 쓰임도 돌아온다), 7일 안에 확인하지 않으면 주기 청소가 지워 이름과 주소가 다시 풀린다. 확인 전의 로그인은 비밀번호가 맞을 때만 403(`email_unverified`)이고, 틀리면 계정이 없을 때와 같은 401이다. 확인 메일은 재설정 메일과 같은 줄·차례로 가고 한 주소에 시간당 `mail_reset_per_account_hour`통까지다. 메일을 보낼 길이 없으면 이 키를 말하며 시작하지 않는다. 켜기 전에 만든 계정, 공급자로 만든 계정은 그대로 들어온다. 조직의 공급자가 주소를 증명하며 들어오면(새 계정이든 있던 계정이든) 같은 주소로 그 조직의 초대 링크에서 기다리던 가입은 그때 지워진다(이름과 링크의 쓰임이 풀린다). `getLoginOptions`의 `emailVerification`이 웹에 알린다 |
| `mail_transport` | `FVOCI_MAIL_TRANSPORT` | 문자열 | `""` | `smtp`, `cloudflare`, `ses`, `oci`. 비우면 `smtp_host`가 있을 때 `smtp`, 없으면 메일을 보내지 않는다. 다른 값은 시작하지 않는다 |
| `mail_api_endpoint` | `FVOCI_MAIL_API_ENDPOINT` | 문자열 | `""` | 공급자의 요청이 공급자의 공식 주소 대신 가는 곳(`https://호스트[:포트]`, 경로·질의·자격 없이). 테스트의 가짜 공급자를 위한 것이고 `http`는 루프백 IP로 쓴 주소(`127.0.0.1`, `[::1]`)만 받는다(이름은 받지 않는다). 공급자로의 요청은 프록시 환경 변수를 쓰지 않는다. 비우면 공급자의 주소(Cloudflare `https://api.cloudflare.com`, SES `https://email.{ses_region}.amazonaws.com`, OCI `https://cell0.submit.email.{oci_region}.oci.oraclecloud.com`: 다른 렐름은 이 키로 준다). SMTP에는 쓰지 않는다 |
| `cloudflare_account_id` | `FVOCI_CLOUDFLARE_ACCOUNT_ID` | 문자열 | `""` | `cloudflare`일 때 필수. Email Service로 보내는 Cloudflare 계정 ID(영문자·숫자) |
| `cloudflare_api_token` | `FVOCI_CLOUDFLARE_API_TOKEN` | 문자열 | `""` | `cloudflare`일 때 필수. 메일 보내기 권한이 있는 API 토큰(`Authorization: Bearer`). 로그에 남기지 않고, 글자열이 아닌 값은 그 값을 되풀이하지 않는 오류로 시작하지 않는다 |
| `ses_region` | `FVOCI_SES_REGION` | 문자열 | `""` | `ses`일 때 필수. SES가 보내는 AWS 지역(`us-east-1` 같은 것, 영소문자·숫자·`-`). 끝점의 호스트와 서명(SigV4, 서비스 `ses`)에 쓴다 |
| `ses_access_key_id` | `FVOCI_SES_ACCESS_KEY_ID` | 문자열 | `""` | `ses`일 때 필수. `ses:SendEmail`을 할 수 있는 IAM 사용자의 장기 접근 키(영문자·숫자). 세션 토큰이 필요한 임시 키(`ASIA`로 시작)는 시작하지 않는다 |
| `ses_secret_access_key` | `FVOCI_SES_SECRET_ACCESS_KEY` | 문자열 | `""` | `ses`일 때 필수. 그 비밀 키. 로그에 남기지 않고, 글자열이 아닌 값은 그 값을 되풀이하지 않는 오류로 시작하지 않는다 |
| `oci_region` | `FVOCI_OCI_REGION` | 문자열 | `""` | `oci`일 때 필수. Email Delivery가 보내는 OCI 지역(`us-ashburn-1` 같은 것, 영소문자·숫자·`-`). 승인된 보내는 사람이 있는 지역이어야 한다 |
| `oci_tenancy` | `FVOCI_OCI_TENANCY` | 문자열 | `""` | `oci`일 때 필수. 테넌시의 OCID(서명의 `keyId` 첫 조각) |
| `oci_user` | `FVOCI_OCI_USER` | 문자열 | `""` | `oci`일 때 필수. API 키를 가진 사용자의 OCID |
| `oci_fingerprint` | `FVOCI_OCI_FINGERPRINT` | 문자열 | `""` | `oci`일 때 필수. 그 API 키의 지문: 콘솔이 보이는 꼴대로 소문자 16진수 두 자리 16쌍을 `:`로 이은 것(`12:34:…`). 다른 꼴은 시작하지 않는다 |
| `oci_private_key_file` | `FVOCI_OCI_PRIVATE_KEY_FILE` | 문자열 | `""` | `oci`일 때 필수. API 키의 개인 키 파일(PEM, PKCS#8 또는 PKCS#1, 암호 없이, RSA 2048–8192비트). 시작할 때 읽고, 읽지 못하거나 쓸 수 없으면 이 키를 말하며(내용은 말하지 않고) 시작하지 않는다 |
| `oci_compartment` | `FVOCI_OCI_COMPARTMENT` | 문자열 | `""` | `oci`일 때 필수. 승인된 보내는 사람(`mail_from`)이 있는 구획의 OCID |
| `smtp_host` | `FVOCI_SMTP_HOST` | 문자열 | `""` | SMTP 호스트 이름이나 IP. `mail_transport`가 비었을 때 이것이 있으면 SMTP로 보낸다. 스킴·포트·공백 없이 쓴다 |
| `smtp_port` | `FVOCI_SMTP_PORT` | 정수(1–65535) | `587` | SMTP 포트 |
| `smtp_tls` | `FVOCI_SMTP_TLS` | 문자열 | `"starttls"` | `starttls`(필수 업그레이드), `tls`(처음부터 TLS), `none`(암호화 없음). TLS는 시스템의 인증서 신뢰를 쓴다 |
| `smtp_user` | `FVOCI_SMTP_USER` | 문자열 | `""` | SMTP 인증 사용자. 비밀번호와 함께 주거나 둘 다 비운다 |
| `smtp_password` | `FVOCI_SMTP_PASSWORD` | 문자열 | `""` | SMTP 인증 비밀번호. 로그에 남기지 않는다 |
| `mail_from` | `FVOCI_MAIL_FROM` | 문자열 | `""` | 모든 메일(어느 길이든)의 보내는 주소 하나. 메일이 켜지면 필수이고 시작할 때 검사한다. 표시 이름과 줄바꿈은 받지 않는다 |
| `mail_per_second` | `FVOCI_MAIL_PER_SECOND` | 정수(1 이상) | `1` | 모든 메일(재설정과 초대, 다시 하는 것까지)이 함께 지키는 초당 보내기 시도 수. 기다리는 재설정 메일이 초대보다 먼저 차례를 얻는다. 기본 1은 새 공급자 계정(샌드박스)의 한도에도 맞는다: 공급자가 허락하는 만큼 올린다 |
| `mail_reset_queue` | `FVOCI_MAIL_RESET_QUEUE` | 정수(1 이상) | `200` | 기다리거나 보내는 중인 비밀번호 재설정 메일의 천장. 차면 새 재설정 메일은 버리고(답은 그대로 204) 경고 로그와 `fvoci_password_reset_mail_dropped_total{reason="queue_full"}`에 센다 |
| `mail_reset_per_account_hour` | `FVOCI_MAIL_RESET_PER_ACCOUNT_HOUR` | 정수(1 이상) | `3` | 한 계정에 지난 한 시간에 줄 세운 재설정 메일의 천장(메모리에서 센다). 넘으면 메일을 보내지 않고(답은 그대로 204) 정보 로그와 `fvoci_password_reset_mail_dropped_total{reason="account_limit"}`에 센다. 계정도 출처도 잠그지 않는다 |
| `mail_invitation_queue` | `FVOCI_MAIL_INVITATION_QUEUE` | 정수(`invitation_mail_recipients`의 두 배 이상) | `200` | 기다리거나 보내는 중인 초대 메일의 천장. 서버에 조직이 하나뿐이면(SQLite, 또는 PostgreSQL에 쓰이는 조직이 하나) 그 조직이 다 쓴다. 조직이 여럿이면 한 조직(같은 조직의 워크스페이스를 모두 합해)은 다른 조직들이 남긴 자리의 절반까지 쥔다(한 요청의 받는 사람 수보다 적지는 않다): 줄에 메일이 있는 조직이 `k`이면 저마다 `k + 1`분의 1쯤에 머문다. 줄이 한 요청의 받는 사람 수(`invitation_mail_recipients`)의 여덟 배 이상이면 바쁜 조직이 셋이어도 넷째가 들어갈 자리가 남는다(기본값 200과 20에서는 넷째와 다섯째까지). 한 요청의 메일이 조직의 몫이나 줄에 다 들어가지 않으면 그 요청은 아무것도 보내지 않고 503(`mail_queue_full`) |
| `invitation_mail_recipients` | `FVOCI_INVITATION_MAIL_RECIPIENTS` | 정수(1 이상) | `20` | 초대 메일 요청 하나의 받는 사람 수(중복 포함)의 천장. 넘으면 400(`too_many_recipients`) |
| `invitation_mail_per_hour` | `FVOCI_INVITATION_MAIL_PER_HOUR` | 정수(`invitation_mail_recipients` 이상) | `100` | 워크스페이스마다 지난 한 시간에 줄 세운 초대 메일의 천장(메모리에서 센다). 이 요청의 메일까지 넘으면 아무것도 보내지 않고 429(`invitation_mail_limit`, `Retry-After`) |
| `invitation_mail_per_org_hour` | `FVOCI_INVITATION_MAIL_PER_ORG_HOUR` | 정수(`invitation_mail_recipients`와 `invitation_mail_per_hour` 이상, 작으면 시작하지 않는다) | `300` | 한 조직(그 워크스페이스 모두)이 지난 한 시간에 줄 세운 초대 메일의 천장(메모리에서 센다). 이 요청의 메일까지 넘으면 아무것도 보내지 않고 429(`invitation_mail_limit`, `Retry-After`). 지금은 모든 조직이 이 값이고, 서버는 조직의 한도(`App::limits`)로 읽는다 |

보내기는 동시에 넷까지이며 초대 메일은 그 가운데 셋까지 쓴다(하나는 늘 재설정의 몫). 조직이 여럿인 서버에서 한 조직의 초대 메일은 둘까지만 쓴다(하나는 늘 다른 조직의 몫). 재설정과 초대는 줄을 따로 서고, 시도마다 `mail_per_second`의 차례를 받는다. 한 번의 시도는 10초까지다(HTTP 연결은 그 가운데 5초까지, 리다이렉트는 따르지 않는다). 보내는 길의 답이 "잠시 뒤 다시"(SMTP 4xx, `454` 포함, 답이 없는 시간 초과, 연결이 도중에 끊김, HTTP `408`·`429`·`5xx`와 HTTP의 연결·시간 초과, SES의 `ThrottlingException`·`TooManyRequestsException`·`LimitExceededException`(초당·하루 한도), OCI의 `400 LimitExceeded`(분당·하루 한도))이면 1초·4초 뒤(길이 기다릴 시간을 말하면 그만큼이고, 15분보다 길면 그 메일은 포기한다) 두 번 더 시도하고, "영구 실패"(SMTP 5xx, HTTP의 그 밖의 4xx, Cloudflare가 2xx로 받고 `permanent_bounces`에 둔 받는 사람)이면 다시 하지 않는다. 인사에서 서버를 거절하는 릴레이(`554`)는 받는 사람의 거절과 가를 수 없어 메일마다의 영구 실패로 센다. "길이 막힘"(SMTP `530`·`534`·`535`·`538`처럼 서버의 자격을 받지 않는 답, 연결·TLS를 맺지 못함, `smtp_tls`가 바라는 STARTTLS나 자격에 맞는 인증 방식이 없는 릴레이, HTTP `401`·`403`과 따르지 않는 `3xx`, OCI와 Cloudflare의 `404`, SES의 `SendingPausedException`·`AccountSuspendedException`)은 메일의 잘못이 아니므로 그 메일의 시도로 세지 않는다: 1분에 한 번만 시도하고 그 시도는 기다리는 재설정 메일이 먼저 쓴다. 초대 메일은 멈추고, 기다리는 재설정 메일이 없을 때만 그 1분의 시도에 쓰인다. 막힌 길을 15분 넘게 기다린 재설정 메일은 포기하고, 초대 메일은 길이 열릴 때까지 자리를 쥐고 기다린다. 길이 막히면 오류 로그 한 줄과 `fvoci_mail_path_down`(막힘 1, 아님 0)이 남고, 메일을 받거나 길의 답으로 한 메일만 거절하면 길이 열린 것으로 보고 정보 로그를 남긴다. 다시 하기를 기다리는 동안에는 아무것도 쥐지 않는다. 차례를 기다리는 동안 재설정 메일은 아무것도 쥐지 않고, 초대 메일은 조직의 몫과 초대의 몫 하나를 쥔다. 시간 초과도 다시 시도하므로, 길이 이미 받은 메일이 드물게 두 번 갈 수 있다. "잠시 뒤 다시"가 세 번 잇따르면(공급자의 하루 한도로 본다) 초대 메일은 1분(길이 말한 시간, 15분까지) 기다리고 재설정 메일만 시도한다. 보낸 메일이 하나 나오면 초대도 다시 나간다. 포기한 메일은 경고 로그(종류, 시도, 응답 코드만)와 `fvoci_mail_failed_total{kind,class}`(`kind`: `password_reset`·`invitation`·`email_verification`, `class`: `permanent`·`temporary`)에 센다. 공급자의 응답 글, 메일의 주소·토큰은 로그에 남기지 않는다. 재설정 토큰은 보낼 차례를 얻은 뒤 만든다. 작업은 메모리에 있으므로 프로세스가 갑자기 멈추면 보내지 못할 수 있다. 메일이 꺼져 있으면 운영자가 위 관리 소켓으로 링크를 발급한다.

## 키: 본문 검색

카드와 위키 페이지의 제목과 본문은 따로 띄우는 검색 서버로 찾는다: Elasticsearch나 OpenSearch(둘 다 같은 요청으로 쓴다)에 한국어 형태소 분석기 nori 플러그인(`analysis-nori`)이 있어야 한다. `search_url`이 비어 있으면 본문 검색은 없다: `getClientConfig`의 `searchEnabled`가 `false`이고 `searchWorkspaceContent`는 404(`search_not_configured`)이며, DB로 대신 찾지 않고 보낼 것 표에도 적지 않는다(검색 서버 없이 뜨는 서버는 시작할 때 표와 색인의 기억을 지우고, 지우지 못하면 시작하지 않는다. 검색 서버에 남은 옛 색인은 서버가 지우지 않는다: 운영자가 지운다). 브라우저는 검색 서버에 닿지 않는다. 서버는 `search_index`라는 색인 하나를 스스로 만든다(한국어는 nori로 읽되 복합어는 통째로도 나눠서도 싣고, 같은 글을 표준 분석기로도 한 번 더 싣는다). 그 이름의 색인이 없으면 만들고(서버가 도는 동안 사라져도 다시 만든다), 이 서버가 더 옛 판의 정의로 만든 것이면 지우고 다시 만든다. fvoci의 표지(`_meta.fvoci`)가 없는 색인이나 더 새 서버가 만든 색인이 그 이름에 있으면 건드리지 않고, nori가 없으면 색인을 만들지 못한다: 이 경우들은 까닭을 로그에 남기고 검색을 끈 채 서버는 뜬다(고친 뒤 서버를 다시 시작한다). 검색 서버가 답하지 않는 것은 서버의 시작을 막지 않는다: 답할 때까지 1초에서 60초까지 간격을 늘려 가며 다시 묻는다. 검색 요청은 검색 서버를 3초까지 기다리고, 답이 없거나 거절되거나 일부만 답하면 503(`search_unavailable`)이다. `/ready`와 다른 조작은 검색 서버를 기다리지 않는다. 검색 서버의 답은 그대로 믿지 않는다: 답에 든 카드와 페이지마다 DB에서 지금 그 사람이 읽는 것인지 다시 확인하고, 제목과 그릇(보드나 위키)은 DB의 것을 답한다. 그래서 모자라면 그 3초 안에서 다음 것을 더 물어 채운다: 답의 개수는 걸러진 것이 있었는지를 말하지 않는다(역할을 가진 보드와 위키가 1,000개를 넘는 사람은 워크스페이스 전체에서 찾아 걸러 내며, 가장 잘 맞는 10,000개까지만 본다). 서버가 멈출 때는 검색 서버로 가던 요청을 1초까지만 기다린다. 바뀐 것은 저장된 뒤에 보낸다: 카드나 페이지의 제목·본문이 바뀌거나, 카드·페이지(그 아래 페이지와 함께)·보드·위키가 휴지통으로 가거나 돌아오면 그 트랜잭션이 DB의 보낼 것 표(`search_outbox`)에 무엇이 바뀌었는지만 적고(내용은 싣지 않는다), 서버의 주기 작업이 1초마다 표를 읽어 그때 DB에 저장된 것을 검색 서버에 넣거나 지운다. 보내기 전에 검색 서버의 색인이 DB가 기억한 그 색인인지 먼저 묻는다(없는 색인에 문서를 넣으면 두 검색 서버 모두 분석기 없는 색인을 말없이 만들기 때문이다). 보낸 뒤에는 그사이 다시 적히지 않은 행만 표에서 지운다. 댓글, 속성 값, 템플릿 보드의 카드는 보내지 않는다. 검색 서버가 요청을 받지 않으면 표는 그대로 두고 1초에서 60초까지 간격을 늘려 다시 해 보며, 돌아오면 밀린 것을 보낸다. 검색 서버가 문서 하나를 거절하면 그 행만 5초에서 10분까지 간격을 늘려 다시 보낸다(로그에 오류 이름을 남긴다). 표는 검색 서버가 있는 서버에서만 적는다. DB는 표를 어느 색인(이름, 검색 서버가 준 식별자, 정의의 판)에 맞춰 적어 왔는지 기억한다: 색인이 없거나 그 색인이 아니면(처음 켰을 때, 정의의 판이 바뀌었을 때, 검색 서버 없이 뜬 적이 있을 때, 다른 검색 서버를 가리킬 때, 서버가 도는 동안 색인이 사라졌을 때) 색인을 다시 만들고 모든 조직의 보드와 위키를 표에 넣어 처음부터 보낸다. 관리 소켓의 `POST /api/v1/admin/search/reindex`도 같은 일을 한다(답은 표에 넣은 보드와 위키의 수 `boards`. 보내기가 끝날 때까지 검색은 보낸 만큼만 찾는다). 보내기는 DB 하나에 서버 하나라는 전제(`server_lock_wait_seconds`)에 기댄다. 지표: `fvoci_search_requests_total`, `fvoci_search_changes_total`, `fvoci_search_outbox`. 검색 서버로의 요청은 리다이렉트를 따르지 않고 프록시 환경 변수를 쓰지 않는다. 비밀 값(`search_password`, `search_api_key`)은 로그·오류·`Debug`에 나오지 않는다.

| 키 | 환경 변수 | 형 | 기본값 | 뜻 |
|---|---|---|---|---|
| `search_url` | `FVOCI_SEARCH_URL` | 문자열 | `""` | 검색 서버의 주소(`https://호스트[:포트]`, 경로·질의·자격 없이). `http`는 루프백 IP로 쓴 주소(`127.0.0.1`, `[::1]`)만 받는다(이름은 받지 않는다). 비우면 본문 검색이 없고, 그때 아래의 자격 키가 있으면 그 키를 말하며 시작하지 않는다 |
| `search_username` | `FVOCI_SEARCH_USERNAME` | 문자열 | `""` | 검색 서버에 HTTP Basic으로 로그인할 사용자(`:`와 제어 문자는 쓰지 못한다). `search_password`와 함께 준다. 두 검색 서버가 모두 받는다 |
| `search_password` | `FVOCI_SEARCH_PASSWORD` | 문자열 | `""` | 그 사용자의 비밀번호(제어 문자는 쓰지 못한다) |
| `search_api_key` | `FVOCI_SEARCH_API_KEY` | 문자열 | `""` | Elasticsearch의 API 키(인코딩된 꼴). `Authorization: ApiKey …`로 보낸다(공백 없는 ASCII). OpenSearch는 이 방식을 받지 않는다. 사용자·비밀번호와 함께 주면 시작하지 않는다 |
| `search_index` | `FVOCI_SEARCH_INDEX` | 문자열 | `"fvoci"` | 이 서버의 색인 이름. 소문자·숫자·`-`·`_`로 100자까지, 글자나 숫자로 시작한다. 서버 하나(DB 하나)에 색인 하나다: 같은 검색 서버를 여러 fvoci 서버가 쓰면 저마다 다른 이름을 준다. 같은 이름을 주면 서로의 색인을 지우고 다시 만들고, DB를 복제한 서버(스테이징)가 원본과 같은 이름을 쓰면 원본의 색인에 쓴다 |
| `search_requests_per_second` | `FVOCI_SEARCH_REQUESTS_PER_SECOND` | 정수 | `5` | 한 사람이 1초에 할 수 있는 검색 수(쉬었다가는 한 번에 그 두 배까지). 넘으면 429와 `Retry-After`. `0`이면 한도가 없다 |

## 키: 조직의 로그인 공급자

조직마다 OIDC 공급자를 하나 둘 수 있다(`PUT /api/v1/org/{orgId}/oidc`, 그 조직의 관리자). 서버가 공급자에게 보내는 요청(discovery, 토큰, 키)은 `https`의 주소에만 가고, 리다이렉트를 따라가지 않고, 프록시를 쓰지 않으며, 응답은 1 MiB와 10초(연결 5초)까지다. 이름을 풀어 나온 주소가 모두 공인 주소일 때만 붙고, 풀어 본 그 주소로 붙는다. 공급자의 client secret은 `FVOCI_SECRET_KEY`(아래 "그 밖의 환경 변수")로 봉인해 저장한다.

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `oidc_allow_private_addresses` | `FVOCI_OIDC_ALLOW_PRIVATE_ADDRESSES` | 참·거짓 | `false` | 참이면 공인 주소가 아닌 곳(루프백, 사설망, 링크 로컬 등)의 공급자에도 붙는다. 같은 망 안의 공급자를 쓰는 직접 설치를 위한 것이다. 거짓이면 그런 주소로 풀리는 공급자는 저장도 로그인도 거절된다. `https`가 아닌 주소는 이것과 관계없이 거절된다 |
| `oidc_former_provider_days` | `FVOCI_OIDC_FORMER_PROVIDER_DAYS` | 정수(0 이상) | `30` | 조직이 공급자를 다른 issuer로 옮긴 뒤 옛 공급자를 '연결용'으로 남겨 두는 날. 그동안 옛 공급자로는 로그인되지 않고, 새 공급자로 처음 들어온 사람이 옛 공급자에 한 번 로그인해 자기 계정을 이어 받는 데만 쓰인다. 조직 관리자가 일찍 끝낼 수 있고, 지나면 지워진다. 0이면 남기지 않는다 |

조직 관리자는 조직 화면의 통합 로그인(SSO) 절에서 공급자와 자동 가입을 설정하고 콜백 주소를 복사한다. 강제는 지금 저장된 그대로의 공급자로 로그인한 세션에서만 켠다. issuer, client ID, client secret, 로그인 허용 가운데 하나라도 바꾸면 그 전에 만든 세션은 증명이 되지 못하므로 다시 로그인해야 하고, 바꾸는 저장과 강제를 한 번에 하지 못한다(저장된 secret을 그대로 다시 보내는 것은 바꾸는 것이 아니다). 강제 중에는 공급자를 끄거나 지우지 못한다. 강제 중에 issuer, client ID, client secret을 바꿔 저장하면 바로 바뀌지 않고 대기 설정이 되며, 그동안 모두 지금 설정으로 로그인한다. 관리자가 화면의 ‘새 설정으로 로그인해 보기’로 대기 설정의 로그인을 자기 계정으로 끝까지 마치면 그때 대기 설정이 지금 설정이 되고 그 세션이 새 설정의 증명이 된다. 다른 계정으로 끝나거나 그사이 관리자가 아니게 되면 아무것도 바뀌지 않고, 계정을 만들지도 않는다. 다른 issuer로 옮기는 대기 설정이면 새 공급자의 관리자 신원이 아직 어느 계정에도 연결돼 있지 않은 것이 보통인데, 그 신원은 로그인해 보기를 시작한 세션이 지금 공급자(지금 revision)로 만든 것이고, 브라우저가 그 세션을 그대로 갖고 돌아왔으며, 돌아온 때가 그 세션을 만든 지 5분 안일 때에만 그 관리자 자신의 계정에 연결되며, 연결과 적용은 한 트랜잭션이고 감사 기록에 남는다. 세션만 가진 사람은 지금 공급자의 로그인을 다시 통과하지 못하므로 아무것도 연결하지 못한다. 세션이 오래됐으면 화면이 ‘지금 공급자로 다시 로그인’ 단추를 먼저 내놓고, 로그인하면 같은 화면으로 돌아온다. 이미 관리자 계정에 연결된 신원이면 세션의 나이와 관계없이 적용되고, 같은 issuer의 대기 설정은 신원을 연결하지 않는다. 실패하면 아무것도 바뀌지 않는다. 대기 설정은 버릴 수 있고 secret은 돌려주지 않는다. 강제하지 않는 동안에는 바로 바뀌며, 로그인에 쓰이는 값이 바뀌면 대기 설정은 버려진다. 강제를 끈 뒤에 남은 대기 설정은 로그인해 보기로 적용하거나 버릴 수 있다. 대기 저장, 적용, 버림은 조직의 감사 기록에 남는다. 공급자 장애로 로그인할 수 없으면 로컬 관리 소켓의 POST /api/v1/admin/orgs/{orgId}/sso/release로 강제를 해제한다. 공급자 설정과 삭제, 강제 전환은 조직의 감사 기록에 남으며 관리 소켓의 해제는 계정 없이 ‘서버 운영자’로 남는다. 서버 관리자 변경이 인스턴스 자체 조직의 역할을 바꾸면 그 조직에만 같은 행위자로 남는다. 관리 소켓으로 비밀번호를 정하면(그리고 비밀번호 재설정 링크를 만들면) 그 사람이 구성원인 조직마다 한 줄씩 ‘서버 운영자’가 이 구성원에게 한 일로 남고, 비밀번호나 링크는 남기지 않는다. 전체 파일 정리처럼 전역인 관리 소켓 명령은 조직의 기록에 넣지 않으며, 관리 소켓 명령은 모두 비밀 값 없이 명령·대상 사용자 ID(해당할 때)·결과·운영자 표시를 서버의 구조화된 로그에 남긴다.

issuer가 바뀌면(바로 바꾸든, 대기 설정을 적용하든) 옛 공급자는 `oidc_former_provider_days` 동안 ‘연결용’으로 남는다. 옛 공급자로는 로그인되지 않는다: 그 issuer로 만든 세션은 옮기는 순간 끝나고, 옛 공급자의 로그인은 세션을 만들지 않는다. 그동안 새 공급자로 로그인했는데 그 신원이 어느 계정에도 연결돼 있지 않은 사람은 계정이 바로 만들어지지 않고, 로그인 화면에서 ‘이미 계정이 있음(예전 로그인으로 잇기)’과 ‘새로 시작’ 가운데 고른다. 서버는 그 로그인을 그 브라우저의 쿠키로만 찾을 수 있게 10분 동안 잡아 두고(쿠키의 비밀 값은 해시로만 저장), 한 번만 쓴다. 잇기는 옛 공급자로 한 번 로그인하는 것이고, 옛 공급자가 확인해 준 신원이 이 조직에 자리가 있는 계정(구성원, 또는 쓰이는 워크스페이스의 손님)에 연결돼 있으면 새 신원을 그 계정에 연결하고 새 공급자로 만든 세션으로 들어온다. 두 신원을 모두 로그인으로 증명하므로 이메일은 보지 않는다. 잡아 둔 것이 없거나 끝났거나 다른 브라우저의 것이거나, 연결용 기간이 끝났거나, 옛 신원의 계정이 없거나 이 조직에 자리가 없거나, 새 신원이 그사이 다른 계정에 연결됐으면 아무것도 바뀌지 않고 모두 같은 답(`oidc_carry_over_refused`)을 받는다. 새로 시작은 첫 로그인의 규칙 그대로다(자동 가입이 꺼져 있으면 초대가 있어야 한다). 조직 관리자는 SSO 절에서 연결용 기간이 끝나는 때를 보고 일찍 끝낼 수 있으며, 기간이 지나면 서버가 지운다. 연결용으로 남김, 일찍 끝냄, 기간이 끝남(행위자 ‘서버’), 관리자가 연결한 신원, 구성원이나 손님이 이은 신원은 issuer와 함께 조직의 감사 기록에 남는다. 연결하거나 이은 신원의 기록에는 그 신원이 누구인지도 남는다: 새 공급자가 준 이메일(확인하지 않은 값), 없으면 subject. 조직의 기록은 조직 관리자만 읽고, 기록은 고쳐지지 않으므로 그 값은 계정이 없어진 뒤에도 남는다. 다시 옮기면 그 전 공급자가 연결용이 되고, 먼저 남아 있던 것은 끝난다.

공급자로 만든 세션은 그 공급자의 조직과 issuer, 그리고 공급자가 준 subject를 적어 둔다. 공급자를 지우거나 다른 issuer로 바꾸면 그 조직에서 그 issuer로 만든 세션이 모두 끝나고, 계정에서 신원을 끊으면 그 신원(issuer와 subject)으로 만든 그 사람의 세션이 끝난다(끊는 요청을 보낸 세션도 그렇게 만든 것이면 함께). 같은 issuer의 다른 신원으로 만든 세션은 남는다. client ID만 바꾸면 사람의 신원이 바뀌지 않으므로 세션을 끝내지 않는다. 끝난 세션의 WebSocket 연결은 곧바로 닫히고 카드 본문의 방에서도 나간다. 마이그레이션 0028은 공급자가 남은 옛 세션에 issuer를 채우고, 공급자가 없어진 옛 OIDC 세션은 끝낸다. 0029 전에 만든 세션은 subject를 모르므로 그 issuer의 어느 신원을 끊어도 끝난다.

## 키: 예약된 키

**아직 아무 일도 하지 않는다.** 뒤에 쓰기로 정해 둔 키다(사용자 결정, 2026-11-11 05:04 KST). 그때까지는 기본값이 아닌 값을 주면 서버가 시작하지 않는다. 쓰지 않는 기능을 켰다고 믿고 돌지 않게 하려는 것이다.

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `authmode` | `FVOCI_AUTHMODE` | 문자열 | `"native"` | 로그인 방식. `native`(이 서버의 사용자 이름과 비밀번호)만 받는다. OIDC 로그인은 이 키가 아니라 조직마다 두는 공급자다(아래 "키: 조직의 로그인 공급자") |

## 없앤 키

원본 서버의 키, 그리고 이 서버가 다른 이름으로 읽는 키다. 파일이 이 키에 뜻이 있는 값(빈 문자열, 0, `false`, `null`, 빈 목록이 아닌 것)을 주거나, 환경 변수(`FVOCI_` + 대문자)가 공백만이 아니면(`false`나 `0`이어도) 그 키나 변수를 말하며 **서버가 시작하지 않는다**. 말없이 무시하면 운영자가 적은 것과 다르게 돌기 때문이다.

| 키 | 대신 |
|---|---|
| `dbtableprefix` | 테이블 이름은 고정이다. 한 PostgreSQL DB에서 다른 테이블과 떼어 두려면 `dbconfig`의 `search_path`로 이 서버만의 스키마를 준다 |
| `trust_forwarded_for` | 역방향 프록시를 `trusted_proxies`에 적는다. `X-Forwarded-For`는 그 프록시에서 온 것만 믿는다 |
| `securecookie` | 세션 쿠키는 `serverRoot`가 https일 때 `Secure`다 |
| `session_expire_time` | 세션은 `session_idle_days` 동안 쓰지 않으면, 그리고 로그인한 뒤 `session_max_days`가 지나면 끝난다. 둘 다 날 단위다 |
| `session_refresh_time` | 세션을 마지막으로 쓴 때는 많아야 10분마다 적는다. 정할 것이 없다 |
| `logging_cfg_file` | 로그는 `FVOCI_LOG_LEVEL`, `FVOCI_LOG_FORMAT`으로 정한다 |
| `logging_cfg_json` | 위와 같다 |
| `audit_cfg_file` | 감사 로그는 없다 |
| `audit_cfg_json` | 위와 같다 |
| `filess3config.trace` | S3 요청마다 로그를 남기지 않는다. 환경 변수는 `FVOCI_FILESS3CONFIG_TRACE`다 |
| `smtp_from` | 모든 길의 보내는 주소는 `mail_from`(`FVOCI_MAIL_FROM`)이다 |

## 명령줄 인자

| 인자 | 기본값 | 뜻 |
|---|---|---|
| `--config <경로>` | `./config.json` | 설정 파일 |
| `--port <포트>` | — | `port`를 바꾼다. 0이면 무시한다 |
| `--dbtype <종류>` | — | `dbtype`을 바꾼다. 비면 무시한다 |
| `--dbconfig <문자열>` | — | `dbconfig`를 바꾼다. 비면 무시한다. 로그에 값을 남기지 않는다 |
| `--print-openapi` | — | 계약(OpenAPI 문서)을 표준 출력에 쓰고 끝난다. 설정을 읽지 않는다 |
| `--help`, `--version` | — | 도움말과 판 |

## 그 밖의 환경 변수

| 변수 | 뜻 |
|---|---|
| `FVOCI_HTTP_SERVER_BASEPATH` | 웹앱과 API를 놓을 경로. 비거나 `/boards` 같은 절대 경로여야 한다 |
| `FVOCI_LOG_LEVEL` | 로그의 수준과 거르개(`tracing`의 `EnvFilter` 문법, 예: `info`, `fvoci_server=debug`). 기본은 `info` |
| `FVOCI_LOG_FORMAT` | `json`이면 로그를 줄마다 JSON으로 쓴다. 그 밖에는 사람이 읽는 꼴이다 |
| `FVOCI_SECRET_KEY` | 서버가 DB에 봉인해 두는 값(조직 공급자의 client secret)의 키. 32바이트를 표준 base64로 쓴 것이다(`openssl rand -base64 32`). AES-256-GCM의 키로 쓴다. 환경 변수로만 받는다: 설정 파일에 `secret_key`가 있으면 시작하지 않는다. 꼴이 틀리면 값을 되풀이하지 않는 오류로 시작하지 않는다. 로그와 `Debug` 출력에 나오지 않는다. 없으면 공급자를 저장할 수 없다(409, `secret-key-required`). 바꾸면 저장된 secret이 열리지 않으므로 공급자마다 secret을 다시 저장한다 |
| `AWS_*` | `filess3config.accesskeyid`가 비었을 때 S3 클라이언트가 읽는 자격과 설정(위 "S3의 예") |

다른 접두어(`FOCALBOARD_` 등)를 단 이 서버의 설정(`…_SERVERROOT`, `…_DBCONFIG` 등)이 환경에 있으면, 무시된다고 시작할 때 경고한다.

## 예시 파일

`docker/fvoci/config.json`이 컨테이너 이미지에 들어가는 설정이다. 테스트 `the_example_configuration_files_are_read`가 이 파일을 읽고 검사해 오류가 없는지 본다.

# 서버의 설정

적은 때: 2026-11-11 05:05 KST, 고친 때: 15:56 KST. 서버(`fvoci-server`)가 읽는 설정의 전부다. 코드는 `crates/server/src/config.rs`다.

이 문서는 코드와 함께 검사된다. `config.rs`의 테스트 `the_configuration_document_lists_every_key`가 다음을 견준다.

- 코드가 받는 키와 아래 "키" 표들에 적힌 키가 같은지.
- 키마다 기본값이 같은지.
- 환경 변수로 바꿀 수 있는 키가 같은지.
- "없앤 키" 표가 코드의 목록과 같은지.

키를 더하거나 바꾸면 같은 커밋에서 이 문서를 고친다.

## 읽는 법

설정은 세 곳에서 온다. 뒤의 것이 앞의 것을 이긴다.

1. **설정 파일.** JSON 객체 하나다. 자리는 명령줄의 `--config`로 주며 기본은 `./config.json`이다.
   - 키는 대소문자를 가리지 않는다(`serverRoot`와 `serverroot`는 같다). `filess3config` 안의 키도 같다.
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
| `websocket_ping_seconds` | `FVOCI_WEBSOCKET_PING_SECONDS` | 정수(0 이상) | `30` | WebSocket마다 Ping을 보내는 간격(초). 0이면 보내지 않는다. 한가한 연결을 프록시가 끊지 않게 한다. Ping마다 연결의 세션도 다시 보고 끝났으면 닫는다(0이면 1분마다) |
| `compress_responses` | `FVOCI_COMPRESS_RESPONSES` | 참·거짓 | `true` | 참이면 API의 JSON 응답을 클라이언트의 `Accept-Encoding`에 따라 zstd, brotli, gzip으로 압축한다(32바이트 미만과 오류 응답은 그대로). 압축하는 프록시나 CDN 뒤라면 거짓으로 둔다. 내보내는 아카이브와 파일은 이것과 관계없이 압축하지 않는다(길이를 그대로 보낸다). 웹앱의 정적 파일은 빌드가 만든 `.zst`, `.br`, `.gz`를 고르므로 이것과 관계없다 |

## 키: 데이터베이스

한 데이터베이스는 서버 하나가 쓴다. 서버 여럿이 한 데이터베이스를 나눠 쓰는 배치는 지원하지 않는다: WebSocket의 알림, 로그인 제한, 열린 업로드의 한도가 서버의 메모리에서 셈을 하고, 끝나지 않은 업로드의 부분은 그 서버의 `tmp_path`에 있다.

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `dbtype` | `FVOCI_DBTYPE` | 문자열 | `"sqlite3"` | `sqlite3` 또는 `postgres` |
| `dbconfig` | `FVOCI_DBCONFIG` | 문자열 | `"./fvoci.db"` | 연결 문자열. SQLite는 파일의 경로, PostgreSQL은 `postgres://사용자:비밀번호@호스트:포트/DB?sslmode=disable` 꼴이다. 다른 테이블과 한 DB를 나눠 쓰려면 `search_path`로 이 서버만의 스키마를 준다. 비밀번호가 들어 있을 수 있어 로그에 남기지 않는다 |
| `dbpingattempts` | `FVOCI_DBPINGATTEMPTS` | 정수(0 이상) | `5` | 시작할 때 DB에 닿으려고 해 보는 횟수. 0이어도 한 번은 해 본다 |

## 키: 파일 저장소

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `filesdriver` | `FVOCI_FILESDRIVER` | 문자열 | `"local"` | `local`(디스크) 또는 `amazons3`(S3와 S3 호환 저장소). 그 밖의 값이면 시작하지 않는다 |
| `filespath` | `FVOCI_FILESPATH` | 문자열 | `"./files"` | `local`일 때 파일을 두는 디렉터리. 파일은 그 아래 `.tmp/writing`에 다 쓴 뒤 제자리로 옮긴다(같은 파일시스템이라 원자적이다. `tmp_path`와 상관없다) |
| `maxfilesize` | `FVOCI_MAXFILESIZE` | 정수(1 이상, 바이트) | `104857600` | 올릴 수 있는 파일의 가장 큰 크기(100 MiB). 0이나 음수는 시작 오류다(예전에는 0이 "한도 없음"이었다). `upload_part_bytes` × 10,000(S3 멀티파트의 부분 수 한도)을 넘어도 시작 오류다. 요청 하나로 올리는 파일(`uploadFile`)은 그에 더해 `upload_part_bytes`까지다. 사용자 한 명이 열린 업로드에 걸어 둘 수 있는 크기는 이 한도와 `max_import_bytes` 가운데 큰 것의 두 배다 |
| `filess3config.accesskeyid` | — | 문자열 | `""` | 액세스 키. **비워 두면 S3 클라이언트가 환경에서 자격을 읽는다**(`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN` 등. 아래 "S3의 예") |
| `filess3config.secretaccesskey` | — | 문자열 | `""` | 비밀 키. `accesskeyid`가 있을 때만 쓴다. 로그에 남기지 않는다 |
| `filess3config.bucket` | `FVOCI_FILESS3CONFIG_BUCKET` | 문자열 | `""` | 버킷. `amazons3`이면 비어 있을 수 없다 |
| `filess3config.pathprefix` | `FVOCI_FILESS3CONFIG_PATHPREFIX` | 문자열 | `""` | 모든 객체 키 앞에 붙는 접두어. 앞뒤의 `/`는 떼고 쓴다 |
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
| `login_address_attempts_per_minute` | `FVOCI_LOGIN_ADDRESS_ATTEMPTS_PER_MINUTE` | 정수(0 이상) | `20` | 한 주소가 1분에 해 볼 수 있는 로그인과 가입. 0이면 주소로 세지 않는다(모든 사용자가 한 주소에서 오고 프록시도 없는 서버) |
| `password_hash_concurrency` | `FVOCI_PASSWORD_HASH_CONCURRENCY` | 정수(1 이상) | `4` | 한꺼번에 하는 비밀번호 해시와 확인. 하나에 19 MiB와 몇 밀리초의 처리기가 든다 |
| `password_hash_queue` | `FVOCI_PASSWORD_HASH_QUEUE` | 정수(0 이상) | `16` | 해시할 차례를 기다릴 수 있는 요청의 수 |
| `password_hash_queue_seconds` | `FVOCI_PASSWORD_HASH_QUEUE_SECONDS` | 정수(1 이상) | `5` | 차례를 기다리는 시간(초). 넘으면 다시 해 보라고 답한다(429) |

## 키: 프록시

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `trusted_proxies` | `FVOCI_TRUSTED_PROXIES` | 문자열 목록 | `[]` | 서버 앞의 역방향 프록시의 주소나 주소 범위(`10.0.0.0/8` 꼴). 이 주소에서 온 요청만 `X-Forwarded-For`가 말하는 주소에서 온 것으로 본다. 비우면 클라이언트가 서버에 바로 닿는다고 본다. 읽을 수 없는 값이 있으면 시작하지 않는다. `serverRoot`가 https이고 `useSSL`이 거짓인데 이 목록이 비면 시작할 때 경고한다(모든 클라이언트가 프록시 하나로 보여 로그인 제한이 모두에게 한꺼번에 걸린다) |

## 키: 한도와 휴지통, 임시 파일

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `max_request_bytes` | `FVOCI_MAX_REQUEST_BYTES` | 정수(1 이상, 바이트) | `4194304` | JSON을 받는 API 요청 본문의 가장 큰 크기(4 MiB). 넘으면 413. 파일 올리기(`uploadFile`과 `putUploadPart`)에는 쓰지 않는다. 아카이브 가져오기(`importArchive`)는 업로드를 이름으로만 받는 JSON이라 이것을 따른다. 세션 없는 요청(64 KiB)과 WebSocket 메시지(64 KiB)의 한도는 설정이 아니다 |
| `trash_retention_days` | `FVOCI_TRASH_RETENTION_DAYS` | 정수(1 이상) | `30` | 지운 보드와 블록이 휴지통에 남아 되살릴 수 있는 날. 지나면 영구히 지운다. 어느 블록도(휴지통의 것까지) 가리키지 않는 파일도 그렇게 된 지 이만큼 지나면 지운다 |
| `upload_part_bytes` | `FVOCI_UPLOAD_PART_BYTES` | 정수(5242880–5368709120, 바이트) | `8388608` | 청크로 올릴 때(`createUpload`) 마지막을 뺀 부분 하나의 크기(8 MiB). S3 멀티파트가 받는 부분이 5 MiB에서 5 GiB라서 그 밖이면 시작하지 않는다. 서버 앞의 프록시가 받는 요청 본문보다 작아야 한다(Cloudflare의 Free와 Pro는 100 MB). `filesdriver`가 `amazons3`이어도 부분은 `tmp_path`의 파일로 받아 확인한다. 버킷에 보내는 동안만 메모리에 있고, 그 메모리는 이 크기의 8배까지다(기본 64 MiB). 넘으면 다음 부분은 디스크에서 기다린다 |
| `max_pending_upload_bytes` | `FVOCI_MAX_PENDING_UPLOAD_BYTES` | 정수(`maxfilesize`와 `max_import_bytes` 이상, 바이트) | `2147483648` | 서버의 열린 업로드(`createUpload`, `createImportUpload`) 모두가 걸어 둘 수 있는 크기의 합(2 GiB). 넘는 업로드를 열면 429이고 `reason`이 `server_uploads_pending`이다. `maxfilesize`보다 작으면 가장 큰 파일을 언제나 429로 거절하게 되므로 시작 오류다 |
| `max_import_bytes` | `FVOCI_MAX_IMPORT_BYTES` | 정수(1 이상, 바이트) | `268435456` | 가져올 수 있는 아카이브의 가장 큰 크기(256 MiB). 아카이브도 청크로 올리므로 `upload_part_bytes` × 10,000을 넘으면 시작 오류이고, `max_pending_upload_bytes`보다 커도 시작 오류다. 풀린 합계는 1 GiB까지다(고정) |
| `tmp_path` | `FVOCI_TMP_PATH` | 문자열 | `""` | 서버의 임시 파일을 두는 디렉터리. 청크로 올리는 부분은 `filesdriver`가 `amazons3`이어도 여기에 받는다(S3로 보낸 뒤 지운다). 업로드에 필요한 자리는 열린 업로드가 걸어 둔 크기의 합까지다(`max_pending_upload_bytes`). 가져오는 중인 아카이브는 업로드가 끝난 것으로 세어 이 합에 들지 않고, 가져오는 동안 아카이브 하나와 부분 하나, 풀어 낸 파일 항목 하나(`maxfilesize`까지)의 자리를 차지한다. 동시에 가져오는 수에는 한도가 없다. 내보내는 아카이브는 보내는 동안 자리를 차지하고(디렉터리에서는 보이지 않는다), 그 크기와 수에는 한도가 없다. 끝나지 않은 업로드의 부분(`uploads/<uploadId>/`), 가져오는 중인 아카이브(`uploads/<uploadId>.importing-…/`)와 내보내는 중인 아카이브(`exports/`. 다 쓰면 열어 둔 채 디렉터리에서 지우고 응답으로 흘린다)를 둔다. 그 아래 `uploads`와 `exports`는 서버의 것이다: 서버가 시작할 때 어느 업로드의 것도 아닌 것과 내보내다 남은 것을 지운다. 한 디렉터리를 두 서버가 함께 쓰지 않는다(`filespath`도 그렇다. 시작할 때 `filespath/.tmp/writing`을 비운다). 비우면 `filespath` 아래 `.tmp`다. 시스템의 임시 디렉터리는 메모리에 있을 수 있어 쓰지 않는다 |

## 키: 공유와 보이는 것

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `enablePublicSharedBoards` | `FVOCI_ENABLEPUBLICSHAREDBOARDS` | 참·거짓 | `false` | 참이면 보드를 읽기 토큰으로 공개 공유할 수 있다. 거짓이면 어느 읽기 토큰도 맞지 않는다 |
| `featureflags` | — | 문자열에서 문자열로의 객체 | `{}` | 웹앱에 넘기는 기능 표시(`getClientConfig`의 `featureFlags`) |
| `teammatenamedisplay` | `FVOCI_TEAMMATENAMEDISPLAY` | 문자열 | `"username"` | 웹앱이 다른 사용자의 이름을 보이는 방식(`getClientConfig`의 `teammateNameDisplay`) |
| `showemailaddress` | `FVOCI_SHOWEMAILADDRESS` | 참·거짓 | `false` | 사용자 목록에 이메일을 싣는지 |
| `showfullname` | `FVOCI_SHOWFULLNAME` | 참·거짓 | `false` | 사용자 목록에 이름과 성을 싣는지 |

## 키: 관리자 소켓과 웹훅

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `enableLocalMode` | `FVOCI_ENABLELOCALMODE` | 참·거짓 | `false` | 참이면 관리 API를 유닉스 소켓으로 연다(비밀번호 재설정, 관리자 지정 등) |
| `localModeSocketLocation` | `FVOCI_LOCALMODESOCKETLOCATION` | 문자열 | `"/var/tmp/fvoci_local.socket"` | 그 소켓의 경로 |
| `webhook_update` | `FVOCI_WEBHOOK_UPDATE` | 문자열 목록 | `[]` | 바뀐 블록마다 그 블록을 `POST`로 받는 주소들 |

## 키: 지표

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `prometheusaddress` | `FVOCI_PROMETHEUSADDRESS` | 문자열(`호스트:포트`) | `""` | Prometheus 지표를 내는 주소. 비어 있으면 지표를 모으지 않고 아무것도 듣지 않는다. 주소가 있으면 API와 **다른 리스너**를 그 주소에 열고 `GET /metrics`만 Prometheus의 글 형식으로 답한다(그 밖의 길은 404). API의 리스너에는 지표의 길이 없다. 호스트는 이름, IPv4, 괄호 안의 IPv6(`[::1]:9092`)이고, 비우면(`:9092`) 모든 인터페이스다(`localOnly`여도 그렇고, 그 둘을 함께 주면 시작할 때 경고한다). 포트 0은 빈 포트 아무것이고 고른 주소를 로그에 남긴다. 꼴이 틀리면, 그 주소에서 들을 수 없으면(이미 쓰임, 이 기기의 주소가 아님) 시작하지 않는다. **인증이 없고** `usessl`과 관계없이 평문 HTTP다. 모니터링만 닿는 안쪽 주소(루프백, 사설망)에 묶는다. 요청 로그에 남기지 않는다 |

## 키: 예약된 키

**아직 아무 일도 하지 않는다.** 뒤에 쓰기로 정해 둔 키다(사용자 결정, 2026-11-11 05:04 KST). 그때까지는 기본값이 아닌 값을 주면 서버가 시작하지 않는다. 쓰지 않는 기능을 켰다고 믿고 돌지 않게 하려는 것이다.

| 키 | 환경 변수 | 타입 | 기본값 | 뜻과 범위 |
|---|---|---|---|---|
| `authmode` | `FVOCI_AUTHMODE` | 문자열 | `"native"` | 로그인 방식. 뒤에 OIDC 로그인을 넣을 자리다. 지금은 `native`(이 서버의 사용자 이름과 비밀번호)만 받는다 |

## 없앤 키

원본 서버에 있었고 이 서버에는 없는 키다. 파일이 이 키에 뜻이 있는 값(빈 문자열, 0, `false`, `null`, 빈 목록이 아닌 것)을 주거나, 환경 변수(`FVOCI_` + 대문자)가 공백만이 아니면(`false`나 `0`이어도) **서버가 시작하지 않는다**. 말없이 무시하면 운영자가 적은 것과 다르게 돌기 때문이다.

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
| `AWS_*` | `filess3config.accesskeyid`가 비었을 때 S3 클라이언트가 읽는 자격과 설정(위 "S3의 예") |

다른 접두어(`FOCALBOARD_` 등)를 단 이 서버의 설정(`…_SERVERROOT`, `…_DBCONFIG` 등)이 환경에 있으면, 무시된다고 시작할 때 경고한다.

## 예시 파일

`docker/fvoci/config.json`이 컨테이너 이미지에 들어가는 설정이다. 테스트 `the_example_configuration_files_are_read`가 이 파일을 읽고 검사해 오류가 없는지 본다.

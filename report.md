# 30. Oracle Cloud(OCI)에서 직접 운영하기

- 바탕: `master` `2edbe3c1`. 코드는 읽기만 했다.
- 바깥 자료는 모두 **2026-11-25**에 확인했다. 값은 그날의 USD 정가(종량제)이고 한 달은 730시간으로 셈했다. 가격표는 oracle.com 가격 페이지를 브라우저로 그려 읽었다(표가 스크립트로 채워진다).
- 표기: **[확인]** 문서나 코드에서 본 것, **[짐작]** 확인하지 못한 추론, **[못 봄]** 찾았지만 공개 자료로 닿지 못한 것.

## 0. 한 줄 결론

OCI 서울 리전에서 **유료(종량제) 계정 + Ampere 기계 하나 + 지금의 compose(PostgreSQL 18·OpenSearch를 기계 안에) + 파일은 서울 Object Storage + 앞단은 지금처럼 Cloudflare 터널**로 시작하는 것이 가장 싸고 덜 바뀐다. 장면 A는 월 약 $35, B는 기계를 셋으로 나눠 월 약 $210로 본다. 관리형 PostgreSQL은 이제 18이 있지만 OCPU당 값이 커서(A 기준 월 $145 이상) 지금 단계엔 이르다. 다만 **S3 호환 API가 fvoci가 쓰는 ListObjectsV2를 문서에 적지 않았고**, **메일 엔드포인트를 코드가 고정 주소로 짓는다**는 두 가지는 첫 배치 전에 실측해야 한다.

## 1. 계산

| 항목 | 값 | 출처 |
|---|---|---|
| Ampere A1 | OCPU $0.01/시간, 메모리 $0.0015/GB·시간 (모든 리전 같은 값이라고 적힘) | [oracle.com/cloud/price-list](https://www.oracle.com/cloud/price-list/), [oracle.com/kr/cloud/compute/arm](https://www.oracle.com/kr/cloud/compute/arm/) |
| Ampere A2 / A4 | A2 OCPU $0.014·메모리 $0.002, A4 OCPU $0.0138·메모리 $0.0027 | 같은 가격표 |
| AMD E5/E6 (비교) | OCPU $0.03, 메모리 $0.002 | 같은 가격표 |
| 블록 볼륨 | $0.0255/GB·월 + 성능 단위(균형 10 VPU) $0.017 → 합 $0.0425/GB·월 | 같은 가격표 |

- **A1 4코어·24 GB 한 대 = 월 $55.5** (29.2 + 26.3). 무료 몫을 빼기 전이다.
- **서울·춘천 재고** [못 봄]: Oracle은 리전별 재고를 공개하지 않는다. 서드파티(Northflank) 목록에는 서울에 A1·A2·A4가 있고 "계정과 리전에 따라 다르다"고 적었다([northflank.com/cloud/oci/regions/ap-seoul-1](https://northflank.com/cloud/oci/regions/ap-seoul-1)). 춘천은 확인하지 못했다. 두 리전 모두 가용 도메인이 하나다([regions.htm](https://docs.oracle.com/en-us/iaas/Content/General/Concepts/regions.htm), 2026-06-22 갱신). 인기 리전에서 A1 "Out of capacity"가 잦다는 말은 커뮤니티에 많지만 서울 자료는 찾지 못했다.

### Always Free 한도 [확인, [resourceref.htm](https://docs.oracle.com/en-us/iaas/Content/FreeTier/resourceref.htm) 2026-06-12 갱신]

| 항목 | 무료 몫 |
|---|---|
| A1 | **월 1,500 OCPU·시간과 9,000 GB·시간 = 2코어·12 GB** ("All tenancies") |
| AMD Micro | E2.1.Micro 두 대(각 1/8 OCPU, 1 GB) |
| 블록 | 부트+블록 합 200 GB, 볼륨 백업 5개 (홈 리전만) |
| Object Storage | 20 GB(무료 전용 계정) / 유료 계정은 Standard 10 GB |
| 외부 전송 | 월 10 TB (가격표: APAC 출발도 첫 10 TB 무료, 넘으면 $0.025/GB) |
| 부하분산기 | Flexible LB 1개(10 Mbps 고정), Network LB 1개(가격표에 "Free") |
| 메일 | 월 3,000통 |
| 모니터링·로그 | 수집 5억·조회 10억 데이터포인트, 로그 저장 월 10 GB |
| Bastion | 비용 없음 |

- **줄어든 한도**: 예전 몫은 4코어·24 GB(3,000/18,000)였고 2026-06 중순에 절반이 됐다는 보도가 있다([InfoQ 2026-07-03](https://www.infoq.com/news/2026/07/oracle-cloud-free-tier-limits/), [linuxiac 2026-06-14](https://linuxiac.com/oracle-quietly-cuts-free-tier-ampere-a1-resources-in-half/)). **그런데 가격표(2026-08-25 갱신) 각주는 아직 "첫 3K OCPU-hrs와 18K GB-hrs 무료"라고 적는다.** 두 공식 자료가 어긋난다. InfoQ는 사용자 메일을 들어 종량제 계정은 그대로일 수 있다고 썼지만 Oracle의 공식 설명은 없다. 아래 비용 표는 문서 쪽(2코어·12 GB)으로 셈했다.
- 지금 테스트 서버(4코어·24 GB)는 예전 무료 몫 그대로다. 새 한도 아래라면 이 기계가 이미 과금되고 있거나(종량제) 무료 전용 계정이면 같은 꼴로 새로 만들 수 없을 수 있다 [짐작].
- Always Free 계산과 블록은 **홈 리전에서만** 만든다(freetier.htm). 홈 리전은 가입 때 고르며 나중에 바꿀 수 없다고 알려져 있다 [짐작: 이번에 그 문장을 문서에서 보지는 못했다].

### 되찾아 가는 규칙 [확인, resourceref.htm]

- 7일 동안 **CPU 95번째 백분위 < 20%, 네트워크 < 20%, (A1만) 메모리 < 20%가 모두 참이면** 유휴로 보고 되찾을 수 있다.
- 무료 계정이 30일 이상 놀면 버려진 것으로 볼 수 있다([Free Tier FAQ](https://www.oracle.com/cloud/free/faq/)).
- 유료로 올리면 이 규칙에서 빠지는지는 **문서에 없다**. 문서는 "올린 뒤에도 Always Free 자원은 과금하지 않는다"까지만 말한다.
- 피하는 길 [짐작]: (1) 종량제로 올린다. 커뮤니티의 공통된 경험담이지만 공식 문구가 없다. (2) fvoci는 PostgreSQL과 OpenSearch(힙 512 MB, `docker/compose.yaml:304`)를 함께 띄우므로 12 GB 기계에서 메모리 20%(2.4 GB)를 넘기 쉽다. 세 조건이 **모두** 참이어야 하니 메모리만으로도 유휴가 아니게 된다. 다만 측정 방식(캐시 포함 여부)은 문서에 없다. CPU를 일부러 태우는 스크립트는 약관에 맞지 않을 수 있어 권하지 않는다.

### 운영에 무료 등급을 써도 되나

- [확인] FAQ: Free Tier에는 **SLA와 기술 지원이 없고**, Always Free만 쓰는 고객은 Oracle Support 대상이 아니다. 문서는 무료 자원을 "작은 앱이나 개념 증명"용으로 적는다. 상업적 사용을 금하는 문구는 읽은 페이지에 없었다(약관 본문 Cloud Services Agreement는 읽지 않았다).
- 위험 [짐작]: 한도를 예고 없이 줄인 전례(2026-06), 유휴 회수, 계정 정지 시 지원 창구 없음, 재고 부족 시 다시 만들 수 없음. **유료 사용자의 데이터를 올리는 운영이라면 종량제 계정이 맞다.** 종량제로 올려도 무료 몫은 그대로 깎아 준다.

## 2. PostgreSQL

### 관리형 OCI Database with PostgreSQL

| 질문 | 답 | 출처 |
|---|---|---|
| 18이 있나 | **있다.** 2026-10-15에 18 선택 가능 공지 | [릴리스 노트](https://docs.oracle.com/iaas/releasenotes/services/postgresql/index.htm) (2026-11-16 갱신) |
| 판 올리기 | 제자리 올리기 지원(2026-10-20). 큰 DB는 pg_dump/pglogical 권장 | [upgrades.htm](https://docs.oracle.com/en-us/iaas/Content/postgresql/upgrades.htm) (2026-11-23) |
| 관리자 | 슈퍼유저가 아니다. `CREATEROLE`로 역할을 만들 수 있다 | upgrades.htm |
| 백업·시점 복구 | 자동 백업 최대 35일, 시점 복구(새 DB 시스템으로, 2026-07-06), 다른 리전 백업 복사, 리전 간 대기 복제(2026-06-16) | 릴리스 노트 |
| 값 | **OCPU $0.098/시간 + 저장 $0.072/GB·월** (메모리 별도 표기 없음). 백업은 Object Storage 값 | [가격 페이지](https://www.oracle.com/cloud/postgresql/pricing/) |
| 서울 리전 | [못 봄] 읽은 페이지에 리전 목록이 없었다 | — |
| ARM 모양 | [못 봄] 공지된 모양은 x86(`Standard3.Flex`, 최소 2 OCPU·32 GB 예시)뿐 | 릴리스 노트 2024-02-26 |

fvoci 조건과 맞춰 보기:
- **세션 연결** [짐작]: 문서에 풀러(PgBouncer) 언급이 없다. 앞에 풀러가 없으면 클라이언트가 바로 붙으므로 세션 advisory lock(`docs/configuration.md:47`)은 문제없다.
- **비슈퍼유저 서버 계정과 RLS** [짐작]: fvoci는 표의 주인(마이그레이션)과 주인 아닌 서버 역할 둘을 쓰고, 서버 역할이 `BYPASSRLS`면 `open` 모드에서 시작을 거부한다(`docs/configuration.md:47,55`). 관리자에게 `CREATEROLE`이 있으니 두 역할을 만들 수 있고 RLS는 PostgreSQL 기본 기능이라 막을 까닭이 없다. 다만 "RLS 지원"이라는 문장은 문서에서 찾지 못했다. 관리자 역할이 `BYPASSRLS`를 가졌는지도 모른다. 그 역할은 마이그레이션용으로만 쓴다.
- 최소 비용: 2 OCPU만으로 월 $143 + 저장. 한 대 기계 값의 세 배 가까이다.

### 기계 안에서 직접 운영할 때 필요한 것

지금 있는 것 [확인]: `docker/backup.sh`가 `pg_dump -Fc`를 **같은 기계의 폴더**에 뜨고(`docker/backup.sh:54`, `docker/compose.yaml:104-109`), 파일이 S3에 있으면 버킷은 운영자 몫이라고 말한다(`docker/backup.sh:74`). `restore.sh`는 빈 볼륨에만 되살린다.

OCI에서 더할 것 [짐작, 설계 제안]:
1. **기계 밖 사본**: 덤프 폴더를 서울 Object Storage 버킷으로 올린다(S3 호환 API로 `aws s3 cp`/rclone). 같은 리전 안 전송은 외부 전송이 아니다.
2. **WAL 보관과 시점 복구**: pgBackRest나 WAL-G를 S3 저장소(경로 방식, 리전 `ap-seoul-1`)로 붙인다. 하루 전체 백업 + 연속 WAL이면 RPO가 분 단위가 된다.
3. **블록 볼륨 백업**: 백업 정책(일·주)으로 PG 데이터 볼륨을 뜨고, `CopyVolumeBackup`으로 춘천에 복사한다([cross-region copy](https://docs.oracle.com/en-us/iaas/Content/Block/Tasks/copyingvolumebackupcrossregion.htm) 2026-07-31). 단일 볼륨의 충돌 일관 사본이라 PG는 WAL로 회복한다. 무료 몫은 백업 5개.
4. **복구 훈련**: 달마다 새 기계에 `restore.sh`(또는 pgBackRest restore)로 되살리고 `search/reindex`, `files/reconcile`까지 돌려 본다(`docs/configuration.md:57`).
5. **판 올리기**: 부판은 `postgres:18` 이미지 갱신(`docker/compose.yaml:233`). 주판(19)은 `update.sh`가 다루지 않으므로 덤프→새 볼륨→복원으로 한다. 내리는 마이그레이션은 없다(`docker/compose.yaml:127-130`).

## 3. 파일: OCI Object Storage

### fvoci가 쓰는 S3 동작 [확인]

클라이언트는 `object_store` 0.14.2의 AmazonS3다(`crates/server/src/storage/s3.rs:9`, `Cargo.lock`). 사용자 지정 엔드포인트면 **경로 방식**으로 부른다(`s3.rs:65-71`).

| fvoci 동작 | S3 요청 | OCI 문서 |
|---|---|---|
| 쓰기 `write` (`s3.rs:129`, 큰 것은 BufWriter가 여러 부분으로) | PutObject / 멀티파트 | 지원 |
| 읽기 범위 `read` (`s3.rs:181`) | GetObject + Range | GetObject 지원, Range는 **언급 없음** |
| 크기 `size` (`s3.rs:173`) | HeadObject | 지원 |
| 지우기 (`s3.rs:208`) | DeleteObject | 지원 |
| 복사·이름 바꾸기 (`s3.rs:201,235`, 쓰는 곳 `app/files.rs:586`, `app/file_versions.rs:197`) | PutObject + `x-amz-copy-source` (rename은 복사 후 지움) | 복사 머리글로 복사를 알아본다고 적힘 |
| 나열 `list` (`s3.rs:216`, 쓰는 곳 `app/reconcile.rs:99`) | **ListObjectsV2** (`object_store-0.14.2/src/aws/client.rs:999`의 `list-type=2`) | **문서에는 ListObjects(v1)만 있다** |
| 여러 부분 올리기 (`s3.rs:253-339`) | Create/UploadPart/Complete/Abort | 지원(Initiate…). 부분 최대 50 GiB, 최대 10,000개, 객체 최대 10 TiB |
| 부분 검증 | SigV4 서명된 SHA-256 (`s3.rs:263-265`) | SigV4 지원, SigV2 아님 |
| 서버 쪽 암호화 (`s3.rs:74-80`) | `x-amz-server-side-encryption: AES256` | **언급 없음**. 저장 데이터는 언제나 암호화되며 API로 켜고 끌 수 없다고 적힘 |

출처: [S3 호환 API 지원](https://docs.oracle.com/en-us/iaas/Content/Object/Tasks/s3compatibleapi_topic-Amazon_S3_Compatibility_API_Support.htm), [S3 호환 API](https://docs.oracle.com/iaas/Content/Object/Tasks/s3compatibleapi.htm) (둘 다 2026-10-19 갱신), [Object Storage 개요](https://docs.oracle.com/en-us/iaas/Content/Object/Concepts/objectstorageoverview.htm) (2026-08-20).

결론:
- 업로드·다운로드·복사·멀티파트는 문서상 맞는다. fvoci 부분 크기 8 MiB(5 MiB–5 GiB, `docs/configuration.md:145`)는 OCI 한도 안이다.
- **위험 1 — ListObjectsV2**: 문서가 v2를 적지 않았다. 안 받으면 관리 소켓의 파일 대조(`files/reconcile`)만 실패하고 평소 요청은 상관없다 [짐작: 실제로 v2를 받는다는 사용자 보고는 있지만 확인하지 못했다]. 첫 배치 때 대조를 한 번 돌려 본다.
- **위험 2 — Range GET**: 문서에 적히지 않았다. 동영상·큰 파일 이어받기가 이것에 기댄다. 실측 필요.
- **설정**: `server_side_encryption`은 거짓으로 둔다(OCI가 이미 암호화하고 머리글 처리는 문서에 없다). `region`은 꼭 `ap-seoul-1`로 준다. 리전을 비우면 **홈 리전에서만** 쓸 수 있다. 엔드포인트는 `<namespace>.compat.objectstorage.ap-seoul-1.oci.customer-oci.com`, `https` 참. 자격은 사용자의 Customer Secret Key(액세스·비밀 키 쌍)다.
- 준비 검사가 5초에 한 번까지 작은 객체를 쓰고 읽고 지운다(`docs/configuration.md:68`). 늘 불리면 월 약 150만 요청, $0.5 안팎이다.

### R2와 견줘

| | OCI Object Storage (서울) | Cloudflare R2 |
|---|---|---|
| 저장 | $0.0255/GB·월 | $0.015/GB·월 (10 GB 무료) |
| 요청 | $0.0034/1만 건 | 쓰기 $4.50/백만, 읽기 $0.36/백만 |
| 외부 전송 | 월 10 TB 무료, 넘으면 $0.025/GB | 없음 |
| 한국 안 보관 | 서울 리전 | 한국 관할 없음(29번 보고) |

출처: oracle.com 가격표, [R2 가격](https://developers.cloudflare.com/r2/pricing/) (2026-10-30 갱신).

- 200 GB: OCI 약 $4.9, R2 약 $2.9. 2 TB: OCI 약 $52, R2 약 $31.
- fvoci는 다운로드를 서버로 흘려 보낸다(26번 보고). OCI에 서버를 두면 버킷은 같은 리전이라 그 구간은 무료이고 빠르다. R2면 서버가 바깥의 R2에서 받아 온다 [짐작: R2 쪽 전송료는 없지만 왕복이 길다].
- 개인정보를 한국 안에 두려면 OCI 서울 버킷이 낫다.

## 4. 메일: OCI Email Delivery

### 한도와 값 [확인]

- 값: 월 3,000통 무료, 넘으면 **$0.085/1,000통**(가격표). 받는 사람 한 명(To/Cc/Bcc)이나 2 MB 덩어리가 한 통이다.
- 한도: 무료 체험 하루 200통·분당 10통, 유료("Enterprise") 하루 50,000통·분당 18,000통, 메시지 최대 2 MB(요청하면 60 MB). 한도 올리기는 SPF와 DKIM이 있어야 한다([개요](https://docs.oracle.com/en-us/iaas/Content/Email/Concepts/overview.htm), 2026-05-27).
- 승인된 보내는 사람과 반송 억제 목록은 **리전 자산**이다. 서울에서 보내려면 서울에 만든다. Email Delivery가 서울·춘천에 있는지는 [못 봄] (읽은 페이지에 리전 목록이 없었다).

### 저장소의 Oracle 제공자가 가정하는 것

| 가정 | 코드 | 공식 문서와 견줘 |
|---|---|---|
| 엔드포인트 `https://cell0.submit.email.{region}.oci.oraclecloud.com` | `crates/server/src/config/mail.rs:413-416`, 시험 `mail.rs:851` | **문서는 고정 주소를 주지 않는다.** "HTTPS 제출 엔드포인트는 Email Delivery Configuration API로 얻기를 권한다", 콘솔의 "HTTPS sending information"에 있다고만 쓴다([HTTPS 제출](https://docs.oracle.com/iaas/Content/Email/Concepts/email-submission-using-https.htm), 2025-08-05). 다른 주소면 `mail_api_endpoint`로 덮어 쓸 수 있다(`docs/configuration.md:220`). |
| 경로 `POST /20220926/actions/submitEmail` | `oci.rs:21,75` | [못 봄] API 참조가 스크립트 전용이라 읽지 못했다. 판 `20220926`은 코드 주석의 링크와 같다(`oci.rs:3`). |
| 본문 `sender.senderAddress.email`, `sender.compartmentId`, `recipients.to[].email`, `subject`, `bodyText`, `bodyHtml` | `oci.rs:61-70` | Go·Python SDK 예제의 필드 이름과 맞는다. 예제도 bodyText와 bodyHtml을 함께 준다. `messageId`는 예제에만 있고 필수라는 말은 없다. |
| 서명: draft-cavage, RSA-SHA256, `date (request-target) host content-length content-type x-content-sha256` | `http_signature.rs:1-3,83-94`, `oci.rs:78-86` | 서명 머리글 목록은 일반 OCI 요청 서명 문서의 POST 규칙이다(코드 주석이 2019년 보관본으로 시험, `http_signature.rs:105-111`). 메일 문서는 "OCI SDK의 모든 인증 방식"을 받는다고만 한다. |
| 인증은 **API 키 파일만** | `oci.rs:46-48`, `config/mail.rs:192-209` | 문서는 API 키 말고 **인스턴스 주체(instance principal)**도 받는다. OCI 기계 위라면 키 파일 없이 보낼 수 있는데 지금 코드는 그 길이 없다. |
| `400 LimitExceeded`면 나중에, `404 NotAuthorizedOrNotFound`면 길 전체를 닫음, `429`는 나중에 | `oci.rs:115-136`, 공통 `provider.rs:117-128` | [못 봄] 읽은 문서에 SubmitEmail 오류 코드 목록이 없었다. `404`를 설정 문제로 보는 것은 OCI 일반 API 관례와 맞는다 [짐작]. |

### 보내는 도메인 인증과 반송 [확인]

- SPF: 아시아·태평양은 `v=spf1 include:ap.rp.oracleemaildelivery.com ~all`, 하위 도메인마다 따로([configurespf](https://docs.oracle.com/en-us/iaas/Content/Email/Tasks/configurespf.htm)).
- DKIM: 콘솔이 만든 CNAME을 건다. 도메인당 키 둘, 하나만 활성, 여섯 달쯤마다 교체([DKIM](https://docs.oracle.com/en-us/iaas/Content/Email/Tasks/configure-dkim-using-the-console.htm)).
- 반송 경로(MAIL FROM) 맞춤 도메인: [못 봄]. DMARC 정렬이 SPF로는 안 될 수 있으니 DKIM 정렬에 기댄다 [짐작].
- 반송: 영구 실패와 수신 거부 신고는 **억제 목록에 자동으로** 들어간다. fvoci는 Cloudflare 제공자에서만 영구 반송을 응답에서 읽는다(`provider/cloudflare.rs:80-106`). OCI는 받아 둔 뒤 나중에 반송하므로 fvoci는 반송을 모른다(`oci.rs`에 반송 처리 없음). 억제된 주소로 다시 보내면 OCI가 조용히 버린다 [짐작].
- 지금 Cloudflare 메일을 쓰고 있다면 OCI로 바꿀 까닭은 "한 업체에 모으기"와 월 3,000통 무료 정도다.

## 5. 검색

### OCI Search with OpenSearch

- **nori 지원** [확인]: 지원 플러그인 표에 "Analysis Nori — Supports Korean language analysis"가 모든 판(1.2.4–3.2.0)에 체크되어 있다([supportedplugins](https://docs.oracle.com/en-us/iaas/Content/search-opensearch/Concepts/supportedplugins.htm), 2026-10-30).
- 판: 표의 가장 새것은 3.2.0. fvoci 이미지는 OpenSearch 3.9.0이다(`docker/search/Dockerfile:20`). fvoci가 3.2에서 도는지는 [짐작] 요청이 같으니 될 것이다.
- 노드당 최소 메모리 20 GB, 접속은 VCN 안의 **사설 엔드포인트**([ociopensearch](https://docs.oracle.com/en-us/iaas/Content/search-opensearch/Concepts/ociopensearch.htm), 2026-10-23). 보안 모드의 기본 인증 사용자는 읽은 페이지에 없었다 [못 봄]. fvoci는 HTTP Basic만 쓴다(`docs/configuration.md:255`).
- 값: 데이터 노드 둘까지는 **인프라 값만**, 셋 이상(HA)이면 노드당 $0.25/시간 관리비를 더한다([가격](https://www.oracle.com/cloud/search/pricing/)). 노드 모양별 값과 ARM 여부는 [못 봄]. E5 2코어·20 GB로 치면 노드당 월 약 $73, HA 셋이면 관리비만 월 $548이다 [짐작].

### 기계 안에서 직접 [확인 + 짐작]

- 지금 이미지는 보안 플러그인과 nori만 남긴 OpenSearch이고 힙 기본 512 MB다(`docker/search/Dockerfile:27-34`, `docker/compose.yaml:304`). 장면 A는 힙 1 GB, 컨테이너 2 GB 안팎이면 충분할 것으로 본다 [짐작]. B는 힙 2–4 GB로 따로 기계를 둔다 [짐작].
- 색인은 DB에서 다시 만들 수 있으니(`search/reindex`, `docs/configuration.md:57`) 백업할 필요가 없다. 검색이 죽어도 본문 검색만 꺼진다.

## 6. 네트워크와 앞단

### 부하분산기 [확인, [connectionreuse](https://docs.oracle.com/en-us/iaas/Content/Balance/Reference/connectionreuse.htm) 2025-09-30]

- Flexible LB(L7): 리스너는 HTTP/HTTPS/HTTP2/gRPC/TCP. WebSocket 전용 리스너는 없고, 시간 제한 문서가 "HTTP와 WebSocket 연결"을 함께 다룬다. **유휴 시간 기본 HTTP 60초, TCP 300초, 최대 7,200초.** 보내기와 받기의 타이머가 따로다.
- fvoci는 30초마다 Ping을 보내고 브라우저가 Pong으로 답한다(`docs/configuration.md:39`, `contracts/websocket.md:53`). 양방향에 30초마다 패킷이 지나니 60초 제한에 걸리지 않을 것이다 [짐작: Ping 프레임이 타이머를 되돌린다는 문장은 없다]. 그래도 리스너 유휴 시간을 300초쯤으로 올려 두는 편이 안전하다.
- 무료 Flexible LB는 10 Mbps 고정이다. 장면 B의 월 5 TB는 평균 약 15 Mbps라 모자란다. 유료는 기본 $0.0113/시간(월 $8.2) + $0.0001/Mbps·시간(100 Mbps면 월 $7.3)이고 LB가 다룬 데이터 값은 따로 없다(가격표).
- Network LB(L4): 무료, TCP 유휴 기본 360초(CLI·API로만 바꿈, [NLB 유휴](https://docs.oracle.com/en-us/iaas/Content/NetworkLoadBalancer/NetworkLoadBalancers/configure-idle-timeout.htm)). TLS는 뒤의 기계가 끝낸다.

### TLS, 방화벽, 관리 접속

- TLS: Flexible LB는 Certificates 서비스가 관리하는 인증서를 바로 쓰고 새 판을 스스로 따른다([리스너](https://docs.oracle.com/en-us/iaas/Content/Balance/Tasks/managinglisteners_topic-Creating_Listeners.htm)). Let's Encrypt 자동 발급은 문서에 없다 [못 봄]. LB 없이 가면 기계의 Caddy 등이 Let's Encrypt로 받는다 [짐작].
- 방화벽: VCN 보안 목록(서브넷 단위)과 NSG(VNIC 단위). 기계에 공인 IP를 주지 않고 사설 서브넷 + NAT 게이트웨이로 두면 바깥에서 닿을 길이 없다 [짐작: 표준 구성]. OCI의 Ubuntu·Oracle Linux 이미지는 OS 방화벽 규칙도 미리 걸려 있다고 알려져 있다 [짐작].
- 관리 접속: **Bastion**은 비용이 없고, 관리형 SSH(에이전트 필요)와 포트 포워딩·SOCKS5를 준다. 세션 수명 30분–3시간([Bastion](https://docs.oracle.com/en-us/iaas/Content/Bastion/Concepts/bastionoverview.htm)). 지금처럼 Cloudflare Access로 SSH를 열어 두는 것도 된다.

### Cloudflare를 둘 때와 뺄 때 [짐작]

| | 계속 앞에 둠(터널) | 뺌(OCI LB 또는 공인 IP) |
|---|---|---|
| 들어오는 길 | 바깥으로 연 포트 없음(`docker/compose.yaml:46-52`) | NLB(무료)나 Flexible LB, 보안 목록으로 443만 |
| TLS | Cloudflare가 끝냄 | LB나 기계가 끝냄, 인증서 갱신을 우리가 맡음 |
| 요청 본문 | Free·Pro는 100 MB가 천장(`docs/configuration.md:145`) | 한도는 fvoci 설정만 |
| 외부 전송 | Cloudflare를 지나도 OCI의 송출은 같다. 10 TB까지 무료라 A·B 모두 0 | 같음 |
| DDoS·WAF | Cloudflare 몫 | OCI 기본 보호, WAF는 따로 과금 |
| 개인정보 | 미국 회사의 망을 지난다. 이것이 국외 이전인지는 법률 질문(아래) | OCI 서울 안에서 끝남 |
| X-Forwarded-For | `trusted_proxies`를 edge 망으로(`docker/compose.yaml:53-70`) | LB의 사설 대역으로 바꿈 |

## 7. 운영

- **모니터링** [확인/짐작]: Oracle Cloud Agent가 CPU·메모리·디스크·네트워크 지표를 OCI Monitoring에 보낸다(무료 5억 데이터포인트). 경보는 Notifications로 메일(월 1,000통 무료)이나 HTTPS. fvoci는 Prometheus 지표를 따로 연다(`docs/configuration.md:207`). OCI Monitoring이 그것을 바로 긁지 않으므로 [짐작] 기계 안의 Prometheus나 외부 수집기가 필요하다.
- **로그**: 월 10 GB 무료, 넘으면 $0.05/GB(가격표). 컨테이너 로그를 OCI Logging에 보내려면 에이전트 설정이 필요하다 [짐작].
- **블록 볼륨**: 백업 정책 + 다른 리전 복사(위 2절). 볼륨 비동기 리전 복제 문서는 받지 못했다 [못 봄].
- **국외 이전이 없는 구성**: 계산·블록·Object Storage·백업을 모두 `ap-seoul-1`에, 재해 대비 사본은 `ap-chuncheon-1`에 둔다. 남는 바깥 길은 (1) Cloudflare 앞단·R2·Cloudflare 메일, (2) Email Delivery가 서울에 없을 때의 다른 리전, (3) 홈 리전을 서울로 하지 않았을 때 리전 없는 S3 호출이다.
- **CSAP** [못 봄]: Oracle 한국 Trust 페이지에 CSAP·ISMS 언급이 없다([oracle.com/kr/trust](https://www.oracle.com/kr/trust/), 2025-03-04 갱신). 28번 보고가 인용한 기사(2026-06-14)의 CSAP 보유 목록에도 OCI는 없었다. 공공 고객을 받을 계획이면 OCI는 지금 후보가 아니다.

## 8. 비용

가정 [짐작]: 종량제 계정, 무료 몫은 문서 쪽(A1 2코어·12 GB, 블록 200 GB, Standard 10 GB, 외부 전송 10 TB, 메일 3,000통). Cloudflare 터널은 앞에 그대로 둔다(LB 0). 메일은 A 월 3,000통 안, B 월 2만 통. PG 데이터 A 50 GB·B 500 GB.

### 직접 운영

| 항목 | 무료 등급 안 | 장면 A | 장면 B |
|---|---|---|---|
| 계산 | A1 2코어·12 GB 한 대 $0 | A1 4코어·24 GB 한 대 $55.5 − 무료 $28.5 = **$27** | 앱 A1 4·24($55.5), PG A1 4·32($64), 검색 A1 2·16($32) − 무료 $28.5 = **$123** |
| 블록(+10 VPU) | 200 GB $0 | 200 GB **$0** | 750 GB − 200 = **$23** |
| 파일 | 블록에 `local`로 (Object Storage 20 GB뿐) | 200 GB **$4.9** | 2 TB **$52** |
| 백업(Object Storage) | 블록 백업 5개 | 50 GB **$1.3** | 300 GB **$7.7** |
| 외부 전송 | 10 TB까지 $0 | 500 GB **$0** | 5 TB **$0** |
| 메일 | 3,000통 | **$0** | 17,000통 **$1.5** |
| 합계(월) | **$0** | **약 $35** | **약 $210** |

- 무료 등급 안에서 어디까지 [짐작]: 2코어·12 GB 한 대에 fvoci+PG+OpenSearch, 파일은 블록 볼륨(약 100 GB)에. 장면 A의 절반(사용자 500명, 동시 50, 파일 100 GB) 정도까지. 가격표 각주(3K/18K)가 맞다면 4코어·24 GB까지 무료라 장면 A의 계산도 0이 되어 A 합계가 약 $8이 된다.

### 관리형을 쓸 때

| 항목 | 장면 A | 장면 B |
|---|---|---|
| 앱 기계 | A1 2·12 무료 몫 **$0** | A1 4·24 − 무료 **$27** |
| PostgreSQL | 2 OCPU $143 + 50 GB $3.6 = **$147** | 4 OCPU $286 + 500 GB $36 = **$322** (대기 복제 시 약 두 배) |
| OpenSearch | 노드 1(2코어·20 GB 짐작) **$73** | 노드 2 **$146** (HA 3노드면 관리비 포함 약 $767) |
| 파일·백업·메일 | **$6** | **$61** |
| 합계(월) | **약 $226** | **약 $556** (HA 다 켜면 약 $1,500) |

- PG 메모리가 따로 과금되는지는 가격 페이지에 없다. "표준 계산·블록 값 위에 붙는 요금"이라는 문장도 있어 실제로는 더 나올 수 있다 [짐작].
- 참고: 29번 보고의 Lightsail 서울 한 대 구성은 A 기준 약 $60이었다.

## 9. 권고

### OCI에서 먼저 돌린다면 첫 구성

1. **계정**: 종량제로 올린 계정, 홈 리전은 서울. (무료 몫은 그대로 깎이고, 유휴 회수·한도 축소의 위험을 줄인다 [짐작].)
2. **기계**: 서울 A1 4코어·24 GB 한 대(지금 테스트 서버와 같은 꼴), 블록 200 GB. 사설 서브넷 + NAT, 공인 IP 없음.
3. **소프트웨어**: 지금의 `docker/compose.yaml` 그대로(PostgreSQL 18, OpenSearch+nori, fvoci). 이미지는 arm64가 이미 있다.
4. **파일**: 서울 Object Storage 버킷, `file_storage_s3`의 `region=ap-seoul-1`, 경로 방식 엔드포인트, `https=true`, `server_side_encryption=false`.
5. **앞단**: Cloudflare 터널과 Access 유지. 메일도 지금 쓰는 것을 유지.
6. **백업**: `backup.sh` 매일 + 결과를 Object Storage로, WAL 보관(pgBackRest 등), 블록 볼륨 주간 백업을 춘천으로 복사, 달마다 복구 훈련.
7. **첫 배치 전에 볼 것**: (1) S3 호환 API에서 `files/reconcile`(ListObjectsV2)과 Range 다운로드, (2) OCI 메일을 쓸 거면 콘솔의 HTTPS 엔드포인트가 `cell0.submit.email.ap-seoul-1…`인지와 Email Delivery가 서울에 있는지.

까닭: 지금 꼴과 거의 같아 옮길 것이 적고, 장면 A가 월 $35 안팎으로 끝나며, 외부 전송 10 TB 무료라 B까지 전송료가 0이다. 서울 리전 하나로 데이터를 한국 안에 둘 수 있다. 관리형 PG는 18이 생겼지만 값이 기계 값의 몇 배라, 사용자가 늘어 운영 인력이 모자랄 때 옮겨 탈 다음 단계로 남긴다.

### 뒤에 AWS 등으로 옮길 때 다시 할 것

- **DB**: pg_dump/restore(또는 논리 복제)로 옮긴다. 서버는 한 DB에 하나라 옮기는 동안 내려야 한다(`docs/configuration.md:47`). 비슈퍼유저 서버 역할과 기본 권한(`ALTER DEFAULT PRIVILEGES`)을 새로 건다.
- **파일**: 버킷을 통째로 복사(rclone 등). OCI의 외부 전송은 10 TB까지 무료라 2 TB면 전송료가 거의 없다 [짐작]. 설정은 `endpoint`를 비우고(AWS) 자격을 IAM 역할로.
- **메일**: OCI 메일을 썼다면 SES로 바꾸고 SPF·DKIM·승인 발신자, 억제 목록을 새로 만든다.
- **검색**: 옮기지 않고 새 곳에서 `search/reindex`.
- **앞단**: Cloudflare를 계속 두면 터널만 새 기계에서 띄운다. OCI LB를 썼다면 ALB와 인증서(ACM)로 바꾸고 `trusted_proxies`를 고친다.
- **운영**: OCI 백업 정책·Bastion·Monitoring 경보·Notifications를 AWS 쪽(Backup, SSM, CloudWatch)으로 다시 만든다. ARM이면 Graviton으로 같은 이미지를 쓴다.

## 10. 전문가에게 물을 것 (법률 자문 아님)

1. Cloudflare(미국 회사) 앞단과 터널을 지나는 개인정보 처리가 개인정보 보호법상 국외 이전(처리 위탁)에 해당하는지, 고지·동의가 필요한지.
2. OCI 서울 리전만 쓰더라도 Oracle의 해외 운영·지원 인력의 접근을 국외 이전으로 봐야 하는지.
3. Always Free 자원을 유료 SaaS에 쓰는 것이 Oracle Cloud Services Agreement에 맞는지(Oracle 영업 담당 확인 사항).

## 보지 못한 것

- Oracle Cloud Services Agreement 본문, Email Delivery·PostgreSQL·OpenSearch의 리전 목록, SubmitEmail API 참조(스크립트 전용), OpenSearch 노드 모양과 값, PG 지원 모양 페이지, 블록 볼륨 리전 복제 페이지(열리지 않음), Oracle 비용 계산기 API(이 환경의 프록시가 막음).
- 아무것도 실측하지 않았다: OCI 계정을 만들지 않았으므로 S3 호환성, 메일 엔드포인트, LB의 WebSocket 유휴 동작은 모두 문서와 추론이다.
- 앞선 26–29번 보고 브랜치는 이미 지워져 있어 프로젝트 메모리의 요약만 참고했다.

---
읽은 것: 저장소 파일 15개(`storage/s3.rs`, `storage/mod.rs`, `mail/provider/{oci,http_signature,cloudflare}.rs`, `mail/provider.rs`, `config/mail.rs`, `app/reconcile.rs`, `app/files.rs`, `app/file_versions.rs`, `docker/compose.yaml`, `docker/backup.sh`, `docker/search/Dockerfile`, `docs/configuration.md`, `contracts/websocket.md`)와 `object_store` 0.14.2 크레이트 소스 1개, 바깥 자료 약 30쪽(Oracle 문서·가격 페이지, Cloudflare 가격, 기사 2, 서드파티 1). 돌린 것: `git clone`, `grep`, crates.io에서 크레이트 내려받기, Playwright로 Oracle 가격 페이지 3개 그리기. 빌드와 테스트는 돌리지 않았다.

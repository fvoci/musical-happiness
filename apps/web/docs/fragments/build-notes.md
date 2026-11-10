# 조각: 빌드의 `.zst`, 원본에서 떼어 내기, 템플릿의 주소 (한 것과 잰 것)

작성: 2026-11-10 23:00 KST. `PORTING.md`의 "빌드 산출물", "번역", "내장 템플릿의 변환 산출물"에 합칠 조각이다. 스스로 정한 것과 그 이유는 `build-decisions.md`에 있다. 측정은 이 기기(Node 26.13.0, WSL2)에서 했다.

## 1. 빌드가 `.zst`도 만든다

`pnpm --filter web run build`가 `dist/static/` 아래의 텍스트 파일마다 `.br`, `.zst`, `.gz`를 쓴다(`tools/build/output.ts`의 `precompress`). 압축본이 원본보다 작지 않으면 쓰지 않고, `woff2`와 `index.html`은 전처럼 압축하지 않는다. 지금의 빌드는 텍스트 8개에 24개를 쓴다.

**방법.** Node에 들어 있는 `node:zlib`의 `zstdCompress`다. 의존성은 더하지 않았다.

- 문서(`nodejs.org/docs/latest-v26.x/api/zlib.html`, 2026-11-10 22:36 KST에 읽음): Zstd 계열(`zlib.zstdCompress`, `ZstdOptions`, 상수)은 v22.15.0과 v23.8.0에서 들어왔고 v26에서도 **"Stability: 1 - Experimental"**이다. `pledgedSrcSize`(압축할 크기를 미리 알림)는 v26.11.0부터 `zstdCompress`에서 입력 길이가 기본값이다. 이 저장소의 `engines`는 Node 24 이상이라 그 값을 직접 넘긴다.
- 실제 호출: `process.versions.zstd`는 `1.5.7`. 압축한 것을 `zstdDecompressSync`로 풀면 원본과 바이트까지 같다. 실험 기능 경고는 찍히지 않는다.
- 실험 단계라는 것이 남는 위험이다. 옵션의 이름이나 기본값이 바뀌면 `tools/build/output.ts`의 `compressedForms` 한 곳을 고치면 되고, `tests/tools/build.test.ts`가 세 파일이 만들어지고 풀면 원본과 같은지를 본다.

**수준.** 19로 정했다(`ZSTD_LEVEL`). zstd의 설명(`zstd.h` 1.5.7): 보통 수준은 1에서 22(`ZSTD_maxCLevel()`)까지이고 기본값은 3이며, 20 이상은 "ultra"로 메모리를 더 쓴다. 빌드 산출물의 텍스트 8개(합계 806,912바이트)를 수준별로 잰 것:

| 수준        | 합계 바이트 | 걸린 시간 (8개 합계) |
| ----------- | ----------: | -------------------: |
| 3 (기본값)  |     166,248 |               4–8 ms |
| 9           |     134,050 |                11 ms |
| 15          |     113,038 |                41 ms |
| **19**      | **105,453** |          **0.12 초** |
| 20          |     105,457 |              0.12 초 |
| 22          |     105,456 |              0.13 초 |
| (gzip 9)    |     229,032 |                36 ms |
| (brotli 11) |      94,910 |              0.39 초 |

- 19 위로는 얻는 것이 없다(세 값이 4바이트 안에서 같다). 시간은 빌드에 0.1초를 더할 뿐이다. `precompress` 단계 전체(세 형식, 24개 파일)는 0.4–0.5초이고 그 대부분은 brotli다.
- 20 이상은 큰 파일에서 창을 8 MB보다 크게 잡는다. `Content-Encoding: zstd`로 보내는 프레임은 창이 8 MB를 넘으면 안 된다(RFC 9659 3절: 받는 쪽은 8 MB까지만 지원하면 되고, 보내는 쪽은 그보다 큰 창이 필요한 프레임을 만들면 안 된다). 17 MB짜리 입력으로 재 보니 수준 3은 2 MB, 15는 4 MB, **19는 8.0 MB**, 20·21·22는 17 MB(입력 전체)를 요구했다. 19가 한도 안에 드는 가장 높은 수준이다.
- 지금의 파일은 모두 "한 덩어리" 프레임이다(머리에 내용의 길이가 적히고 창이 그 길이와 같다). 가장 큰 파일이 187 KB다. 프레임 머리에서 창을 읽어 8 MB를 넘으면 빌드가 실패한다(`zstdWindowSize`).

**파일별 크기**(바이트, 2026-11-10 22:38 KST의 빌드. `cd apps/web && node tools/build/sizes.ts`가 이 표를 찍는다):

| 파일                       |    원본 |    gzip |     br |    zstd |
| -------------------------- | ------: | ------: | -----: | ------: |
| `THIRD_PARTY_NOTICES.txt`  |  34,706 |   3,136 |  2,266 |   2,850 |
| `faces-d2coding-*.css`     |  59,847 |  12,244 |  6,369 |   7,365 |
| `faces-noto-sans-jp-*.css` | 131,737 |  48,875 | 13,913 |  16,142 |
| `faces-noto-sans-kr-*.css` | 151,945 |  49,315 | 15,939 |  17,694 |
| `faces-noto-sans-sc-*.css` | 103,587 |  22,367 | 11,195 |  12,463 |
| `faces-noto-sans-tc-*.css` | 187,395 |  63,474 | 19,362 |  21,421 |
| `index-*.js`               |  34,235 |  13,426 | 12,156 |  12,951 |
| `index-*.css`              | 103,460 |  16,195 | 13,710 |  14,567 |
| 합계                       | 806,912 | 229,032 | 94,910 | 105,453 |

- 모든 파일에서 brotli 11이 zstd 19보다 작다(합계로 zstd가 11% 크다). zstd는 gzip의 46%다. 글꼴 규칙 CSS에서 gzip만 크게 뒤지는 것은 전에 적은 대로 gzip의 창이 32 KB라서다.

**미리 보기 서버.** `tools/preview.ts`와 `tools/serve.ts`는 `Accept-Encoding`이 받는다고 한 것 가운데 `br` → `zstd` → `gzip`의 순서로 처음 있는 압축본을 보낸다(`tools/build/encodings.ts`). `q=0`으로 적힌 것은 보내지 않는다. 확인한 것(2026-11-10 22:38 KST, `curl -m 3`, `index-*.js`):

| 보낸 `Accept-Encoding`    | `preview.ts`의 `Content-Encoding` | 받은 바이트 |
| ------------------------- | --------------------------------- | ----------: |
| `zstd`                    | `zstd`                            |      12,951 |
| `gzip, deflate, br, zstd` | `br`                              |      12,156 |
| `gzip`                    | `gzip`                            |      13,426 |
| `zstd;q=0, gzip`          | `gzip`                            |      13,426 |
| `identity`                | (없음)                            |      34,235 |

- `zstd`로 받은 것을 풀면 원본과 같다. `serve.ts`에 `app=dist`로 올려도 같은 답이 온다. 모든 응답에 `Vary: Accept-Encoding`이 있다.
- e2e의 브라우저(Chromium 158 headless)는 `http://127.0.0.1`에 `gzip, deflate, br, zstd`를 보낸다(직접 찍어 봤다). `e2e/shell.spec.ts`는 스크립트와 스타일시트가 `br`로 오는지를 본다. 순서를 `zstd` 먼저로 바꾸면 그 테스트도 고쳐야 한다.

**서버 쪽에 전달할 것**(서버 세션의 몫, 여기서는 읽기만 했다): 서버의 `crates/server/src/web.rs`는 `ServeDir`에 `.precompressed_br().precompressed_gzip()`만 켜 두었다. `.zst`를 보내려면 zstd 쪽도 켜야 한다. `static/`에 `<파일>.zst`가 생긴다는 것, 프레임은 수준 19·한 덩어리·창 8 MB 이하라는 것, 지금의 파일은 전부 brotli가 더 작다는 것이 서버가 순서를 정할 때의 재료다.

## 2. 원본 폴더에서 떼어 내기

`webapp/`과 `server/`가 없어도 `check`, `test`, `build`, `e2e`가 통과한다. 그 둘이 있는 이 저장소에서는 전과 같은 검사가 그대로 돌고, 두 가지가 더해졌다(아래 표의 "더한 검사").

### `webapp/`이나 `server/`를 읽던 곳

`apps/web` 아래의 코드·설정·테스트를 `webapp`, `server/`, `../../..`, `REPOSITORY_ROOT`, `boardarchive`, `deny.toml`, `git`으로 찾았다. `src/`, `e2e/`, `spikes/`, `stories/`, `.storybook/`, 설정 파일들(`vite*.config.ts`, `playwright*.config.ts`, `eslint.config.js`, `tsconfig.json`, `project.inlang/`)에는 없다. 원본의 `node_modules`나 스냅샷을 읽는 테스트도 없다.

| 읽던 곳                                                                              | 읽던 것                                                                           | 처리                                                                                                                                                                                             |
| ------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `tools/i18n/conversion.ts`의 `convertAll`(`locales.ts`의 `bundledFiles`, `usage.ts`) | `webapp/i18n/*.json`, `webapp/src/i18n.tsx`, `webapp/src/**`의 소스(id를 쓰는 곳) | **선택 항목.** 원본이 있을 때만 돈다. 없으면 `tools/i18n/standalone.ts`의 `convertStandalone`이 `messages/`를 원천으로 같은 결과를 낸다                                                          |
| `tools/i18n/convert.ts`(`i18n:convert`, `check`의 `--check`)                         | 위와 같다                                                                         | 원본이 있으면 전과 같고, **더한 검사**: 원본 없이 돌려도 같은 파일이 나오는지. 없으면 한 줄을 찍고 `messages/`를 원천으로. 원본의 자리를 `--original <폴더>`로 받는다                            |
| `tools/i18n/review.ts`                                                               | 위 변환과 `webapp/i18n/en.json`("결정으로 제거한 메시지" 표의 영어 문구)          | 원본이 없으면 `ko-review.md` 전부와 `i18n-conversion.md`의 "덮어쓴 문구" 표만 쓰고, 원본을 세는 세 부분(`stats`, `removed-by-decision`, `stale`)은 그대로 두며 한 줄을 찍는다                    |
| `tools/i18n/check-keys.ts`                                                           | (원본을 읽지 않는다. `messages/`와 `project.inlang`만)                            | 그대로                                                                                                                                                                                           |
| `tests/tools/i18n.test.ts`                                                           | `convertAll()`을 여러 번                                                          | 원본을 다시 변환하는 두 묶음(36건)은 **선택 항목**. 메시지 파일에 대한 단언은 원본이 없어도 돈다. **더한 검사**: 두 길의 결과가 같은지, 원본 없는 변환의 동작 18건, 메시지를 원문으로 되읽기 4건 |
| `tools/templates/archive.ts`의 `ARCHIVE` 상수, `readArchive`                         | `server/assets/templates.boardarchive`                                            | **인자로 받게 함.** 자리는 `tools/original.ts` 한 곳(저장소 루트의 그 경로, `FVOCI_TEMPLATE_ARCHIVE`, `--archive <파일>`)                                                                        |
| `tools/templates/output.ts`의 `convertTemplates`                                     | 같은 아카이브                                                                     | 인자로 받는다. `index.json`의 `source.path`는 읽은 자리와 상관없이 원본에서의 자리를 적는다                                                                                                      |
| `tools/templates/convert.ts`                                                         | 같은 아카이브                                                                     | `--archive <파일>`. 없으면 `--check`는 한 줄을 찍고 커밋된 폴더끼리의 앞뒤를 보고, `--write`와 건수 출력은 멈춘다(종료 코드 2)                                                                   |
| `tests/tools/templates.test.ts`                                                      | `convertTemplates()`, `templateDocuments()`                                       | 수·참조·이름·주소·검증에 대한 단언은 **커밋된 산출물**로 늘 돈다. 다시 변환해 바이트까지 견주는 5건은 **선택 항목**                                                                              |
| `tests/ui/body-compare.test.ts`(내 파일이 아니다. 고치지 않았다)                     | `templateDocuments()`로 아카이브의 문서 58개                                      | 함수 쪽에서 처리: 아카이브가 없으면 커밋된 산출물의 본문 58개를 주고 한 줄을 찍는다. 원본이 있으면 전과 같다. 사본에서 그 테스트의 단언이 모두 통과한다                                          |
| `tools/licenses/policy.ts`                                                           | 저장소 루트의 `deny.toml`                                                         | 그대로 둠. 새 저장소에도 있는 파일이고 사본에서 통과한다                                                                                                                                         |
| `tools/fonts/fetch.ts`(건드리지 않는 폴더)                                           | 저장소 루트의 `target/web-fonts/src`(내려받은 글꼴의 캐시)                        | 그대로 둠. 원본 폴더가 아니다. 사본에는 글꼴 산출물을 복사하므로 그 단계가 돌지 않는다                                                                                                           |
| `tools/names/check.ts`                                                               | 원본 폴더는 아니지만 `git ls-files`에 기댄다                                      | git이 목록을 주지 못하면(0개 포함) 폴더를 걸어서 검사하고 한 줄을 찍는다. 그래도 0개면 실패                                                                                                      |
| `tools/licenses/check.ts`                                                            | `pnpm licenses list`(설치된 것)                                                   | 그대로 둠. 사본에서 통과한다                                                                                                                                                                     |
| `tools/templates/output/index.json`의 `source.path`, 문서들의 원본 경로 표기         | 글자로만 있다(읽지 않는다)                                                        | 그대로 둠                                                                                                                                                                                        |
| 루트 `pnpm-workspace.yaml`의 주석(원본 `webapp/`이 구성원이 아니라는 설명)           | 주석                                                                              | 내 파일이 아니라 그대로 둠. 새 저장소에서는 지워도 되는 문장이다                                                                                                                                 |

### 번역: 원본이 없을 때

`docs/i18n-conversion.md`의 "원본 폴더가 없을 때"에 표와 고치는 법을 적었다. 요점:

- `messages/<태그>.json`이 원본에서 온 문구의 원천이 된다. 자체 키(`messages-own/`과 그 묶음 폴더)는 매번 다시 써 넣으므로, 원본이 없어도 키를 더하거나 고치고 `pnpm --filter web run i18n:convert`를 돌리면 반영된다.
- 원본에서 온 문구는 전처럼 `messages-override/<태그>.json`(없던 번역은 `messages-fill/`)으로 고친다. `was`는 대조하지 못하고 기록으로 남는다.
- `check`는 원본이 있든 없든 "메시지 파일이 최신인가"(`convert.ts --check`)와 키 집합(`check-keys.ts`)을 본다.
- 원본이 있는 곳에서 두 길의 결과를 견줬다: 18개 파일의 바이트, id마다의 원문, 덮어쓴·채운·빠진 목록, 두 문서의 생성 부분이 모두 같다.

### 원본 없는 사본에서 확인하는 스크립트

```bash
# 워크스페이스 루트에서. pnpm 이 PATH 에 있어야 한다
node apps/web/tools/detached/verify.ts <사본을 만들 자리> --port <e2e 포트>
```

- `<자리>/copy`에 사본을, `<자리>/logs`에 단계마다의 전체 출력을 둔다. 자리는 새 폴더이거나 비어 있어야 한다. `--replace`는 이 도구가 전에 만든 자리(표식 파일이 있다)만 비우고 다시 쓴다.
- 사본에 드는 것: git이 추적하는 `apps/web` 아래의 파일과 저장소 루트에 바로 놓인 추적 파일(`package.json`, `pnpm-lock.yaml`, `pnpm-workspace.yaml`, `deny.toml`, `.gitignore` 등 33개). **작업 트리의 지금 내용**을 복사한다. 추적되지 않는 파일은 가져가지 않고 목록을 찍는다. `webapp/`이나 `server/`가 사본에 있으면 멈춘다.
- 글꼴 산출물은 `--fonts <폴더>`에서 복사한다(기본값은 이 체크아웃의 `apps/web/src/styles/fonts`. `stamp.json`이 없으면 사본을 만들기 전에 멈춘다).
- 기본으로 사본에서 `git init`과 `git add --all`을 한다(새 저장소에는 `.git`이 있다). `--no-git`은 하지 않고, 바깥 저장소가 대신 답하지 못하게 막는다.
- 단계: `pnpm install --frozen-lockfile` → `check` → `test` → `build` → `e2e`(`FVOCI_E2E_PORT=<포트>`). 단계마다 명령, 종료 코드, 걸린 시간, "원본이 없어 건너뜀" 줄, 출력의 끝 25줄(`--tail <줄 수>`)을 찍는다. 단계마다 20분(`--timeout <분>`)이 지나면 끊는다. 하나라도 실패하면 종료 코드가 1이다(인자가 틀렸거나 글꼴이 없으면 2).
- 사본의 환경에서 `FVOCI_ORIGINAL_WEBAPP`, `FVOCI_TEMPLATE_ARCHIVE`, `FVOCI_E2E_URL`을 지운다(부른 쪽의 설정이 사본을 원본이나 다른 서버로 이끌지 않게).

결과는 아래 "마지막 실행"에 있다.

## 3. 템플릿 산출물의 주소

`url` 속성의 값 가운데 바깥 주소 32건을 전에는 모두 비웠다. 이제 도메인으로 갈라, 원본 회사로 가는 것만 빼고 나머지는 `example.com` 아래로 옮긴다(`tools/templates/rewrite.ts`의 `rewriteLinkValue`). 산출물을 다시 만들었다(2026-11-10 22:40 KST).

| 전      | 지금                           | 건수 | 무엇                                                         |
| ------- | ------------------------------ | ---: | ------------------------------------------------------------ |
| 비움    | **비움**(`outside-link-value`) |   21 | 원본 회사의 도메인: 제품 소개 페이지 10, 보도 자료 2, 문서 9 |
| 비움    | **옮김**(`example-link-value`) |   11 | 지어낸 회사의 사이트 5, 다른 사이트의 그림 4·기사 1·게시물 1 |
| 합계 32 | 합계 32                        |      |                                                              |

- 옮긴 값: `liminarycorp.com` → `https://liminarycorp.example.com`(나머지 넷도 같은 꼴), `https://www.toolbox.com/tech/…` → `https://toolbox.example.com/tech/…`, 그림 4건 → `https://user-images.githubusercontent.example.com/…gif`, 게시물 → `https://twitter.example.com/fvoci/status/…`.
- 글에서 바뀐 곳의 합계는 전과 같은 47곳이다(9 + 5 + 1 + 21 + 11). 바뀐 파일: 템플릿 파일 셋(`08-user-research-sessions.json`, `09-competitive-analysis.json`, `10-content-calendar.json`), `index.json`(건수), `changes.json`, `CHANGES.md`. 손으로 쓴 `README.md`의 표와 설명도 고쳤다.
- 본문의 바깥 링크 처리(결정 14: 링크와 그 문장을 뺀다)는 그대로다(1건).
- **서버 쪽에 전달할 것**: `index.json`의 `changes`에 `example-link-value` 키가 생겼다. 카드의 `url` 속성 값 11개가 빈 값이 아니라 `https://…example.com…`이다.

## 마지막 실행

2026-11-10 23:02–23:10 KST, 이 조각을 담은 커밋의 내용으로(커밋하기 직전의 작업 트리).

원본이 있는 이 워크트리에서(전과 같은 검사가 돈다):

| 명령    | 결과                                                                                                                                          | 시간 |
| ------- | --------------------------------------------------------------------------------------------------------------------------------------------- | ---- |
| `check` | 통과. svelte-check 3,042 파일 오류 0, 이름 검사 599 파일, 메시지 파일은 "원본에서 다시 변환한 것과 같고, 원본 없이 돌려도 같다", 키 검사 통과 | 66초 |
| `test`  | 통과. 세 시간대 각 55 파일, 1,666건 통과, 건너뛴 것 없음(기준 커밋에서는 54 파일, 1,601건)                                                    | 53초 |
| `build` | 통과. `.br/.zst/.gz` 24개                                                                                                                     | 2초  |
| `e2e`   | 16 통과, 14 건너뜀(전과 같다)                                                                                                                 | 8초  |

`webapp/`과 `server/`가 없는 사본에서(`node apps/web/tools/detached/verify.ts target/detached/<이름> --port 18312`):

| 사본                         | `install` | `check`                                               | `test`                                                | `build` | `e2e`              |
| ---------------------------- | --------- | ----------------------------------------------------- | ----------------------------------------------------- | ------- | ------------------ |
| `git init`과 `git add --all` | 통과      | 통과. 이름 검사 599 파일(git의 목록)                  | 통과. 세 시간대 각 55 파일, 1,625건 통과, 41건 건너뜀 | 통과    | 16 통과, 14 건너뜀 |
| `--no-git`                   | 통과      | 통과. 이름 검사 599 파일(폴더를 걸어서, 한 줄을 찍음) | 같다                                                  | 통과    | 같다               |

- 건너뛴 41건은 원본을 다시 읽는 테스트다: 번역 36건, 템플릿 5건. 사본의 출력에 "원본이 없어 건너뜀" 줄이 `check`에 1개(번역의 재변환), `test`에 2개(번역, 템플릿) 찍힌다. `tests/ui/body-compare.test.ts`도 한 줄을 찍는다(커밋된 문서를 쓴다는 것. 에이전트가 돌릴 때의 vitest 리포터는 이 줄을 보여 주지 않는다).
- `.git`이 없는 사본을 이 워크트리의 `target/` 아래에 두면 `git ls-files`는 바깥 워크트리를 보고 0개를 답한다(직접 확인). 고치기 전의 이름 검사를 그 사본에서 돌리면 `names: ok (0 files)`를 찍고 통과했다(직접 확인). 지금은 폴더를 걸어 599개를 본다.
- 스크립트가 실패를 알리는지도 봤다: 사본을 뜬 뒤에 문서 하나가 Prettier 꼴이 아니었던 실행에서 `check`만 실패로 찍히고 나머지 단계는 돌았으며 종료 코드가 1이었다. `pnpm`이 PATH에 없으면 `install`이 실패로 찍히고 나머지는 "돌리지 않음"이 된다.
- `spikes:e2e`는 돌리지 않았다. 스파이크, `lib/drag`, `lib/editor`를 건드리지 않았다.

## 확인하지 못한 것

- Node의 zstd가 실험 단계에서 벗어날 때 옵션이 바뀌는지. Node 24(이 저장소의 `engines` 하한)에서 `pledgedSrcSize`를 받는지는 돌려 보지 못했다(이 기기에는 26뿐이다).
- 브라우저가 `.zst` 응답을 실제로 푸는지. 미리 보기 서버가 `br`을 먼저 보내므로 e2e의 브라우저는 `.zst`를 받지 않는다. `curl`로 받아 Node로 푼 것까지 봤다.
- 실제 Rust 서버에 이 빌드를 물려 `.zst`가 나가는지. 서버가 아직 zstd 쪽을 켜지 않았다.
- 원본이 없을 때 `removed.json`에 id를 더하는 길은 테스트로만 봤다(실제 파일을 고쳐 돌려 보지는 않았다).
- 사본 검증은 이 워크트리의 `target/` 아래에서만 돌렸다. 저장소 밖의 자리(다른 디스크, 다른 사용자)에서는 돌려 보지 않았다.

# fvoci 웹앱 포팅 노트

`apps/web`은 원본 웹앱(`webapp/`, React)을 Svelte 5로 새로 짠 것이다. 계획은 저장소 밖의 `~/.claude/plans/fvoci-web-svelte.md`에 있고, 이 문서는 실제로 한 것과 확인한 것을 적는다. 원본 참조는 저장소 기준 상대 경로다. 시각은 모두 KST다.

| 단계    | 내용                                                            | 상태                        |
| ------- | --------------------------------------------------------------- | --------------------------- |
| W0      | 스택, 공용 UI, 디자인 토큰, 글꼴 도구, 라이선스 검사, 검증 명령 | 완료 (2026-11-10 17:40 KST) |
| W1      | 순수 로직 `src/lib/logic`과 원본 단위 테스트                    | 아래 "W1" 절                |
| W2 이후 | 서버의 새 와이어 설계가 나온 뒤                                 | 시작하지 않음               |

## 검증 명령

워크스페이스 루트에서 실행한다. 패키지 이름이 `web`이다.

```bash
pnpm install --frozen-lockfile
pnpm --filter web run check     # paraglide 컴파일 → svelte-check → eslint → prettier --check → 라이선스 검사 → 이름 검사
pnpm --filter web run test      # paraglide 컴파일 → vitest run
pnpm --filter web run build     # paraglide 컴파일 → 글꼴 도구 → vite build
pnpm --filter web run e2e       # Playwright. 먼저 build 가 있어야 한다
```

- `check`에 `tsc`를 따로 넣지 않았다. `svelte-check`가 `tsconfig.json`의 `include` 전체(`.ts` 포함)를 같은 설정으로 검사한다. `tools/*.ts`와 `vite.config.ts`의 타입 오류가 `svelte-check` 출력에 나오는 것을 확인했다.
- 보조 스크립트: `i18n`(메시지 컴파일), `fonts`(글꼴 도구), `licenses`, `names`, `lint`, `format`, `preview`(빌드 결과를 서버 규칙대로 서빙), `storybook`, `build-storybook`.
- `e2e`는 `FVOCI_E2E_URL`이 있으면 그 서버를, 없으면 `tools/preview.ts`를 띄워 대상으로 삼는다. `preview.ts`에는 API가 없으므로 정적 뼈대 테스트만 그쪽으로 돌 수 있다. 제품 시나리오는 W2부터 Rust 서버를 대상으로 한다.
- pnpm은 이 기기에 없어서 `target/tools/node`에 설치해 썼다: `npm install --prefix target/tools/node pnpm@12.18.0`, 그 뒤 `export PATH=$PWD/target/tools/node/node_modules/.bin:$PATH`.

## 고정한 판 (2026-11-10 16:50 KST 확인)

출처는 npm 레지스트리다. 패키지마다 `npm view <이름> version`(dist-tag `latest`)과 `npm view <이름> "time[<판>]"`(공개 시각)을 조회했고, 호환 범위는 `npm view <이름> peerDependencies`로 확인했다. 기억으로 정한 판은 없다.

기준: `latest`를 쓴다. 다만 공개된 지 24시간이 안 된 판은 직전 판으로 내린다. pnpm의 `minimumReleaseAge` 기본값이 1440분(1일)이고(출처: pnpm 문서 settings/dependency-resolution), 이것을 `pnpm-workspace.yaml`에 명시해 조건에 안 맞으면 설치가 실패하게 했다.

| 패키지                                               | 고정 판 | 공개 (UTC) | 메모                                                                                                                             |
| ---------------------------------------------------- | ------- | ---------- | -------------------------------------------------------------------------------------------------------------------------------- |
| pnpm                                                 | 12.18.0 | 2026-11-05 | `packageManager`에 sha512와 함께 적었다                                                                                          |
| svelte                                               | 5.57.3  | 2026-10-26 |                                                                                                                                  |
| vite                                                 | 8.3.6   | 2026-11-05 | `latest`는 8.3.7(2026-11-09 10:25, 확인 시점에 22시간 전). 변경 기록상 Rolldown 갱신이고 보안 수정이 아니어서 직전 판으로 뒀다   |
| @sveltejs/vite-plugin-svelte                         | 7.3.1   | 2026-09-23 |                                                                                                                                  |
| typescript                                           | 6.0.3   | 2026-04-16 | `latest`는 7.0.2. `svelte-check` 4.7.8의 peer가 `^5 \|\| ^6`, `typescript-eslint` 8.72의 peer가 `<6.1.0`이라 6.x의 최신으로 뒀다 |
| tailwindcss, @tailwindcss/vite                       | 4.3.5   | 2026-11-06 |                                                                                                                                  |
| bits-ui                                              | 2.20.6  | 2026-11-05 |                                                                                                                                  |
| shadcn-svelte (CLI, CSS)                             | 1.8.0   | 2026-11-03 | `shadcn-svelte/tailwind.css`를 가져오므로 의존성이기도 하다                                                                      |
| cn                                                   | 0.5.2   | 2026-11-02 | 생성된 `utils.ts`가 쓴다                                                                                                         |
| tailwind-variants                                    | 3.3.1   | 2026-08-03 | `latest`는 4.0.2. CLI가 요구한 범위가 `^3.3.0`이고 4.x는 새 메이저다                                                             |
| tw-animate-css                                       | 1.4.0   | 2025-09-24 |                                                                                                                                  |
| @lucide/svelte                                       | 1.70.0  | 2026-11-06 | `latest` 1.71.0은 확인 2시간 전에 공개됐다                                                                                       |
| svelte-sonner                                        | 1.2.3   | 2026-11-02 |                                                                                                                                  |
| @internationalized/date                              | 3.12.4  | 2026-09-01 | bits-ui의 peer, 달력 컴포넌트가 쓴다                                                                                             |
| @inlang/paraglide-js                                 | 3.2.1   | 2026-11-07 |                                                                                                                                  |
| @inlang/plugin-message-format                        | 4.6.0   | 2026-11-03 | `project.inlang/settings.json`이 CDN 주소가 아니라 `node_modules`의 파일을 가리킨다                                              |
| vitest                                               | 5.1.0   | 2026-11-02 |                                                                                                                                  |
| jsdom                                                | 31.0.0  | 2026-11-05 |                                                                                                                                  |
| @testing-library/svelte                              | 5.4.2   | 2026-06-23 |                                                                                                                                  |
| @testing-library/jest-dom                            | 7.0.1   | 2026-08-09 |                                                                                                                                  |
| @playwright/test                                     | 1.65.0  | 2026-11-05 | Chromium 빌드 1254                                                                                                               |
| storybook, @storybook/svelte, @storybook/svelte-vite | 10.6.6  | 2026-11-05 |                                                                                                                                  |
| svelte-check                                         | 4.7.8   | 2026-11-01 |                                                                                                                                  |
| eslint                                               | 10.14.0 | 2026-10-30 |                                                                                                                                  |
| @eslint/js                                           | 10.0.1  | 2026-02-06 |                                                                                                                                  |
| typescript-eslint                                    | 8.72.1  | 2026-10-26 | `latest` 8.72.2는 확인 15시간 전에 공개됐다                                                                                      |
| eslint-plugin-svelte                                 | 3.24.1  | 2026-11-07 |                                                                                                                                  |
| eslint-config-prettier                               | 10.1.8  | 2025-07-18 |                                                                                                                                  |
| globals                                              | 17.14.0 | 2026-11-01 |                                                                                                                                  |
| prettier                                             | 3.9.10  | 2026-10-10 |                                                                                                                                  |
| prettier-plugin-svelte                               | 4.1.1   | 2026-06-15 |                                                                                                                                  |
| prettier-plugin-tailwindcss                          | 0.8.1   | 2026-07-15 |                                                                                                                                  |
| @types/node                                          | 26.6.6  | 2026-10-28 | Node 26.13.0에서 작업했다                                                                                                        |
| subset-font                                          | 2.9.0   | 2026-09-20 | 글꼴 도구. HarfBuzz의 WebAssembly 빌드를 쓴다                                                                                    |

플랜의 스택 가운데 Tiptap은 넣지 않았다. 편집기는 W3 전의 스파이크에서 다룬다.

글꼴 원본과 유니코드 자료(2026-11-10 17:07 KST 확인, `tools/fonts/sources.json`에 주소와 SHA-256):

| 자료                                          | 판                                                         | 출처                                           |
| --------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------- |
| Noto Sans, Noto Sans KR·JP·SC·TC (가변, wght) | `google/fonts` 커밋 `5bb28733` (2026-11-08)                | GitHub API로 커밋과 파일 크기 확인             |
| Noto Serif, Noto Serif KR·JP·SC·TC            | 같은 커밋                                                  | 도구가 지원만 한다                             |
| D2Coding Regular·Bold                         | 1.4.0 (`naver/d2-coding-font` 태그 `VER1.4.0`, 2026-10-03) | GitHub 릴리스 목록. 1.4.0에도 가변 글꼴은 없다 |
| Unihan                                        | 유니코드 18.0.0                                            | `unicode.org/Public/UCD/latest`의 ReadMe       |

## 프로젝트 구조

```
apps/web/
  index.html               서버가 {{.BaseURL}} 을 채우는 뼈대
  src/app/                 진입점
  src/lib/ui/              shadcn-svelte 생성물 (README 에 수정 기록)
  src/lib/logic/           순수 로직 (W1)
  src/lib/i18n/paraglide/  메시지 컴파일 결과 (생성물, git 무시)
  src/lib/fonts.ts         @font-face 규칙의 지연 로드
  src/lib/theme.svelte.ts  밝은·어두운 화면 상태
  src/styles/app.css       디자인 토큰
  src/styles/fonts/        글꼴 도구의 산출물 (git 무시)
  messages/                en, ko 메시지
  tests/                   vitest: logic, tokens, tools (node), ui (jsdom)
  e2e/                     Playwright
  stories/                 Storybook
  tools/                   build(산출물 규칙), fonts, licenses, names, preview
```

루트에 추가한 파일은 셋이다: `package.json`(`packageManager` 고정), `pnpm-workspace.yaml`, `pnpm-lock.yaml`. 워크스페이스 구성원은 `apps/*`뿐이고 원본 `webapp/`은 들어가지 않는다. 루트 `.gitignore`는 건드리지 않았다(`apps/web/.gitignore`를 따로 뒀다).

## 빌드 산출물

`dist/index.html` + `dist/static/`이다. 서버의 `webpath`를 `dist`로 두면 바로 서빙된다.

- **파일명의 해시는 16자리 16진수다.** 서버(`crates/server/src/web.rs`의 `has_content_hash`)는 이름을 `-`, `.`, `_`로 나눈 조각 가운데 "8자 이상의 16진수이고 숫자가 하나 이상"인 것이 있어야 내용 해시로 보고 `immutable`을 붙인다. Vite의 기본 해시는 base64 계열이라 이 조건에 안 맞는다. `hashCharacters: 'hex'`로 바꾸고, `static/` 아래에 조건을 못 맞춘 파일이 있으면 빌드가 실패하게 했다(`tools/build/output.ts`).
- **base path를 빌드에 넣지 않는다.** `index.html`은 `{{.BaseURL}}/static/…`으로, 스크립트와 스타일시트끼리는 상대 경로로 참조한다(`experimental.renderBuiltUrl`). `/boards` 아래의 깊은 주소에서 파일이 모두 풀리는 것을 확인했다.
- **사전 압축.** `static/` 아래의 텍스트 파일마다 `.br`(brotli 11)과 `.gz`(gzip 9)를 만든다. woff2는 이미 압축돼 있어 뺀다. `index.html`은 서버가 요청마다 치환해 보내므로 압축하지 않는다.
- **고지.** `static/THIRD_PARTY_NOTICES.txt`. 이름에 해시가 없어 서버가 `no-cache`로 보낸다(의도).

실제 Rust 서버(`hafgufa/target/release/fvoci-server`, 읽기만 함)에 `webpath`를 이 `dist`로 주고 확인한 것(2026-11-10 17:24 KST): `{{.BaseURL}}` 치환, 해시 파일의 `Cache-Control: public, max-age=31536000, immutable`, `Accept-Encoding`에 따른 `br`·`gzip`·무압축 응답과 `Vary`, 고지 파일의 `no-cache`, woff2의 `font/woff2`. `FVOCI_E2E_URL`로 그 서버를 대상으로 한 Playwright 6건도 통과했다.

빈 화면 빌드의 크기(2026-11-10 17:33 KST, 바이트):

| 파일                       | 원본    | br     | gzip   |
| -------------------------- | ------- | ------ | ------ |
| `index-*.js`               | 33,386  | 11,895 | 13,110 |
| `index-*.css`              | 97,359  | 12,915 | 15,196 |
| `faces-noto-sans-kr-*.css` | 87,765  | 9,306  | 35,646 |
| `faces-noto-sans-jp-*.css` | 131,281 | 13,860 | 48,756 |
| `faces-noto-sans-sc-*.css` | 103,587 | 11,195 | 22,367 |
| `faces-noto-sans-tc-*.css` | 187,395 | 19,362 | 63,474 |
| `faces-d2coding-*.css`     | 59,847  | 6,369  | 12,244 |

글꼴 규칙 CSS는 같은 `unicode-range` 목록이 굵기 구간마다 되풀이되어 brotli에서 크게 줄고 gzip에서는 덜 줄어든다(gzip의 창이 32KB라 되풀이를 못 본다).

주 스타일시트가 97KB인 것은 Tailwind가 `src/lib/ui`의 컴포넌트 24종이 쓰는 클래스를 모두 내기 때문이다(화면이 쓰는 것만 남기는 것이 아니라 소스에 나오는 것을 낸다). 번들 크기 상한은 플랜대로 W2의 얇은 조각에서 정한다.

## 디자인 토큰 (`src/styles/app.css`)

- **색.** 밝은 화면 `:root`, 어두운 화면 `.dark`. 출발점은 생성기의 `neutral` 팔레트이고 다음을 바꿨다.
  - `--input-border`(새 토큰): 입력 칸의 테두리. 생성기의 `--input`은 바탕과 1.2:1이라 테두리로 쓸 수 없다.
  - `--ring`, `--sidebar-ring`: 초점 표시. 회색(2.6:1)에서 파란색으로(밝은 화면 5.0:1, 어두운 화면 7.4:1).
  - `--muted-foreground`: 밝은 화면에서 `--muted` 위 4.3:1이던 것을 5.1:1로.
  - `--destructive`: 삭제 버튼은 같은 색의 옅은 바탕 위에 같은 색 글자다(`bg-destructive/10`, 호버 `/20`). 호버 상태에서 3.8:1이던 것을 4.6:1로(대비 테스트가 잡았다).
  - 브랜드 색은 정해진 것이 없어 무채색 그대로다.
- **글자 단계.** 모두 rem이다. `--text-sm`이 UI 기본이고 0.9375rem(15px)이다. `--text-base`는 1rem으로, 본문과 좁은 화면의 입력 칸에 쓴다. 뷰포트 메타에서 `maximum-scale`과 `user-scalable`을 뺐다.
- **줄 높이.** UI 1.45, 본문 1.6. `:lang(ko)`, `:lang(ja)`, `:lang(zh)`에서는 1.55, 1.75. `lang`이 다른 하위 트리에도 적용되도록 `@theme inline`으로 변수 자체를 참조하게 했다.
- **글꼴 스택.** `'Noto Sans'`, 그다음 CJK 네 family를 현재 언어의 것부터(`--font-cjk`), 그다음 시스템 글꼴. 고정폭은 `'D2Coding'`.
- **간격·모서리.** `--spacing` 0.25rem, `--spacing-touch` 2.75rem(손가락으로 누르는 대상의 최소 변), `--radius` 0.5rem.
- **초점.** 자체 표시가 없는 요소는 `--ring` 색의 2px 윤곽선. 생성 컴포넌트는 같은 색의 테두리와 링을 쓴다.
- **대비 검사.** `tests/tokens/contrast.test.ts`가 `app.css`를 직접 읽어 두 화면 각각 글자 15쌍(4.5:1), 입력 테두리·초점 11쌍(3:1), 삭제 버튼의 옅은 바탕을 검사한다. 반투명 색은 브라우저처럼 감마 인코딩된 sRGB에서 합성해 계산한다.

## 글꼴 (`tools/fonts`)

`pnpm --filter web run fonts`(= `build`의 한 단계)가 원본을 내려받아(SHA-256 검사, `target/web-fonts/src`에 캐시) family마다 "상용" 파일 하나와 "잔여" 파일 하나를 WOFF2로 만들고 `@font-face` 규칙을 쓴다. 산출물(`src/styles/fonts/`)은 커밋하지 않는다. 입력이 같으면 건너뛴다. 전체 빌드는 이 기기에서 1분 50초이고, 건너뛸 때의 `build`는 2초다.

글자 집합 정의(커밋됨):

| family       | 상용 파일에 들어가는 것                                                            | 정의                                                     |
| ------------ | ---------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Noto Sans    | 기본 라틴, 라틴-1, 라틴 확장-A, 결합 부호, 일반 문장부호, 통화·글자꼴 기호 등      | `charsets.ts`의 블록                                     |
| Noto Sans KR | 현대 한글 음절 11,172자 전부, 호환 자모, CJK 문장부호, 전각 기호, KS X 1001의 기호 | `charsets/ks-x-1001-symbols.txt` (986자)                 |
| Noto Sans JP | 히라가나, 가타카나, 상용한자 2,136자, JIS X 0208의 기호                            | `charsets/joyo.txt`, `jis-x-0208-symbols.txt` (524자)    |
| Noto Sans SC | 통용규범한자표 1급 3,500자, GB 2312의 기호                                         | `charsets/tgh-level1.txt`, `gb-2312-symbols.txt` (728자) |
| Noto Sans TC | Big5 1수준 5,401자, 주음부호, Big5의 기호                                          | `charsets/big5-level1.txt`, `big5-symbols.txt` (409자)   |
| D2Coding     | 기본 라틴, 라틴-1, 문장부호, 화살표, 수학 기호, 상자 그리기, 도형                  | `charsets.ts`의 블록                                     |

한자 목록은 `derive-charsets.ts`가 Unihan 18.0.0의 `kJoyoKanji`, `kTGH`와 Big5 바이트 범위에서, 기호 목록은 각 국가 문자 집합의 기호 영역을 플랫폼 디코더로 풀어서 만든다. CJK family에서는 Noto Sans(라틴)가 가진 글자를 두 파일 모두에서 뺀다. 스택에서 라틴 family가 앞에 있어 그 글자는 CJK family에 오지 않기 때문이다.

파일 크기(2026-11-10 17:32 KST):

| 파일                            | 글자 수 | 바이트     |
| ------------------------------- | ------- | ---------- |
| `noto-sans-common.woff2`        | 779     | 112,876    |
| `noto-sans-rest.woff2`          | 2,315   | 426,924    |
| `noto-sans-kr-common.woff2`     | 12,321  | 1,225,672  |
| `noto-sans-kr-rest.woff2`       | 10,324  | 2,674,780  |
| `noto-sans-jp-common.woff2`     | 2,719   | 819,520    |
| `noto-sans-jp-rest.woff2`       | 13,484  | 3,534,452  |
| `noto-sans-sc-common.woff2`     | 4,228   | 1,143,144  |
| `noto-sans-sc-rest.woff2`       | 26,133  | 6,842,216  |
| `noto-sans-tc-common.woff2`     | 5,932   | 1,693,848  |
| `noto-sans-tc-rest.woff2`       | 14,284  | 4,017,412  |
| `d2coding-regular-common.woff2` | 809     | 28,712     |
| `d2coding-regular-rest.woff2`   | 19,127  | 1,443,768  |
| `d2coding-bold-common.woff2`    | 809     | 29,456     |
| `d2coding-bold-rest.woff2`      | 19,127  | 1,538,192  |
| 합계                            |         | 25,530,972 |

Noto 계열은 가변 글꼴(wght 축)이라 굵기별 파일이 없다. Noto Sans 원본에는 wdth 축도 있어 100으로 고정했다. D2Coding은 가변 빌드가 없어 Regular와 Bold가 따로다.

첫 화면의 글꼴 요청 수(Chromium headless, 기기에 Noto 없음, 2026-11-10 17:33 KST): 영어 1건(`noto-sans-common`, 113KB), 한국어 2건(+`noto-sans-kr-common`, 1.2MB), 일본어 2건(+`jp-common`), 번체 2건(+`tc-common`). 한글 주석이 든 코드 블록이 있으면 D2Coding 2건이 더해진다.

구현하면서 확인한 것:

- **한 family의 모든 파일은 `font-weight` 기술자가 같아야 한다.** 처음에는 상용 파일에 굵기별 규칙 넷, 잔여 파일에 `400 700` 범위 규칙 하나를 썼다. Chromium은 기술자가 가장 잘 맞는 규칙 묶음을 먼저 고른 뒤에야 `unicode-range`를 보므로, 상용 파일의 규칙이 통째로 무시되어 라틴 글자가 시스템 글꼴로 그려졌다(`CSS.getPlatformFontsForNode`로 확인). 지금은 모든 파일이 `100 599`와 `600 900` 두 규칙을 갖는다.
- **규칙이 둘인 이유는 `local()`이다.** 기기에 굵기별 파일로 설치된 Noto가 있으면 규칙 하나로는 Regular만 가리킬 수 있어 굵은 글자가 Regular를 뭉갠 모양이 된다. 600 이상은 설치된 Bold를 쓰게 했다. 서버에서 받는 가변 파일은 두 구간 어디서든 요청한 굵기를 낸다. 같은 파일을 두 규칙이 가리키므로 두 번째 굵기가 처음 나올 때 요청이 한 번 더 생기는데, 디스크 캐시에서 오고 전송은 0바이트다(CDP로 확인).
- **`unicode-range`는 파일끼리 겹치지 않게 정확히 적는다.** 넓은 범위로 겹쳐 두면 Chromium이 아직 못 받은 규칙을 건너뛰면서 다음 규칙의 파일도 함께 받는다. 정확히 적은 덕에, 한국어 화면에 KR 글꼴에 없는 한자(龘)가 나오면 KR 잔여 파일(2.7MB)을 건너뛰고 그 글자가 있는 SC 잔여 파일로 바로 간다.
- **그 값으로 규칙 CSS가 크다**(family당 압축 전 88–187KB, brotli 9–19KB). 그래서 라틴 family의 규칙만 주 스타일시트에 넣고, 나머지는 `src/lib/fonts.ts`가 따로 받는다: 현재 언어의 family는 바로, 다른 family와 D2Coding은 브라우저가 한가할 때.
- **기호는 국가 문자 집합 기준으로 상용 파일에 넣는다.** 넣기 전에는 `※`나 `→` 하나가 KR 잔여 파일을 받게 했다.

확인하지 못한 것:

- `local()`에 적은 이름(`Noto Sans KR`, `NotoSansKR-Regular`, `Noto Sans CJK KR Regular`, `NotoSansCJKkr-Regular` 등)이 실제 기기(Android, 리눅스 배포판, 글꼴을 직접 설치한 Windows·macOS)에서 맞는지. 이 기기에는 Noto가 없어 "없을 때 내려받는다" 쪽만 확인했다.
- Firefox와 Safari에서의 동작. Chromium headless만 돌렸다.
- 설치된 글꼴이 굵기별 파일일 때 500(Medium)은 Regular로, 600(SemiBold)은 Bold로 그려진다(설계상 그렇고, 실제 화면은 보지 못했다).

## 제3자 라이선스 (`tools/licenses`)

허용 목록은 따로 두지 않고 루트 `deny.toml`의 `[licenses] allow`를 읽는다(`policy.ts`). 검사는 둘이다.

1. **설치된 전체**(`pnpm --filter web run licenses`, `check`에 포함): `pnpm licenses list --json`의 모든 패키지. 허용 목록에 없으면 `exceptions.json`의 `tooling`에 이름·라이선스·사유가 있어야 한다. 맞는 패키지가 없는 예외는 오류다.
2. **번들에 들어간 것**(빌드 때, `notices.ts`): Vite의 라이선스 보고(`build.license`)에 나온 패키지 + `app.css`가 가져오는 CSS 패키지 + 도구가 소스에 써 넣은 코드의 출처(`vendored`: shadcn-svelte, paraglide). 예외 없이 허용 목록만 적용하고, 통과하면 `static/THIRD_PARTY_NOTICES.txt`를 쓴다.

현재 결과(2026-11-10 17:36 KST): 설치된 패키지는 MIT 265, Apache-2.0 23, ISC 10, BSD-2-Clause 9, BSD-3-Clause 6, Apache-2.0 WITH LLVM-exception 5, 그 밖의 허용 3. 예외 6건은 모두 빌드 기계에서만 도는 도구다: lightningcss와 그 플랫폼 바이너리(MPL-2.0), lru-cache·minimatch(BlueOak-1.0.0), tslib(0BSD), argparse(Python-2.0). 번들에는 svelte, tailwindcss, tw-animate-css, shadcn-svelte, @inlang/paraglide-js(모두 MIT)와 글꼴 6종이 들어간다.

결정이 필요한 것:

- **글꼴은 OFL-1.1인데 `deny.toml`의 목록에 없다.** Rust 크레이트 가운데 그 라이선스가 없어서다. 지금은 `notices.ts`가 글꼴 파일에 한해 OFL-1.1만 받는다(다른 라이선스의 글꼴은 빌드 실패). `deny.toml`에 `OFL-1.1`을 넣어 한 목록으로 만들지는 서버 트랙의 파일이라 손대지 않았다.
- **서버 쪽 `NOTICE`와 합치는 일은 하지 않았다.** 루트 `NOTICE`는 서버 트랙이 `cargo about`으로 만드는 파일이다. 웹 고지는 빌드 산출물로만 나온다. 패키징 단계(Docker 이미지)에서 둘을 이어 붙이는 것이 자연스럽다.

## 이름 검사

`pnpm --filter web run names`(`check`에 포함)가 `apps/web` 아래에서 git이 추적하거나 추적할 파일의 이름과 내용에 원본 제품의 두 이름이 있는지 대소문자 구분 없이 찾는다. 현재 0건이다.

## 이 기기에서 걸린 것

- **닫힌 로컬 포트로의 연결이 거절되지 않고 매달린다**(WSL2). `curl`은 `-m`으로 끊긴다. Playwright는 `webServer.url`이나 `port`가 있으면 서버를 띄우기 전에 그 주소가 비었는지 연결해 보는데, 여기서 2분 넘게 멈췄다. `url` 대신 `wait: { stdout: … }`로 서버의 준비 출력을 기다리게 했다.
- **Storybook 빌드에는 앱의 산출물 플러그인을 빼야 한다.** `.storybook/main.ts`의 `viteFinal`에서 `fvoci:` 플러그인, `renderBuiltUrl`, 해시 파일명 설정을 걷어 낸다.
- **글꼴 도구의 건너뛰기 판단은 `tools/fonts` 아래 모든 파일의 해시다.** 주석 하나만 고쳐도 다음 `build`가 글꼴을 다시 만든다(약 2분).

## 미결 사항 (W0)

| 항목                                  | 내용                                                                                                                                                                                                                                                                                |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `{{.BaseURL}}`을 스크립트가 읽는 방법 | `index.html`의 자원 주소에는 넣었다. 라우터와 API 클라이언트가 읽을 값(원본은 인라인 스크립트 `window.baseURL`)은 W2에서 정한다                                                                                                                                                     |
| CJK 문장부호의 글꼴                   | 스택에서 라틴 family가 앞이라 `…`, `“ ”`, `·` 같은 문장부호는 일본어·중국어 문장 안에서도 Noto Sans(라틴)의 모양으로 그려진다(말줄임표가 가운데가 아니라 아래에 붙는다). 언어별로 문장부호를 CJK family에 넘기려면 라틴 family의 `unicode-range`에서 그 글자를 빼는 변형이 필요하다 |
| 비 CJK 언어에서의 CJK 순서            | `lang`이 ko·ja·zh가 아니면 KR, JP, SC, TC 순이다. 영어 화면에 일본어 글이 나오면 한자가 한국어 자형으로 그려진다                                                                                                                                                                    |
| 브랜드 색                             | 무채색 그대로                                                                                                                                                                                                                                                                       |
| Noto Serif의 용도                     | 도구만 지원한다(`--family noto-serif,noto-serif-kr`). 토큰에는 없다                                                                                                                                                                                                                 |
| 이탤릭                                | Noto Sans Italic은 별도 파일이라 넣지 않았다. 지금은 브라우저가 기울여 그린다                                                                                                                                                                                                       |
| `OFL-1.1`을 `deny.toml`에 넣을지      | 위 "제3자 라이선스"                                                                                                                                                                                                                                                                 |
| 웹 고지와 서버 `NOTICE` 합치기        | 패키징 단계에서                                                                                                                                                                                                                                                                     |
| 번들 크기 상한                        | W2                                                                                                                                                                                                                                                                                  |

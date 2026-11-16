# fvoci 웹앱 포팅 노트

`apps/web`은 원본 웹앱(React)을 Svelte 5로 새로 짠 것이다. 이 문서는 실제로 한 것과 확인한 것을 적는다. 계획은 저장소 밖의 `~/.claude/plans/fvoci-web-svelte.md`에 있다. 글과 순서를 다듬은 때는 2026-11-11 03:07 KST다.

## 지금 상태

| 단계            | 내용                                                                                                                                                  | 상태                                                                      |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| W0              | 스택, 공용 UI, 디자인 토큰, 글꼴 도구, 라이선스 검사, 검증 명령                                                                                       | 완료 (2026-11-10 17:40 KST)                                               |
| W1              | 순수 로직 `src/lib/logic`과 원본 단위 테스트                                                                                                          | 완료 (2026-11-10 18:00 KST). 원본 결함 Q1–Q16 고침 (2026-11-10 18:55 KST) |
| 스파이크        | 편집기와 본문 저장, 드래그                                                                                                                            | 완료. 결정 반영 (2026-11-10 20:50 KST)                                    |
| 서버를 기다리며 | 서버 설계에 맞추기, 글꼴·번역 결정, 모바일 화면 안, 화면 부품                                                                                         | 완료 (2026-11-10 20:50 KST). 아래 "서버의 계약을 기다리며 한 것"          |
| 화면 조립       | 데이터의 경계와 메모리 구현, 틀·뷰 머리글·칸반·갤러리·테이블·카드 상세, 편집기의 수식·다이어그램·이모지, 어두운 화면, `.zst`, 원본 폴더에서 떼어 내기 | 완료 (2026-11-11 01:50 KST). 아래 "화면 조립"                             |
| 서버 연결       | 경계의 둘째 구현, 로그인·가입, 캘린더, 공유, 템플릿 선택기, 가져오기·내보내기, 온보딩                                                                 | 시작하지 않음                                                             |

- 서버의 계약은 2026-11-11 02:11 KST에 나왔다. 저장소 루트의 `contracts/openapi.json`이다. 아래에서 "계약이 아직 없다"고 적은 곳은 그 일을 한 때의 사정이다.
- 마지막 검증은 모두 통과했다. 결과는 "마지막 검증"에 있다.
- 다음에 할 일, 정해졌고 아직 코드에 없는 것, 도구의 위치, 화면을 띄우는 법, 걸렸던 함정은 `docs/handoff.md`에 있다.

## 읽는 법

- 경로는 `apps/web` 기준이다. 시각은 모두 KST다. 계획은 "플랜"이라고도 적었다.
- W0–W6은 계획의 단계 이름이다: W0 준비, W1 순수 로직, W2 뼈대와 얇은 종단 조각, W3 보드 페이지, W4 카드, W5 나머지 뷰와 기능, W6 모바일 마무리. S6은 같은 계획의 서버 쪽 단계다.
- 이 문서의 Q1–Q16은 원본의 결함에 붙인 번호다. 코드와 테스트의 주석이 그 번호로 부른다. `docs/mobile-proposal.md`의 질문 번호(Q1–Q36)와는 다른 것이다. 모바일 안의 질문을 가리킬 때는 "모바일 안의 Q8"처럼 적었다.
- "원본"은 이 웹앱이 옮겨 온 출발점인 옛 웹앱이다. 원본과 견준 말은 맨 끝의 두 절 "원본에서 옮겨 온 자료"와 "원본과 견준 것"에 모았다. 그 앞의 절은 원본을 몰라도 읽힌다.
- 글꼴과 압축을 말하는 곳의 "원본"은 가공하기 전의 파일을 뜻한다.
- 스스로 정한 것은 `docs/decisions.md`에, 서버에 대해 가정한 것은 `docs/contract-assumptions.md`에 있다.

## 검증 명령

워크스페이스 루트에서 실행한다. 패키지 이름이 `web`이다. `pnpm`을 놓는 법은 `docs/handoff.md`의 "도구가 있는 곳"에 있다.

```bash
pnpm install --frozen-lockfile
pnpm --filter web run check     # paraglide 컴파일 → svelte-check → eslint → prettier --check → 라이선스 검사 → 이름 검사 → 로케일 키 집합 검사
pnpm --filter web run test      # paraglide 컴파일 → vitest를 TZ=UTC, Asia/Seoul, Pacific/Kiritimati 로 세 번
pnpm --filter web run build     # paraglide 컴파일 → 글꼴 도구 → vite build
pnpm --filter web run e2e       # Playwright. 먼저 build 가 있어야 한다
pnpm --filter web run spikes:e2e
```

- `check`에 `tsc`를 따로 넣지 않았다. `svelte-check`가 `tsconfig.json`의 `include` 전체(`.ts` 포함)를 같은 설정으로 검사한다. `tools/*.ts`와 `vite.config.ts`의 타입 오류가 `svelte-check` 출력에 나오는 것을 확인했다.
- `test`는 단위 테스트를 세 시간대로 돌린다(`tools/test-zones.ts`). 하나라도 실패하면 실패다. 한 번만 돌리려면 `test:once`.
- `e2e`는 `FVOCI_E2E_URL`이 있으면 그 서버를, 없으면 `tools/preview.ts`를 띄워 대상으로 삼는다. `preview.ts`에는 API가 없다. 제품 시나리오는 브라우저 안의 메모리 구현 위에서 돈다.
- `e2e`의 프로젝트는 셋이다: `desktop`, `mobile`, `middle`(626×890, 터치). 글꼴 측정 7건은 `desktop`에서만 돈다.
- 스파이크용: `spikes`(개발 서버), `spikes:build`, `spikes:e2e`.
- 번역: `i18n:keys`(키 집합 검사).
- 보조 스크립트: `i18n`(메시지 컴파일), `fonts`(글꼴 도구), `licenses`, `names`, `lint`, `format`, `preview`(빌드 결과를 서버 규칙대로 서빙), `storybook`, `build-storybook`, `look`(앱·스파이크·Storybook을 한 포트에서 본다. `docs/handoff.md`).

## 마지막 검증

마지막 상태에서 워크스페이스 루트의 명령을 한 번씩 직접 돌렸다(2026-11-11 01:29–01:47 KST). 돌린 커밋은 옛 저장소의 해시로 `59ddf21b`이고 워킹 트리는 깨끗했다. 그 뒤의 커밋은 그림과 문서뿐이다.

| 명령                               | 결과                                                                                                                                          | 걸린 시간       |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | --------------- |
| `pnpm install --frozen-lockfile`   | 통과(잠금 파일 그대로)                                                                                                                        | 1초 미만        |
| `pnpm --filter web run check`      | 통과: svelte-check 3,978 파일 오류 0·경고 0, eslint, prettier, 라이선스 검사, 이름 검사 1,093 파일, 메시지 파일 최신(18 로케일), 키 집합 검사 | 재지 않음(주 1) |
| `pnpm --filter web run test`       | 통과: UTC, Asia/Seoul, Pacific/Kiritimati 각각 95 파일 2,414건                                                                                | 약 66초(주 2)   |
| `pnpm --filter web run build`      | 통과: 고지 91 패키지·글꼴 7, `.br`/`.zst`/`.gz` 345개, 인라인 스크립트 해시 1                                                                 | 재지 않음(주 3) |
| `pnpm --filter web run e2e`        | 통과: 711건 가운데 532건 통과, 179건 건너뜀, 실패 0                                                                                           | 2.6분           |
| `pnpm --filter web run spikes:e2e` | 통과: 394건 통과, 208건 건너뜀(01:37–01:42 KST)                                                                                               | 4.5분           |

1. 테스트가 더 적던 2026-11-10 22:02 KST에는 약 65초였다.
2. vitest가 적은 시간은 시간대마다 17.65초, 18.23초, 29.95초다.
3. 글꼴을 건너뛸 때 Vite의 빌드는 6초쯤이다. 글꼴을 다시 만들면 약 1분 40초가 더 든다. `build-storybook`은 약 10초였다(2026-11-10 22:02 KST).

- `e2e`가 건너뛴 179건은 다른 크기의 몫인 테스트와, `FVOCI_SCREENS`가 있어야 도는 그림이다.
- 고친 뒤 전체를 두 번씩 돌려(`--repeat-each 2`) 1,064건이 통과하는 것을 보고 위의 마지막 실행을 했다. 그 앞의 실행에서 실패한 한 건은 "합친 뒤에 드러나 고친 것"의 방문 기록이다.
  - e2e는 532건 통과·179건 건너뜀으로 원본이 있을 때와 같다.
- 확인하지 못한 것과 가끔 실패하는 테스트는 `docs/handoff.md`의 "가끔 실패하는 것, 확인하지 못한 것"에 있다. 종단 테스트는 Chromium의 세 크기다.

## 프로젝트 구조

```
apps/web/
  index.html               서버가 {{.BaseURL}} 을 채우는 뼈대
  src/app/                 진입점, 주소, 보드 페이지, 어느 구현을 쓸지 고르는 자리
  src/lib/logic/           순수 로직
  src/lib/state/           데이터의 경계(Backend), 메모리 구현, 저장소
  src/lib/layout/          화면 크기의 세 단계, 안전 영역, 뒤로 가기
  src/lib/drag/            드래그의 기반
  src/lib/editor/          본문 문서: 허용 목록, 표시기, 편집기
  src/lib/ui/              shadcn-svelte 생성물 (README 에 수정 기록)
  src/lib/components/      직접 쓴 공용 부품
  src/lib/features/        화면: shell, view-header, views, card
  src/lib/i18n/paraglide/  메시지 컴파일 결과 (생성물, git 무시)
  src/lib/fonts.ts         @font-face 규칙의 지연 로드
  src/lib/theme.svelte.ts  밝은·어두운 화면 상태
  src/styles/app.css       디자인 토큰
  src/styles/fonts/        글꼴 도구의 산출물 (git 무시)
  messages/                메시지 18 로케일 (손으로 고치는 원천)
  tests/                   vitest: logic, state, editor, tokens, tools (node), ui (jsdom)
  e2e/                     Playwright (제품)
  spikes/                  스파이크 화면과 그 Playwright
  stories/                 Storybook
  tools/                   build, emoji, fonts, i18n, licenses, lint, math, names, preview, serve
  docs/                    문서와 그림
```

루트에 추가한 파일은 셋이다: `package.json`(`packageManager` 고정), `pnpm-workspace.yaml`, `pnpm-lock.yaml`. 워크스페이스 구성원은 `apps/*`뿐이다. `apps/web/.gitignore`를 따로 뒀다.

## 구조 규칙

**지금의 규칙은 `src/lib/README.md`의 표다.** `src/lib` 바로 아래의 폴더가 층이고(0 `logic`·`i18n`·`hooks`·`utils`·`theme`, 1 `regional`·`display`·`fonts`·`layout`·`drag`·`editor`, 2 `state`·`ui`, 3 `components`, 4 `features`), 파일은 자기 층과 그보다 낮은 층에서만 가져온다(`fvoci/layer-order`). 공용 부품(`components`)은 `state`를 가져오지 못하고, 화면은 서버를 직접 부르지 못한다(`fvoci/no-direct-server`). 속성 편집기는 `features/properties`에서 `components/properties`로 내렸다. 아래 문단은 처음 세운 규칙의 기록이고 지금도 그대로 검사된다.

`src/lib/features/<기능>/` 폴더끼리는 서로 가져오지 못한다. 공유할 것은 `lib/logic`, `lib/state`, `lib/api`, `lib/ui`로 내린다. 반대로 그 아래 층은 기능 폴더에서 가져오지 못한다. ESLint 규칙 `fvoci/feature-boundaries`(`tools/lint/layers.ts`, 직접 쓴 규칙이라 의존성이 늘지 않았다)가 검사하고 `check`에 들어 있다. `$lib/features/x`와 `../x` 두 가지 쓰기를 모두 잡는다. 규칙 자체의 테스트는 `tests/tools/layers.test.ts`(5건). 설명은 `src/lib/features/README.md`.

## 고정한 판

2026-11-10 16:50 KST에 확인했고, 2026-11-16 10:44 KST에 패치·마이너 판을 다시 올렸다.

출처는 npm 레지스트리다. 패키지마다 `npm view <이름> version`(dist-tag `latest`)과 `npm view <이름> "time[<판>]"`(공개 시각)을 조회했고, 호환 범위는 `npm view <이름> peerDependencies`로 확인했다. 기억으로 정한 판은 없다.

기준: `latest`를 쓴다. 다만 공개된 지 24시간이 안 된 판은 직전 판으로 내린다. pnpm의 `minimumReleaseAge` 기본값이 1440분(1일)이고(출처: pnpm 문서 settings/dependency-resolution), 이것을 `pnpm-workspace.yaml`에 명시해 조건에 안 맞으면 설치가 실패하게 했다.

| 패키지                                               | 고정 판 | 공개 (UTC) | 메모                                                                                                                             |
| ---------------------------------------------------- | ------- | ---------- | -------------------------------------------------------------------------------------------------------------------------------- |
| pnpm                                                 | 12.21.0 | 2026-11-13 | `packageManager`에 sha512와 함께 적었다                                                                                          |
| svelte                                               | 5.57.3  | 2026-10-26 |                                                                                                                                  |
| vite                                                 | 8.3.8   | 2026-11-11 |                                                                                                                                  |
| @sveltejs/vite-plugin-svelte                         | 7.3.1   | 2026-09-23 |                                                                                                                                  |
| typescript                                           | 6.0.3   | 2026-04-16 | `latest`는 7.0.2. `svelte-check` 4.7.8의 peer가 `^5 \|\| ^6`, `typescript-eslint` 8.72의 peer가 `<6.1.0`이라 6.x의 최신으로 뒀다 |
| tailwindcss, @tailwindcss/vite                       | 4.3.5   | 2026-11-06 |                                                                                                                                  |
| bits-ui                                              | 2.21.0  | 2026-11-11 |                                                                                                                                  |
| shadcn-svelte (CLI, CSS)                             | 1.8.1   | 2026-11-11 | `shadcn-svelte/tailwind.css`를 가져오므로 의존성이기도 하다                                                                      |
| cn                                                   | 0.5.2   | 2026-11-02 | 생성된 `utils.ts`가 쓴다                                                                                                         |
| tailwind-variants                                    | 4.0.2   | 2026-11-10 | 4.0에서 `tailwind-merge` 대신 미리 엮은 병합 표를 쓴다. `src/lib/ui`의 `tv()`가 내는 클래스는 3.3.1과 같다                       |
| tw-animate-css                                       | 1.4.0   | 2025-09-24 |                                                                                                                                  |
| @lucide/svelte                                       | 1.73.0  | 2026-11-14 |                                                                                                                                  |
| svelte-sonner                                        | 1.2.3   | 2026-11-02 |                                                                                                                                  |
| @internationalized/date                              | 3.12.4  | 2026-09-01 | bits-ui의 peer, 달력 컴포넌트가 쓴다                                                                                             |
| @inlang/paraglide-js                                 | 3.2.2   | 2026-11-10 |                                                                                                                                  |
| @inlang/plugin-message-format                        | 4.6.0   | 2026-11-03 | `project.inlang/settings.json`이 CDN 주소가 아니라 `node_modules`의 파일을 가리킨다                                              |
| vitest                                               | 5.1.0   | 2026-11-02 |                                                                                                                                  |
| jsdom                                                | 31.0.1  | 2026-11-13 |                                                                                                                                  |
| @testing-library/svelte                              | 5.4.2   | 2026-06-23 |                                                                                                                                  |
| @testing-library/jest-dom                            | 7.0.1   | 2026-08-09 |                                                                                                                                  |
| @playwright/test                                     | 1.65.1  | 2026-11-13 | Chromium 빌드 1255                                                                                                               |
| storybook, @storybook/svelte, @storybook/svelte-vite | 10.6.7  | 2026-11-10 |                                                                                                                                  |
| svelte-check                                         | 4.7.8   | 2026-11-01 |                                                                                                                                  |
| eslint                                               | 10.15.0 | 2026-11-13 |                                                                                                                                  |
| @eslint/js                                           | 10.0.1  | 2026-02-06 |                                                                                                                                  |
| typescript-eslint                                    | 8.72.2  | 2026-11-09 |                                                                                                                                  |
| eslint-plugin-svelte                                 | 3.24.1  | 2026-11-07 |                                                                                                                                  |
| eslint-config-prettier                               | 10.1.8  | 2025-07-18 |                                                                                                                                  |
| globals                                              | 17.14.0 | 2026-11-01 |                                                                                                                                  |
| prettier                                             | 3.10.1  | 2026-11-12 |                                                                                                                                  |
| prettier-plugin-svelte                               | 4.1.1   | 2026-06-15 |                                                                                                                                  |
| prettier-plugin-tailwindcss                          | 0.8.1   | 2026-07-15 |                                                                                                                                  |
| @types/node                                          | 26.6.6  | 2026-10-28 | Node 26.13.0에서 작업했다                                                                                                        |
| subset-font                                          | 2.9.0   | 2026-09-20 | 글꼴 도구. HarfBuzz의 WebAssembly 빌드를 쓴다                                                                                    |
| @tiptap/core, @tiptap/pm, 확장 15개                  | 3.34.0  | 2026-11-13 | 편집기 스파이크(2026-11-10 18:26 KST 확인). 묶음(`starter-kit`)은 쓰지 않는다                                                    |
| marked                                               | 18.2.0  | 2026-11-09 | 마크다운 가져오기                                                                                                                |
| fast-check                                           | 4.10.2  | 2026-09-19 | 속성 기반 테스트(2026-11-10 18:45 KST 확인)                                                                                      |

Tiptap은 편집기 스파이크에서 넣었다(`docs/spike-editor.md`).

글꼴 원본과 유니코드 자료(2026-11-10 17:07 KST 확인, `tools/fonts/sources.json`에 주소와 SHA-256):

| 자료                                          | 판                                                         | 출처                                           |
| --------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------- |
| Noto Sans, Noto Sans KR·JP·SC·TC (가변, wght) | `google/fonts` 커밋 `5bb28733` (2026-11-08)                | GitHub API로 커밋과 파일 크기 확인             |
| Noto Serif, Noto Serif KR·JP·SC·TC            | 같은 커밋                                                  | 도구가 지원만 한다                             |
| D2Coding Regular·Bold                         | 1.4.0 (`naver/d2-coding-font` 태그 `VER1.4.0`, 2026-10-03) | GitHub 릴리스 목록. 1.4.0에도 가변 글꼴은 없다 |
| Unihan                                        | 유니코드 18.0.0                                            | `unicode.org/Public/UCD/latest`의 ReadMe       |

## 빌드 산출물

`dist/index.html` + `dist/static/`이다. 서버의 `webpath`를 `dist`로 두면 바로 서빙된다.

- **파일명의 해시는 16자리 16진수다.** 서버(`crates/server/src/web.rs`의 `has_content_hash`)는 이름을 `-`, `.`, `_`로 나눈 조각 가운데 "8자 이상의 16진수이고 숫자가 하나 이상"인 것이 있어야 내용 해시로 보고 `immutable`을 붙인다. Vite의 기본 해시는 base64 계열이라 이 조건에 안 맞는다. `hashCharacters: 'hex'`로 바꾸고, `static/` 아래에 조건을 못 맞춘 파일이 있으면 빌드가 실패하게 했다(`tools/build/output.ts`).
- **base path를 빌드에 넣지 않는다.** `index.html`은 `{{.BaseURL}}/static/…`으로, 스크립트와 스타일시트끼리는 상대 경로로 참조한다(`experimental.renderBuiltUrl`). `/boards` 아래의 깊은 주소에서 파일이 모두 풀리는 것을 확인했다.
- **사전 압축.** `static/` 아래의 텍스트 파일마다 `.br`(brotli 11), `.zst`(zstd, 2026-11-10에 더함. `docs/build-notes.md`)와 `.gz`(gzip 9)를 만든다. woff2는 이미 압축돼 있어 뺀다. `index.html`은 서버가 요청마다 치환해 보내므로 압축하지 않는다.
- **고지.** `static/THIRD_PARTY_NOTICES.txt`. 이름에 해시가 없어 서버가 `no-cache`로 보낸다(의도).

서버 쪽에서 빌드한 실제 Rust 서버에 `webpath`를 이 `dist`로 주고 확인한 것(2026-11-10 17:24 KST, 서버는 읽기만 했다): `{{.BaseURL}}` 치환, 해시 파일의 `Cache-Control: public, max-age=31536000, immutable`, `Accept-Encoding`에 따른 `br`·`gzip`·무압축 응답과 `Vary`, 고지 파일의 `no-cache`, woff2의 `font/woff2`. `FVOCI_E2E_URL`로 그 서버를 대상으로 한 Playwright 6건도 통과했다.

글꼴 규칙 CSS의 크기(2026-11-10 17:33 KST, 바이트. 한국어의 것은 그 뒤 파일이 셋이 되어 압축 전 151,944바이트가 됐다):

| 파일                       | 원본    | br     | gzip   |
| -------------------------- | ------- | ------ | ------ |
| `faces-noto-sans-kr-*.css` | 87,765  | 9,306  | 35,646 |
| `faces-noto-sans-jp-*.css` | 131,281 | 13,860 | 48,756 |
| `faces-noto-sans-sc-*.css` | 103,587 | 11,195 | 22,367 |
| `faces-noto-sans-tc-*.css` | 187,395 | 19,362 | 63,474 |
| `faces-d2coding-*.css`     | 59,847  | 6,369  | 12,244 |

글꼴 규칙 CSS는 같은 `unicode-range` 목록이 굵기 구간마다 되풀이되어 brotli에서 크게 줄고 gzip에서는 덜 줄어든다(gzip의 창이 32KB라 되풀이를 못 본다).

주 스타일시트에는 Tailwind가 `src/lib/ui`의 컴포넌트가 쓰는 클래스를 모두 낸다. 화면이 쓰는 것만 남기는 것이 아니라 소스에 나오는 것을 낸다. 스크립트와 스타일의 지금 크기는 아래 "번들 크기"에 있다.

### 번들 크기 (조립된 앱, 2026-11-11 01:36 KST)

브라우저가 실제로 받는 파일을 단계마다 세었다. 조건은 1280×720, 영어 화면, 첫 보드는 칸반이다. 측정 스크립트 `target/scratch/measure-sizes.mjs`가 띄운 서버에서 요청을 모으고 `dist`의 파일 크기를 더한다. 그 스크립트는 커밋되지 않는다(`docs/handoff.md`의 "추적되지 않는 것"). 글꼴은 이미 압축돼 있어 그대로 센다. 단위는 바이트다.

| 단계                                               | 파일 | 원본      | gzip      | brotli    | zstd      |
| -------------------------------------------------- | ---- | --------- | --------- | --------- | --------- |
| **첫 화면**(보드, 칸반)                            | 25   | 5,224,878 | 3,833,326 | 3,634,722 | 3,663,122 |
| 그 가운데 스크립트                                 | 16   | 1,143,815 | 308,933   | 243,828   | 262,828   |
| 그 가운데 스타일                                   | 6    | 775,679   | 219,009   | 85,510    | 94,910    |
| 그 가운데 글꼴                                     | 3    | 3,305,384 | —         | —         | —         |
| **카드를 열 때** 더 받는 것(`BodyEditor-*.js`)     | 1    | 413,119   | 130,640   | 110,727   | 116,364   |
| **수식**이 본문에 있을 때 더 받는 것               | 6    | 354,470   | 152,339   | 138,484   | 143,980   |
| 그 가운데 `katex-*.js`                             | 1    | 259,382   | 76,680    | 63,522    | —         |
| 그 가운데 `katex-*.css`                            | 1    | 22,497    | 3,489     | 2,882     | —         |
| 그 가운데 글꼴(수식 둘, 고정폭 하나)               | 3    | 71,424    | —         | —         | —         |
| **다이어그램**(순서도)이 본문에 있을 때 더 받는 것 | 27   | 827,581   | 222,834   | 192,683   | 207,142   |
| 그 가운데 가장 큰 조각                             | 1    | 241,442   | 37,250    | 30,085    | —         |

- 첫 화면의 스크립트는 거의 `index-*.js` 하나다(985,075 / gzip 273,838 / brotli 213,795). 화면 여섯과 공용 부품, 저장소, 메시지가 한 덩어리로 들어 있다.
- 나머지 15개(합쳐 158,740바이트)는 메모리 구현의 조각 둘(구현, 씨앗)과 그것이 읽는 내장 템플릿 13개다. **이 15개는 서버를 부르는 구현이 들어오면 받지 않게 된다.**
- 화면별로 쪼개는 일과 상한을 정하는 일은 하지 않았다.
- 첫 화면의 글꼴 3.3 MB 가운데 2.67 MB는 `noto-sans-kr-rest`다. 본보기 구성원의 이름에 든 한자(陳美玲)가 "자주 쓰는 글자" 묶음 밖이라 나머지 묶음을 통째로 부른다. 영어 화면에 한글·한자 이름이 없으면 `noto-sans-common` 112,876바이트만 받는다.
- 편집기(`BodyEditor`)는 카드를 처음 열 때 받는다. 수식과 다이어그램은 본문에 그것이 있을 때만 받는다(읽기 전용 표시기도 같은 조각을 쓴다). 다이어그램은 종류마다 조각이 따로라 순서도가 아니면 수치가 다르다.

## 디자인 토큰 (`src/styles/app.css`)

- **색.** 밝은 화면 `:root`, 어두운 화면 `.dark`. 출발점은 생성기의 `neutral` 팔레트이고 다음을 바꿨다.
  - `--input-border`(새 토큰): 입력 칸의 테두리. 생성기의 `--input`은 바탕과 1.2:1이라 테두리로 쓸 수 없다.
  - `--ring`, `--sidebar-ring`: 초점 표시. 회색(2.6:1)에서 파란색으로(밝은 화면 5.0:1, 어두운 화면 7.4:1).
  - `--muted-foreground`: 밝은 화면에서 `--muted` 위 4.3:1이던 것을 5.1:1로.
  - `--destructive`: 삭제 버튼은 같은 색의 옅은 바탕 위에 같은 색 글자다(`bg-destructive/10`, 호버 `/20`). 호버 상태에서 3.8:1이던 것을 4.6:1로(대비 테스트가 잡았다).
  - 브랜드 색은 정해진 것이 없어 무채색 그대로다.
- **글자 단계.** 모두 rem이다. `--text-sm`이 UI 기본이고 0.9375rem(15px)이다. `--text-base`는 1rem으로, 본문과 좁은 화면의 입력 칸에 쓴다. 뷰포트 메타에서 `maximum-scale`과 `user-scalable`을 뺐다.
- **줄 높이.** UI 1.45, 본문 1.6. `:lang(ko)`, `:lang(ja)`, `:lang(zh)`에서는 1.55, 1.75. `lang`이 다른 하위 트리에도 적용되도록 `@theme inline`으로 변수 자체를 참조하게 했다.
- **글꼴 스택.** 언어마다 스택 전체를 정한다(`--font-sans-stack`). 한국어는 `'Noto Sans KR'`, 일본어는 `'Noto Sans JP'`가 맨 앞이고 그다음이 `'Noto Sans'`, 나머지 CJK family다. 중국어 두 로케일과 그 밖의 언어는 `'Noto Sans'`가 맨 앞이고 CJK 네 family가 현재 언어의 것부터 뒤따른다. 끝은 시스템 글꼴. 고정폭은 `'D2Coding'`. (2026-11-10 20:02 KST에 바꿈. 아래 "글꼴")
- **간격·모서리.** `--spacing` 0.25rem, `--spacing-touch` 2.75rem(손가락으로 누르는 대상의 최소 변), `--radius` 0.5rem.
- **초점.** 자체 표시가 없는 요소는 `--ring` 색의 2px 윤곽선. 생성 컴포넌트는 같은 색의 테두리와 링을 쓴다.
- **대비 검사.** `tests/tokens/contrast.test.ts`가 `app.css`를 직접 읽어 두 화면 각각 글자 15쌍(4.5:1), 입력 테두리·초점 11쌍(3:1), 삭제 버튼의 옅은 바탕을 검사한다. 반투명 색은 브라우저처럼 감마 인코딩된 sRGB에서 합성해 계산한다.

## 글꼴 (`tools/fonts`)

`pnpm --filter web run fonts`(= `build`의 한 단계)가 원본을 내려받아(SHA-256 검사, `target/web-fonts/src`에 캐시) family마다 "상용" 파일 하나와 "잔여" 파일 하나를 WOFF2로 만들고 `@font-face` 규칙을 쓴다. **한국어만 파일이 셋이다**(상용, 나머지 음절, 잔여). 산출물(`src/styles/fonts/`)은 커밋하지 않는다. 입력이 같으면 건너뛴다. 전체 빌드는 이 기기에서 1분 40초이고, 건너뛸 때의 `build`는 2초다.

사용자 결정(2026-11-10)으로 바꾼 것 둘(2026-11-10 20:02 KST 적용):

1. **한국어는 세 파일**(안 A′): 상용 파일에는 KS X 1001의 한글 2,350자만 넣고, 나머지 현대 음절 8,822자는 따로 `noto-sans-kr-syllables.woff2`로 뗀다. 드문 음절(`똠`, `쌰`, `뷁`)이 나오면 이 0.78 MB만 받고 잔여(2.67 MB)는 받지 않는다. 일본어·중국어의 글자 집합은 그대로다.
2. **한국어·일본어 로케일은 자기 CJK 글꼴이 스택 맨 앞이다.** 영문·숫자·문장부호도 그 글꼴의 것으로 그린다(말줄임표가 가운데 높이에 찍힌다). 그래서 KR·JP의 상용 파일에 기본 라틴, 라틴-1, 일반 문장부호를 넣었다. 중국어 두 로케일은 전처럼 라틴 글꼴이 앞이다(중국어 글꼴의 전각 따옴표가 영문 `it’s`를 벌린다).

글자 집합 정의(커밋됨):

| family       | 상용 파일에 들어가는 것                                                                                            | 정의                                                                       |
| ------------ | ------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------- |
| Noto Sans    | 기본 라틴, 라틴-1, 라틴 확장-A, 결합 부호, 일반 문장부호, 통화·글자꼴 기호 등                                      | `charsets.ts`의 블록                                                       |
| Noto Sans KR | KS X 1001의 한글 2,350자, 호환 자모, CJK 문장부호, 전각 기호, KS X 1001의 기호, **기본 라틴·라틴-1·일반 문장부호** | `charsets/ks-x-1001-hangul.txt` (2,350자), `ks-x-1001-symbols.txt` (986자) |
| Noto Sans JP | 히라가나, 가타카나, 상용한자 2,136자, JIS X 0208의 기호, **기본 라틴·라틴-1·일반 문장부호**                        | `charsets/joyo.txt`, `jis-x-0208-symbols.txt` (524자)                      |
| Noto Sans SC | 통용규범한자표 1급 3,500자, GB 2312의 기호                                                                         | `charsets/tgh-level1.txt`, `gb-2312-symbols.txt` (728자)                   |
| Noto Sans TC | Big5 1수준 5,401자, 주음부호, Big5의 기호                                                                          | `charsets/big5-level1.txt`, `big5-symbols.txt` (409자)                     |
| D2Coding     | 기본 라틴, 라틴-1, 문장부호, 화살표, 수학 기호, 상자 그리기, 도형                                                  | `charsets.ts`의 블록                                                       |

Noto Sans KR의 "나머지 음절" 파일은 현대 한글 음절 블록(U+AC00–D7A3)에서 상용 파일에 들어간 것을 뺀 8,822자다(`families.ts`의 `more`).

한자 목록은 `derive-charsets.ts`가 Unihan 18.0.0의 `kJoyoKanji`, `kTGH`와 Big5 바이트 범위에서, 기호 목록과 KS X 1001의 한글 목록은 각 국가 문자 집합의 해당 영역을 플랫폼 디코더로 풀어서 만든다.

**라틴 글꼴이 가진 글자를 어디서 빼는가**(`families.ts`의 `partition`, 테스트는 `tests/tools/fonts.test.ts`의 "the files of a family" 4건):

- 스택에서 라틴 family 뒤에 오는 family(SC, TC)는 Noto Sans(라틴)가 가진 글자를 모든 파일에서 뺀다. 그 글자는 CJK family에 오지 않기 때문이다. 전과 같고, SC·TC의 파일은 바이트까지 전과 같다.
- 자기 언어에서 스택 맨 앞에 서는 family(KR, JP: `leads`)의 **상용 파일**은 정의가 이름 붙인 글자를 라틴 family가 가졌더라도 그대로 갖는다. 한국어 화면에서는 KR 글꼴이 먼저 물어지므로, 거기 없는 글자만 Noto Sans로 넘어간다. KS X 1001과 JIS X 0208의 기호에 든 그리스·키릴 글자, 화살표, 단위 기호도 이 규칙으로 KR·JP의 상용 파일에 들어갔다(측정 때의 가정보다 글자가 353자 많다: 3,499 → 3,852). 나머지 음절 파일과 잔여 파일에서는 전처럼 뺀다(잔여 파일의 글자 수와 바이트가 전과 같다).
- 영어 화면에서는 Noto Sans가 앞이므로 KR·JP 상용 파일의 라틴 글자는 쓰이지 않는다.

파일 크기(2026-11-10 20:00 KST):

| 파일                            | 글자 수 | 바이트     |
| ------------------------------- | ------- | ---------- |
| `noto-sans-common.woff2`        | 779     | 112,876    |
| `noto-sans-rest.woff2`          | 2,315   | 426,924    |
| `noto-sans-kr-common.woff2`     | 3,852   | 517,728    |
| `noto-sans-kr-syllables.woff2`  | 8,822   | 775,336    |
| `noto-sans-kr-rest.woff2`       | 10,324  | 2,674,780  |
| `noto-sans-jp-common.woff2`     | 3,060   | 843,688    |
| `noto-sans-jp-rest.woff2`       | 13,484  | 3,534,452  |
| `noto-sans-sc-common.woff2`     | 4,228   | 1,143,144  |
| `noto-sans-sc-rest.woff2`       | 26,133  | 6,842,216  |
| `noto-sans-tc-common.woff2`     | 5,932   | 1,693,848  |
| `noto-sans-tc-rest.woff2`       | 14,284  | 4,017,412  |
| `d2coding-regular-common.woff2` | 809     | 28,712     |
| `d2coding-regular-rest.woff2`   | 19,127  | 1,443,768  |
| `d2coding-bold-common.woff2`    | 809     | 29,456     |
| `d2coding-bold-rest.woff2`      | 19,127  | 1,538,192  |
| 합계                            |         | 25,622,532 |

Noto 계열은 가변 글꼴(wght 축)이라 굵기별 파일이 없다. Noto Sans 원본에는 wdth 축도 있어 100으로 고정했다. D2Coding은 가변 빌드가 없어 Regular와 Bold가 따로다. KR의 규칙 CSS는 파일이 셋이 되어 87,765 → 151,944바이트(압축 전)로 늘었다.

**첫 화면의 글꼴 요청 수와 바이트**(Chromium headless, 기기에 Noto 없음, 2026-11-10 20:02 KST). `e2e/fonts.spec.ts`가 빌드 결과(`dist/`)의 스타일시트만으로 그린, 그 언어의 평범한 글 한 문단(글자·숫자·날짜·말줄임표·따옴표·화살표, 보통과 굵은 글씨)이 받는 글꼴 파일을 세고 고정한다. `pnpm --filter web run e2e`가 수치를 찍는다.

| 로케일                             | 요청 | 바이트    | 파일                                            |
| ---------------------------------- | ---- | --------- | ----------------------------------------------- |
| 영어                               | 1    | 112,876   | `noto-sans-common`                              |
| 한국어                             | 1    | 517,728   | `noto-sans-kr-common`                           |
| 한국어 + KS X 1001 밖의 음절(`똠`) | 2    | 1,293,064 | `noto-sans-kr-common`, `noto-sans-kr-syllables` |
| 일본어                             | 1    | 843,688   | `noto-sans-jp-common`                           |
| 중국어 간체                        | 2    | 1,256,020 | `noto-sans-common`, `noto-sans-sc-common`       |
| 중국어 번체                        | 2    | 1,806,724 | `noto-sans-common`, `noto-sans-tc-common`       |

한글 주석이 든 코드 블록이 있으면 D2Coding 2건이 더해진다(글꼴을 바꾼 뒤에 다시 재지는 않았다).

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

## 글꼴 측정: 두 가지 대안 (2026-11-10 19:00 KST의 측정, 결정 전의 기록)

**결정이 났고 적용했다**(2026-11-10 20:02 KST): 한국어는 안 A′, 한국어·일본어는 CJK 글꼴 먼저, 일본어·중국어의 글자 집합과 중국어의 스택은 그대로. 지금의 설정과 수치는 위 "글꼴" 절에 있다. 아래는 결정 전에 사용자가 물은 두 가지를 재 본 수치와 권고를 그대로 남긴 것이다(이 절의 "지금"은 결정 전의 설정을 가리킨다). 측정은 일회용 스크립트로 했고(같은 부분집합 도구, 같은 원본) 산출물은 `target/font-lab/`에 있다(커밋하지 않음).

### 1. 상용 글자 집합을 줄이면

| 파일                                                    | 글자 수 | 바이트      |
| ------------------------------------------------------- | ------- | ----------- |
| KR 지금: 상용(현대 한글 11,172자 + 자모 + 기호)         | 12,321  | 1,225,672   |
| KR 지금: 잔여                                           | 10,324  | 2,674,780   |
| **KR 안 A: 상용(KS X 1001 한글 2,350자 + 자모 + 기호)** | 3,499   | **493,052** |
| KR 안 A: 잔여(나머지 음절 8,822자 + 한자 등)            | 19,146  | 3,444,908   |
| KR 안 A′: 나머지 음절 8,822자만 따로(세 번째 파일)      | 8,822   | 775,336     |
| KR 안 A′: 잔여(한글 음절 없음)                          | 10,324  | 2,674,780   |
| JP 지금: 상용(가나 + 상용한자 2,136 + 기호)             | 2,719   | 819,520     |
| JP: 가나 + 기호만(한자 없음)                            | 583     | 209,432     |
| JP: 가나 + 기호 + 상용한자의 절반 1,068자               | 1,651   | 522,400     |
| SC 지금: 상용(통용규범한자표 1급 3,500 + 기호)          | 4,228   | 1,143,144   |
| SC: 1급 가운데 2,500자 + 기호(크기 가늠용)              | 3,228   | 813,980     |
| TC 지금: 상용(Big5 1수준 5,401 + 주음부호 + 기호)       | 5,932   | 1,693,848   |

- **한국어**: 상용 파일이 1.23 MB에서 0.49 MB로 준다(−60%). 대신 KS X 1001에 없는 음절(이름, 외래어, 줄임말에 나오는 `똠`, `쌰`, `뷁` 같은 글자)이 하나라도 화면에 나오면 잔여 파일을 받는다. 두 파일 원칙을 지키면 그 잔여가 3.44 MB(지금보다 0.77 MB 큼)이고, 나머지 음절을 세 번째 파일로 떼면 0.78 MB만 받는다.
- **일본어·중국어**: 한자 한 글자가 약 0.3 KB라 줄인 만큼만 준다. 일본어는 상용한자가 이미 가장 작은 공인 목록이다(그 아래는 교육한자 1,026자인데 Unihan에 목록이 없어 절반을 뽑아 가늠했다: 0.52 MB). 간체는 2,500자로 줄이면 0.81 MB(−29%)인데, 그 기준이 될 "상용자 2,500" 목록도 Unihan에 없다(순서대로 앞 2,500자를 넣어 크기만 쟀다). 번체는 Unihan 18.0에 빈도 속성(`kFrequency`)이 없어 줄인 집합을 만들지 못했다(재지 못함).

**권고**: 한국어는 안 A′(상용 0.49 MB + 나머지 음절 0.78 MB + 잔여 2.67 MB). 첫 화면이 0.73 MB 가벼워지고, 드문 음절이 나와도 0.78 MB로 끝난다. "언어당 상용 하나, 잔여 하나" 원칙에서 한국어만 파일이 셋이 되므로 확인이 필요하다. 두 파일을 지켜야 하면 안 A(0.49 MB + 3.44 MB)도 지금보다 낫다고 본다: KS X 1001의 2,350자는 일상 한국어의 거의 전부를 덮는다(덮는 비율을 직접 재지는 않았다). 일본어·중국어는 그대로 둔다. 표준 목록으로 줄일 여지가 작고, 줄이면 잔여 파일(3.5–6.8 MB)을 받게 되는 화면이 늘어난다.

### 2. CJK 로케일에서 CJK 글꼴을 스택 맨 앞에 두면

지금은 라틴 글꼴(Noto Sans)이 앞이고 CJK 파일에서는 라틴 글꼴이 가진 글자를 뺐다. 대안은 한국어 화면이면 Noto Sans KR을 맨 앞에 두고 라틴 글자·숫자·문장부호도 그 글꼴의 것을 쓰는 것이다(상용 파일에 기본 라틴과 라틴-1, 문장부호를 넣는다).

첫 화면의 글꼴 요청(Chromium, 한 줄에 한글·영문·숫자·말줄임표, 보통과 굵은 글씨):

| 구성                                     | 요청                                          | 바이트    |
| ---------------------------------------- | --------------------------------------------- | --------- |
| 지금(라틴 먼저)                          | 2 (`noto-sans-common`, `noto-sans-kr-common`) | 1,338,548 |
| KR 먼저(음절 전부 + 라틴)                | 1                                             | 1,249,052 |
| KR 먼저 + 안 A(KS X 1001 2,350자 + 라틴) | 1                                             | 517,344   |

- KR 글꼴에 라틴을 넣는 비용은 23 KB다(1,225,672 → 1,249,052). JP는 +23 KB(842,884), SC +26 KB(1,169,244), TC +16 KB(1,710,324).
- **말줄임표**(100px로 그려 잰 세로 위치, 기준선이 0): Noto Sans는 −2에서 13(기준선에 붙고 폭 0.79em), CJK 네 글꼴은 모두 31에서 45(가운데 높이, 폭 1em). 다만 CJK 글꼴은 앞 글자의 문자에 따라 모양을 바꾼다: 한글·가나·한자 뒤의 `…`는 가운데에, 영문 뒤의 `…`는 기준선에(점 사이가 넓게) 찍힌다(`target/font-lab/latin-vs-cjk-fonts.png`). 지금 구성에서는 한국어·일본어 문장 속의 `…`도 Noto Sans의 것이라 늘 아래에 붙는다.
- **라틴 글자의 모양**: CJK 글꼴의 라틴은 Noto Sans와 다른 설계다(네 CJK 글꼴은 서로 같다). 같은 문장의 폭이 2.2% 좁고(100px에서 1,420 대 1,389), 숫자 열 줄의 폭은 570 대 560, x높이 54 대 55, 대문자 높이 72 대 74. 획 끝과 `g`, `t`의 모양이 다르다. 영어 화면(Noto Sans)과 한국어 화면(KR의 라틴)에서 같은 영문이 다르게 보이게 된다.
- **중국어의 따옴표**: SC와 TC는 `“ ” ’`가 전각이다. 영문 `it’s`가 `it’ s`처럼 벌어진다. KR과 JP는 반각이다.
- **한자 자형 순서**: 지금도 `:lang()`으로 맞춘다(`ja`면 JP 먼저, `zh-Hant`면 TC 먼저). `html`의 `lang`을 로케일에 맞추는 일은 W2의 몫이고, 맞췄을 때 일본어 화면이 JP 상용 파일만, 번체 화면이 TC 상용 파일만 받는 것을 확인했다(W0).

**권고**: 한국어와 일본어는 CJK 글꼴을 앞에 둔다. 요청이 하나 줄고, 문장부호가 그 언어의 관습대로 나오며(말줄임표가 가운데), 안 A와 합치면 한국어 첫 화면의 글꼴이 1.34 MB에서 0.52 MB가 된다. 치르는 값은 영문·숫자가 Noto Sans가 아니라 CJK 글꼴의 라틴으로 보인다는 것이다. 중국어 두 로케일은 지금처럼 라틴을 앞에 둔다(전각 따옴표 때문에). 그 밖의 로케일은 CJK 글꼴이 필요할 때만 받으므로 지금 그대로다. 스택은 `:lang()`별 변수로 이미 나뉘어 있어 바꾸는 곳은 `app.css`의 변수와 글꼴 도구의 family 정의다.

확인하지 못한 것: 실제 한국어 글에서 KS X 1001 밖의 음절이 나오는 빈도, 굵은 글씨와 기울임에서의 라틴 모양 차이, Chromium 밖의 브라우저.

## 제3자 라이선스 (`tools/licenses`)

허용 목록은 따로 두지 않고 루트 `deny.toml`의 `[licenses] allow`를 읽는다(`policy.ts`). 검사는 둘이다.

1. **설치된 전체**(`pnpm --filter web run licenses`, `check`에 포함): `pnpm licenses list --json`의 모든 패키지. 허용 목록에 없으면 `exceptions.json`의 `tooling`에 이름·라이선스·사유가 있어야 한다. 맞는 패키지가 없는 예외는 오류다.
2. **번들에 들어간 것**(빌드 때, `notices.ts`): Vite의 라이선스 보고(`build.license`)에 나온 패키지 + `app.css`가 가져오는 CSS 패키지 + 도구가 소스에 써 넣은 코드의 출처(`vendored`: shadcn-svelte, paraglide). 예외 없이 허용 목록만 적용하고, 통과하면 `static/THIRD_PARTY_NOTICES.txt`를 쓴다.

W0 때의 결과(2026-11-10 17:36 KST): 설치된 패키지는 MIT 265, Apache-2.0 23, ISC 10, BSD-2-Clause 9, BSD-3-Clause 6, Apache-2.0 WITH LLVM-exception 5, 그 밖의 허용 3. 예외 6건은 모두 빌드 기계에서만 도는 도구다: lightningcss와 그 플랫폼 바이너리(MPL-2.0), lru-cache·minimatch(BlueOak-1.0.0), tslib(0BSD), argparse(Python-2.0). 번들에는 svelte, tailwindcss, tw-animate-css, shadcn-svelte, @inlang/paraglide-js(모두 MIT)와 글꼴 6종이 들어갔다.

그 뒤 편집기, 수식, 다이어그램이 더해졌다. 마지막 빌드의 고지는 91 패키지와 글꼴 7종이다. 라이선스별 수는 다시 세지 않았다. 예외 목록에 더해진 갈래(`stated`, `notShipped`)는 `docs/decisions.md`의 5번과 "편집기"의 32–34번에 있다.

결정이 필요한 것:

- **글꼴은 OFL-1.1인데 `deny.toml`의 목록에 없다.** Rust 크레이트 가운데 그 라이선스가 없어서다. 지금은 `notices.ts`가 글꼴 파일에 한해 OFL-1.1만 받는다(다른 라이선스의 글꼴은 빌드 실패). `deny.toml`에 `OFL-1.1`을 넣어 한 목록으로 만들지는 서버 쪽의 파일이라 손대지 않았다.
- **서버 쪽 `NOTICE`와 합치는 일은 하지 않았다.** 루트 `NOTICE`는 서버 쪽이 `cargo about`으로 만드는 파일이다. 웹 고지는 빌드 산출물로만 나온다. 패키징 단계(Docker 이미지)에서 둘을 이어 붙이는 것이 자연스럽다.

## 이름 검사

`pnpm --filter web run names`(`check`에 포함)가 `apps/web` 아래에서 git이 추적하거나 추적할 파일의 이름과 내용을 본다. 이 제품이 전에 불리던 두 이름이 있는지 대소문자 구분 없이 찾는다. 현재 0건이다.

## 순수 로직 (`src/lib/logic`)

계획의 W1이다. React, Redux, react-intl, moment, lodash, marked에 대한 의존이 없고, `window`나 `document`를 읽는 함수도 없다. 번역이 필요한 함수는 메시지를, 날짜 문구가 필요한 함수는 로케일(과 현재 시각)을 인자로 받는다. W1을 마쳤을 때 모듈 21개 2,841행, 테스트 15개 파일 371건이었다.

속성 종류는 `PropertyTemplate.type`으로 구별하고, 종류마다의 동작은 `PROPERTY_BEHAVIORS` 표 한 곳에 있다(`canGroup`, `canFilter`, `filterValueType`, `isReadOnly`, `calculationOptions`, `displayValue`, `exportValue`, `dateRange`).

모듈의 목록은 "원본과 견준 것"의 "모듈과 원본" 표에 있다.

### 와이어 형식에 기대는 곳: `wire.ts` 하나

서버의 와이어 설계가 바뀔 때 고칠 곳을 한 파일로 모았다. `wire.ts`만 아는 것:

- 블록·카드·뷰·보드의 타입과, 빠진 값을 채워 만드는 함수(`createBlock`, `createCard`, `createBoardView`, `createBoard`, `normalizeBlock` 등)
- 블록의 형식 없는 `fields` 안의 내용. 다른 모듈은 `cardProperty`, `cardProperties`, `setCardProperty`, `cardContentOrder`, `viewQuery`, `viewGrouping`, `remapBlockReferences` 같은 함수로만 읽고 쓴다
- 날짜 속성 값과 날짜 필터 값의 저장 형식(`parseDateValue`, `formatDateValue`, `parseFilterDate`, `formatFilterDate`, `plainPropertyValue`). 로직은 `dates.ts`의 내부 표현만 다루고, 와이어 값(달력 날짜 문자열, 시각의 UTC 밀리초 정수)은 여기서만 변환한다
- 식별자 만들기(`createId`: 종류 한 글자 + z-base-32 26자)
- 패치의 모양(`createPatchesFromBlocks`, `createPatchesFromBoards`, `createCardPropertiesPatches`, `createPatchesFromBoardsAndBlocks`)

`tests/logic/wire-boundary.test.ts`가 이 규칙을 검사한다: `wire.ts`가 아닌 로직 모듈에 `.fields`나 `JSON.parse`·`JSON.stringify`가 나오면 실패하고, `wire*` 테스트와 픽스처(`support/factory.ts`)가 아닌 테스트가 `.fields`를 읽어도 실패한다.

경계 밖에 남은 것은 모든 블록이 최상위에 갖는 멤버다. 다른 모듈이 직접 읽는다: `id`, `title`, `type`, `boardId`, `parentId`, `createdBy`, `modifiedBy`, `createAt`, `updateAt`, `deleteAt`, `limited`. 보드에서는 속성 정의 목록(`PropertyTemplate[]`)을 인자로 받을 뿐 보드 객체를 받지 않는다. 이 이름들이 바뀌면 타입 정의가 `wire.ts`에 있으므로 컴파일러가 쓰는 곳을 전부 짚어 준다.

와이어가 바뀌면 사라지거나 달라질 것으로 보이는 부분: `createId`(ID를 서버가 만들게 되면 필요 없다), 날짜 값의 두 형식, `createAttachmentBlock`의 옛 이름 `attachmentId`, `Board.channelId`.

### 정렬, 필터, 검색, 계산의 규칙 (Q1–Q16)

번호는 원본의 결함에 붙인 번호다. 여기에는 지금의 동작만 적는다. 원본이 어땠는지는 "원본과 견준 것"의 "원본의 결함 Q1–Q16"에 같은 번호로 있다. 테스트는 모두 `tests/logic/` 아래다.

| #   | 무엇                                       | 지금의 동작                                                                                                                                                                                                                   | 고정하는 테스트                                                                                                                                                   |
| --- | ------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Q1  | 수동 순서에서 위치 없는 카드               | 위치 없는 카드는 항상 위치 있는 카드 뒤. 그 안에서는 만든 순서(같은 밀리초면 ID 순). 비교자는 순서 관계다                                                                                                                     | `sort.test.ts` "manual order", `sort-properties.test.ts`(속성 기반)                                                                                               |
| Q2  | 텍스트 필터의 대소문자                     | 제목과 모든 속성 종류에서 대소문자를 가리지 않는다. 비교는 `foldCase`(NFKC 정규화 뒤 소문자)로 하며 기기나 화면 언어에 따라 달라지지 않는다. 전각 영문과 풀어 쓴 한글 자모도 같게 본다. 값이 목록이면 항목 하나가 맞으면 된다 | `filter-rules.test.ts` "text conditions do not look at case"                                                                                                      |
| Q3  | 검색의 일치 방식                           | 모든 종류가 부분 일치. 대소문자 처리는 Q2와 같은 `foldCase`                                                                                                                                                                   | `views.test.ts` "search"                                                                                                                                          |
| Q4  | 사람별 그룹에서 사람 없는 카드             | ID가 빈 문자열인 "사람 없음" 그룹이 된다. 이름은 호출부가 주는 문구(선택 속성의 "값 없음" 열과 같은 함수). 맨 앞에 오고, 그런 카드가 없으면 그룹도 없다. 숨김 목록에 `''`가 있으면 다른 그룹처럼 숨는다                       | `views.test.ts` "groups of a board view"                                                                                                                          |
| Q5  | 날짜의 `is` 필터                           | 달력 날짜로 비교한다. 시각이 붙은 값은 보는 사람의 시간대에서 그 시각이 속한 날로 본다. 범위는 양 끝을 포함한 모든 날에 맞는다                                                                                                | `filter-rules.test.ts` "a date property is compared as a calendar day"                                                                                            |
| Q6  | 정렬 옵션이 둘 이상일 때                   | 적힌 순서대로 모두 적용한다. 첫 옵션이 순서를 정하고, 같으면 다음 옵션이 정한다. 모든 옵션에서 같으면 제목, 만든 순서, ID 순. 옵션마다 방향을 따로 뒤집는다. 없어진 속성의 옵션은 건너뛴다                                    | `sort.test.ts` "several sort options", `sort-properties.test.ts`                                                                                                  |
| Q7  | 파일 이름 정리의 금지 문자                 | 전부 지운다                                                                                                                                                                                                                   | `values.test.ts` "text"                                                                                                                                           |
| Q8  | base path가 있을 때 `buildURL`의 상대 결과 | 항상 사이트 루트에서 시작한다(`/base/test`)                                                                                                                                                                                   | `utils.test.ts`의 옮겨 온 2건을 새 기대값으로                                                                                                                     |
| Q9  | 체크박스 계산에서 "체크"                   | 저장된 값이 `true`일 때만 체크다                                                                                                                                                                                              | `calculations.test.ts`의 옮겨 온 4건을 새 기대값으로, `edges.test.ts` "checkbox calculations"                                                                     |
| Q10 | 숫자만 적힌 옛 형식의 날짜 값              | 옮길 데이터가 없으므로 읽지 않는다. 필터·표시·정렬·계산 모두 빈 값으로 본다                                                                                                                                                   | `wire-shapes.test.ts`, `filter-rules.test.ts` "does not read what is not a date value"                                                                            |
| Q11 | 날짜만 있는 값의 저장                      | 날짜만 있는 값은 내부에서도 와이어에서도 달력 날짜(`2026-03-05`)다. 시각으로 꾸미지 않는다(아래 "날짜 속성 값의 와이어 모양")                                                                                                 | `wire-shapes.test.ts` "the stored value of a date property", `values.test.ts` "calendar dates"                                                                    |
| Q12 | CSV의 머리글 행                            | 머리글도 다른 텍스트 칸처럼 감싸고 따옴표를 겹쳐 쓴다                                                                                                                                                                         | `views.test.ts` "CSV rows"                                                                                                                                        |
| Q13 | 필터의 속성을 사람 종류로 바꿨을 때의 조건 | 사람 종류는 선택지 종류와 같은 조건 넷을 유지한다                                                                                                                                                                             | `views.test.ts` "the condition of a filter after its property changed"                                                                                            |
| Q14 | 버려도 되는 변경의 체크포인트              | 앞선 변경이 있으면 늘 그 체크포인트를 잇는다. 맨 처음이면 0                                                                                                                                                                   | `edges.test.ts` "undo manager"                                                                                                                                    |
| Q15 | 카드가 없을 때의 체크 비율                 | 빈 문자열                                                                                                                                                                                                                     | `edges.test.ts` "checkbox calculations"                                                                                                                           |
| Q16 | 생성·수정 시각 필터의 "하루"               | 보는 사람의 시간대의 달력 날짜다. 서울에서 7일 03:00에 만든 카드는 "7일"이다. 시간대는 필수 인자(`FilterContext.timeZone`, IANA 이름)다. 기기의 시간대로 떨어지는 기본값은 없다                                               | `filter-rules.test.ts` "the day a card was created is the day on the reader's calendar", `filter.test.ts`의 옮겨 온 3건(고칠 필요 없이 이제 모든 시간대에서 통과) |

그 밖의 규칙:

- **제목 없는 카드와 값 없는 카드는 정렬 방향과 무관하게 항상 뒤다.**
- **사람 속성으로 정렬하면 사용자 이름순이다.**
- **정렬의 글자 순서는 `SortContext.locale`의 `Intl.Collator`를 따른다.** 언어도 시간대처럼 필수 인자다(기기의 언어로 떨어지지 않는다).
- **체크박스의 "값 있음" 계열 계산**(`countValue`, `countEmpty` 등)은 `false`가 저장돼 있으면 값이 있는 것으로 센다. 체크 여부를 묻는 네 계산만 Q9의 규칙을 따른다.
- **날짜 속성의 earliest·latest·dateRange**는 달력 날짜로 계산한다. `dateRange`는 양 끝을 포함한 날 수다(5일–12일은 "8 days").
- **실행 취소 관리자는 `undo`나 `redo`가 예외를 던져도 멈추지 않는다.** `edges.test.ts`의 다섯 건이 고정한다: 실패한 취소를 다시 시도할 수 있다, 실패한 다시 실행도 같다, 실패 뒤에도 새 변경이 기록된다, 묶음 취소가 실패한 자리에서 멈춘다, 취소 중에 생긴 변경은 기록하지 않는다.

### 날짜: 내부 표현과 규칙

- **달력 날짜**(`CalendarDate`)는 `YYYY-MM-DD` 문자열이다. 시각도 시간대도 없다. 문자열 비교가 곧 날짜 비교다.
- **날짜 값**(`DateValue`)은 `from`과 선택적인 `to`이고, 각 끝은 달력 날짜(`{ date }`)이거나 시각(`{ instant }`)이다. 시각은 날짜에 시간이 붙은 경우에만 쓴다.
- **"하루"는 보는 사람의 시간대 기준이다.** 시각(카드를 만든 때, 시간이 붙은 날짜 값)이 어느 날에 속하는지는 `dateOfInstant(instant, timeZone)`이 정한다. 날짜만 있는 값은 누구에게나 같은 날이다.
- 비교: `is`는 그 날이 값이 덮는 날들(범위면 양 끝 포함) 안에 있는가, `isBefore`는 값의 첫날이 그 날보다 앞인가, `isAfter`는 값의 마지막 날이 그 날보다 뒤인가.
- 정렬: 첫날 순. 같은 날이면 시간 없는 값이 시간 있는 값보다 앞, 시간끼리는 이른 순.
- 표시: 날짜만 있는 값은 시간대와 무관하게 같은 글자(`March 05`)로 나온다.
- 시각 사이의 범위(`dateRange`를 생성·수정 시각에 쓴 경우)는 지나간 시간이다(72시간이면 "3 days"). 달력 날 수로 세는 것은 날짜 속성뿐이다.

### 보는 사람의 시간대: 계정 설정

2026-11-10 19:16 KST에 정했다. "보는 사람의 시간대"는 기기에서 직접 읽지 않는다. 계정 설정의 값이고, 로직에는 인자로 들어온다.

- **`lib/logic`의 모든 날짜·시각 함수는 시간대를 필수 인자로 받는다**(IANA 이름, 예: `Asia/Seoul`). `dateOfInstant(instant, timeZone)`, `startOfDay(date, timeZone)`, `endOfDay(date, timeZone)`, `daysOf(value, timeZone)`, `comparePoints(a, b, timeZone)`, 그리고 `FilterContext.timeZone`, `SortContext.timeZone`, `DisplayContext.timeZone`. 기본값이 없으므로 빠뜨리면 타입 오류다. 시각은 `Date` 객체가 아니라 에포크 밀리초(숫자)로 주고받는다.
- **변환은 `Intl`과 `@internationalized/date`로 한다.** 시각 → 날짜는 `Intl.DateTimeFormat`(`timeZone` 지정)의 `formatToParts`, 날짜 → 그날이 시작하는 시각은 `@internationalized/date`의 `toZoned`.
- **린트 규칙 `fvoci/no-device-time`**(`tools/lint/layers.ts`, `src/lib/logic/**`에 적용, `check`에 포함)이 다음을 막는다: `getHours`·`getDate`·`getDay`·`getMonth`·`getFullYear`·`getTimezoneOffset`·`setHours`·`toLocaleDateString` 등 기기 시각으로 답하는 `Date` 메서드, `new Date(연, 월, …)`과 `new Date('글자')`, `timeZone` 옵션 없는 `Intl.DateTimeFormat`, `resolvedOptions().timeZone`. `getUTC…`, `Date.UTC`, `Date.now()`, `getTime()`은 된다. 규칙의 테스트는 `tests/tools/layers.test.ts`(4건).
- **설정**(`src/lib/logic/regional.ts`): `RegionalSettings`는 세 값이다.

  | 값          | 종류                                           | 기본     | "자동"의 뜻                                                                                          |
  | ----------- | ---------------------------------------------- | -------- | ---------------------------------------------------------------------------------------------------- |
  | `timeZone`  | `'auto'` 또는 IANA 이름                        | `'auto'` | 브라우저의 시간대                                                                                    |
  | `weekStart` | `'auto'`, `'sunday'`, `'monday'`, `'saturday'` | `'auto'` | 화면 언어·지역의 관습(`Intl.Locale`의 주 정보. 한국어·미국 영어는 일요일, 영국 영어·독일어는 월요일) |
  | `hourCycle` | `'auto'`, `'h12'`, `'h23'`                     | `'auto'` | 화면 언어의 관습(한국어·미국 영어는 12시간, 독일어·일본어는 24시간)                                  |

  값의 종류는 서버 설계 5.8절의 세 설정과 같다(2026-11-10 19:47 KST에 맞춤. 전에는 `weekStart`가 1–7의 숫자였다). 푼 뒤의 `weekStart`는 여전히 ISO 8601의 요일 번호(1 월요일 – 7 일요일)다. "자동"이 풀리는 요일은 로케일에 따라 셋 밖일 수 있기 때문이다. `readRegionalSettings(받은 값)`은 서버의 응답을 설정으로 읽고, 모르는 값은 `'auto'`로 읽는다.

  `resolveRegional(settings, locale, browserTimeZone)`이 "자동"을 풀어 `{ timeZone, weekStart, hourCycle }`을 준다. 고정한 시간대를 런타임이 모르면(이름이 바뀐 시간대) 브라우저의 것을, 그것도 못 쓰면 UTC를 쓴다. `weekStart`는 달력을 그리는 `DateProperty.svelte`와 `FilterDate.svelte`가 쓴다. `hourCycle`은 `DisplayContext.hourCycle`로 `displayDateTime`에 들어간다. 세 값을 고르는 화면은 설정 대화상자다.

- **브라우저의 시간대를 읽는 곳은 `src/lib/regional.ts` 하나다**(`browserTimeZone()`). 같은 파일의 `regional(locale)`이 설정을 풀어서 주고, `setRegionalSettings`로 설정을 바꾼다. 작업 공간 저장소(`src/lib/state/workspace.svelte.ts`)가 `Backend`에서 받은 설정으로 그 함수를 부른다. `src` 전체에서 기기의 시간대를 묻는 표현(`resolvedOptions().timeZone`, `getLocalTimeZone`, `getTimezoneOffset`)이 이 파일에만 있다는 것을 `tests/tools/layers.test.ts`의 한 건이 고정한다. 그러려고 shadcn 달력의 머리글 두 파일도 고쳤다(`src/lib/ui/README.md`의 표).
- **테스트**(`tests/logic/time-zones.test.ts` 20건, `regional.test.ts` 9건): 서머타임으로 하루가 23시간·25시간인 날(뉴욕 2026-03-08, 2026-11-01)의 시작과 끝, 그날의 마지막 순간과 다음 날의 첫 순간이 필터에서 갈리는 것, 자정이 건너뛰어지는 날(아바나), 30분만 바뀌는 곳(로드하우), 날짜 변경선 양쪽(키리티마티 UTC+14, 파고파고 UTC−11, `Etc/GMT+12`)에서 같은 시각이 세 날짜로 갈리는 것과 같은 날짜가 26시간 차이로 시작하는 것, 달력에서 빠진 날(사모아 2011-12-30), "올해"가 보는 사람의 달력으로 정해지는 것. 날짜만 있는 값은 열 개 시간대에서 덮는 날·표시·필터·정렬이 모두 같다는 것(속성 기반 300회 포함). 임의의 시각과 시간대에 대해 "시각은 자기 날의 시작과 끝 사이에 있다"(속성 기반 500회).
- 서버 쪽 설계(5.8절): 설정은 `GET`·`PATCH /users/me/settings`로 오가고 응답은 늘 세 멤버를 모두 싣는다. 브라우저가 아는 시간대 이름을 서버가 모르면(시간대 자료의 판 차이) 서버가 거절한다. 그때 저장소가 되돌리고 사용자에게 알린다(서버의 답은 `docs/contract-pending.md` 9절).
- 확인하지 못한 것: 브라우저가 가진 시간대 자료(ICU)의 판이 서로 다를 때의 차이, `Intl.Locale`의 주 정보가 없는 오래된 브라우저(그때는 월요일로 떨어진다).

### 날짜 속성 값의 와이어 모양

2026-11-10 19:47 KST에 서버의 설계 문서(저장소 루트의 `docs/s6-design.md` 5.7절, B안)에 맞췄다.

- 날짜 속성의 값은 문자열에 담긴 JSON이 아니라 **JSON 객체**다. 달력 날짜는 `"2026-03-05"` 문자열, 시각은 1970-01-01 00:00 UTC부터의 **밀리초 정수**다. 날짜인지 시각인지는 JSON의 타입이 말한다.
  - 하루: `{"from": "2026-03-05"}`, 범위: `{"from": "2026-03-05", "to": "2026-03-09"}`(`to`는 그날을 포함), 시각: `{"from": 1772703000000}`.
  - `from`은 필수, `to`는 선택. 둘은 같은 종류여야 하고 `to`는 `from`보다 앞설 수 없다. 같으면 `to` 없이 적는다. `includeTime`과 `timeZone` 멤버는 없다.
  - 날짜 필터의 값도 `"2026-03-05"`다.
- `wire.ts`에서 바뀐 것: `PropertyValue`에 `StoredDateValue`(`{ from: string | number; to?: string | number }`)가 더해졌다. `parseDateValue(stored: unknown)`는 객체를 받고, `formatDateValue(value)`는 객체(빈 값이면 `undefined`)를 준다. `parseFilterDate`·`formatFilterDate`는 달력 날짜 문자열 그대로다. 새 함수 `plainPropertyValue`는 저장 값을 글자나 글자 목록으로 준다(날짜 값은 세고 비교하기 위한 키 `2026-03-05/2026-03-09`가 된다). 날짜가 아닌 종류의 로직(계산의 값 세기, 정렬, 검색, 텍스트 필터)이 이것을 거친다.
- **읽을 때의 규칙**: 서버가 검증하는 것과 같은 조건을 만족하지 않으면 빈 값이다(없는 날, 연도 0, 소수나 음수, 9999년을 넘는 정수, 종류가 다른 두 끝, 거꾸로 된 범위, 그 밖의 타입). 옛 문자열 형식(`"{\"from\":1772712000000}"`)은 더는 읽지 않는다. 옮길 데이터가 없다.
- **쓸 때의 규칙**: 두 끝이 같으면 `from`만, 거꾸로 된 범위는 뒤집어 쓴다. 날짜와 시각이 섞인 값(편집기가 만들지 않는다)은 `from`만 쓴다.
- 내부 표현(`dates.ts`의 `CalendarDate`, `DatePoint`, `DateValue`)과 그 위의 로직은 그대로다. 함께 고친 것 하나: `calendarDate`와 `parseCalendarDate`가 1–99년을 1901–1999년으로 읽던 것(`Date.UTC`의 규칙)을 고쳐 서버가 받는 0001–9999년 전체를 다룬다.
- 테스트: `tests/logic/wire-shapes.test.ts`의 "the stored value of a date property" 10건이 읽기·쓰기·왕복·거절을 고정한다.
- 위 타입은 설계 문서를 읽고 손으로 적은 것이다. 계약에서 생성한 타입에 맞추는 일이 남아 있다.

### 테스트

- 새로 쓴 테스트는 정렬, 검색, 그룹, 속성 표, CSV, 날짜 변환, 서식을 덮는다. 파일별 건수와 원본에서 옮긴 테스트는 "원본과 견준 것"의 "테스트"에 있다.
- **시간대.** `test` 스크립트가 전체 단위 테스트를 프로세스의 `TZ=UTC`, `Asia/Seoul`, `Pacific/Kiritimati`(UTC+14)로 세 번 돌린다. 로직은 기기의 시간대를 묻지 않으므로, 이 실행은 "프로세스의 TZ가 무엇이든 결과가 같다"를 확인하는 용도다. 시간대에 따라 달라져야 하는 동작은 테스트가 시간대를 인자로 넘겨 검사한다.
- **속성 기반 테스트**(`sort-properties.test.ts`, fast-check 4.10.2). 생성한 카드와 수동 순서, 정렬 옵션 조합에 대해 두 가지를 각 300회 검사한다. 하나는 비교자가 순서 관계인지다(자기 자신과는 0, 뒤집으면 부호가 반대, 추이적, 서로 다른 카드는 0이 아님). 다른 하나는 정렬 결과가 입력 순서에 좌우되지 않는지다.

## 화면 조립: 서버의 계약 없이, 메모리 구현 위에서

2026-11-10 22:15 KST부터 2026-11-11 01:50 KST까지 했다. 그때 서버의 계약은 아직 없었다.

화면을 계약 없이 끝까지 조립하려고 **데이터의 경계**를 먼저 세웠다. 화면은 `src/lib/state`의 인터페이스(`Backend`)만 부른다. 지금 그 뒤에 있는 것은 브라우저 안의 메모리 구현이다(`src/lib/state/memory`, 내장 템플릿으로 채운 가짜 작업 공간). 서버를 실제로 부르는 구현은 그 인터페이스의 둘째 구현으로 들어온다. 화면이 서버를 직접 부르지 못하게 하는 것은 린트(`fvoci/no-direct-server`)가 본다.

| 무엇                                                                          | 어디                                                                                                               | 기록                                                                  |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------- |
| 데이터의 경계, 메모리 구현, 저장소                                            | `src/lib/state/`(`backend.ts`, `memory/`, `workspace.svelte.ts`, `board.svelte.ts`, `card.svelte.ts`)              | `docs/contract-assumptions.md`, `docs/decisions.md`의 "데이터의 경계" |
| 층의 순서와 린트                                                              | `tools/lint/layers.ts`(`fvoci/layer-order`, `fvoci/no-direct-server`), `src/lib/README.md`                         | 위 "구조 규칙"                                                        |
| 표시 설정(밝기, 밀도, 카드 여는 방식, 진동), 기기 설정을 따르는 것            | `src/lib/logic/display.ts`, `src/lib/display.svelte.ts`, `src/lib/theme.svelte.ts`, `index.html`의 인라인 스크립트 | `docs/decisions.md`의 "표시 설정과 기기 설정"                         |
| 작업 공간의 틀: 사이드바(목록·서랍·띠), 위쪽 막대, 설정 대화상자, 개발용 표시 | `src/lib/features/shell/`                                                                                          | `docs/decisions.md`, `docs/screens/shell/`                            |
| 뷰 머리글: 뷰 탭과 메뉴, 속성, 그룹, 필터, 정렬, 검색, 접힌 줄                | `src/lib/features/view-header/`                                                                                    | `docs/screens/view-header/`                                           |
| 칸반과 갤러리, 카드의 겉                                                      | `src/lib/features/views/kanban/`, `gallery/`, `src/lib/components/card-face/`                                      | `docs/screens/kanban/`, `gallery/`                                    |
| 테이블                                                                        | `src/lib/features/views/table/`                                                                                    | `docs/screens/table/`                                                 |
| 카드 상세: 세 가지 틀, 속성과 그 정의, 본문, 첨부, 댓글                       | `src/lib/features/card/`                                                                                           | `docs/card-test-mapping.md`, `docs/screens/card/`                     |
| 편집기의 제품 부품, 수식(KaTeX), 다이어그램(Mermaid), 이모지 목록(CLDR)       | `src/lib/editor/`                                                                                                  | `docs/spike-editor.md` 10절                                           |
| 조립: 주소, 보드 페이지, 카드 대화상자                                        | `src/app/`                                                                                                         | `docs/decisions.md`의 "주소와 언어"                                   |
| 빌드의 `.zst`, 원본 폴더에서 떼어 내기, 템플릿의 주소                         | `tools/build/`, 저장소 루트의 `crates/server/assets/templates/`                                                    | `docs/build-notes.md`                                                 |
| 캘린더 뷰의 화면 안(구현은 하지 않았다)                                       | `spikes/calendar/`, `docs/calendar/`(그림 61장)                                                                    | `docs/calendar-proposal.md`(질문 24개)                                |

- **모바일 안의 결정 36건**은 `docs/mobile-proposal.md`의 "결정" 절에 답마다 "바꾸려면 어디를"과 함께 있다. 권고와 다르게 만든 곳은 그 표에 굵게 적었다: 모바일 안의 Q16의 "보이는 열에 만들기"(만들지 않았다), Q22의 표의 "열기", Q24의 터치 드래그 범위, Q34의 단계가 바뀔 때. 무엇을 설정으로 둘지(카드를 여는 방식, 밀도, 진동)도 그때 정해졌고, 설정은 `src/lib/logic/display.ts`와 설정 대화상자에 있다.
- **스스로 정한 것**은 `docs/decisions.md` 한 곳에 있다: 맨 앞의 번호 매긴 항목 58개, 화면별 절 일곱(틀, 뷰 머리글, 칸반과 갤러리, 테이블, 카드 상세, 편집기, 빌드), 검토에서 나온 것과 그 처리.
- **서버에 대해 가정한 것과 물을 것**은 `docs/contract-assumptions.md`에 있다. 답이 난 것은 지웠고, 아직 열린 것만 남았다.
- **화면의 그림**은 `docs/screens/<화면>/`에 있다. 조립된 앱에서 세 단계(좁음·중간·넓음), 밝음과 어두움으로 찍었다. 다시 찍으면 커밋하지 않는 `target/screens/<화면>/`(저장소의 맨 위, `e2e/screens.ts`)에 생긴다. 문서의 그림을 바꾸려면 거기서 옮긴다. 다시 찍는 명령: `FVOCI_SCREENS=1 pnpm exec playwright test e2e/<화면>.spec.ts -g "pictures|screens|screenshots"`(`apps/web`에서, `build` 뒤에).
- **만들지 않은 것**(범위 밖으로 정해진 것): 서버를 실제로 부르는 구현, 로그인·가입 화면의 서버 연결, 캘린더 뷰의 구현, 공유 대화상자, 템플릿 선택기, 아카이브 가져오기·내보내기, 온보딩 투어.

조립된 앱을 띄워 보는 법은 `docs/handoff.md`의 "화면을 띄우는 법과 포트"에 있다.

### 기기에 남는 것

기기에 남는 것은 `localStorage`의 다섯 키뿐이다. 읽고 쓸 때마다 try/catch로 감싸고, 없어도 화면은 그대로 그려진다.

| 키                      | 무엇                                                                           |
| ----------------------- | ------------------------------------------------------------------------------ |
| `fvoci:display`         | 밝기·밀도·카드 여는 방식·진동. 첫 그리기 전에 `index.html`의 스크립트가 읽는다 |
| `fvoci:memory-settings` | 메모리 구현이 들고 있는 나머지 설정. 서버 구현이 들어오면 없어진다             |
| `fvoci:locale`          | 계정 설정 `language`의 사본: 로그인 전과 첫 그리기의 언어. `auto`면 없다       |
| `fvoci:sidebar`         | 접은 사이드바                                                                  |
| `fvoci:dev`             | 개발용 표시(`features/shell/DevReadout.svelte`: 창의 너비와 지금의 단계)       |

### 합친 뒤에 드러나 고친 것

화면 여섯과 편집기를 따로 만들어 합쳤다. 합친 상태에서만 드러난 것이 있었다. 고친 방식과 까닭은 `docs/decisions.md`의 번호에 있다.

| 드러난 것                                                                                       | 처리                                            |
| ----------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| 표가 실행 취소 단축키를 따로 받고 있었다                                                        | 틀만 받는다(50번)                               |
| 칸반과 표가 "보이는 자리 → 저장소가 세는 자리"를 서로 다른 규칙으로 바꾸고 있었다               | 저장소의 `placeAmongAll` 하나로 모았다(50번)    |
| 표의 행에 다른 뷰와 같은 표식(`data-card`)이 없어 뷰 머리글의 종단 테스트가 표에서 실패했다     | 어느 뷰에서나 같은 표식을 단다(49번)            |
| 라이선스 검사 둘이 "라이선스 필드가 없는 패키지"를 서로 다른 목록으로 받고 있었다               | `stated` 하나로 모았다(5번)                     |
| 수식의 가장 작은 글꼴이 스타일시트에 `data:` 주소로 박혀 정책의 `font-src 'self'`에 걸릴 뻔했다 | `assetsInlineLimit: 0`(47번)                    |
| "움직임 줄이기"를 켠 기기에서 다이어그램이 잘렸다                                               | 그리는 자리에서 전환을 껐다(48번)               |
| 시트가 닫히자마자 다음 시트가 열리면 방문 기록이 한 칸 어긋났다                                 | 우리가 낸 "뒤로"가 도착할 때까지 기다린다(58번) |
| 메모리 구현에서 포기한 업로드가 "다음 업로드는 실패"라는 테스트의 주문을 가로챌 수 있었다       | 포기한 호출은 포기한 것으로 끝난다              |

뒤의 셋은 다시 겪을 수 있는 함정이라 경위를 적어 둔다.

- **다이어그램.** 조립된 화면의 그림을 찍다가 찾았다(그림은 움직임을 끄고 찍는다). 페이지는 그 설정에서 모든 전환을 0.01ms로 줄인다. 다이어그램 라이브러리는 노드를 놓자마자 그림의 크기를 잰다. 같은 함정이 "놓자마자 재는" 다른 코드에도 있을 수 있다.
- **방문 기록.** 종단 실행(711건)에서 휴대폰 크기의 한 건이 한 번 실패해 찾았다. 닫히는 시트는 자기 칸을 `history.back()`으로 돌려주는데 이것은 늦게 일어난다. 그사이에 넣은 칸을 브라우저가 엉뚱한 자리에서 셀 때가 있다. 그러면 다음 것을 닫을 때 카드에서 나가 버린다. 방문 기록에 쓰는 것(시트의 칸, 라우터의 주소, 카드를 닫는 한 걸음)이 기다리게 했다(`lib/layout/back.svelte.ts`의 `afterOwnSteps`).
- **포기한 업로드.** 기기가 느릴 때만 드러났다. 카드 스펙을 여섯 번씩 돌렸을 때 한 번 나왔다.

### 검토 한 번

2026-11-11 00:50–01:10 KST에, 합친 상태를 하위 에이전트 하나가 읽기 전용으로 한 번 훑었다. 본 것은 기능 사이에 겹친 것, 쓰이지 않는 것, 층을 돌아가는 것, 키보드로 닿지 않는 것, 이름 없는 단추, 500줄을 넘는 파일이다. 층의 규칙을 어긴 곳, 키보드로 닿지 않는 곳, 이름 없는 단추는 없었다.

고친 것 13건과 남긴 것은 `docs/decisions.md`의 "검토에서 나온 것과 처리"에 까닭과 함께 있다. 고친 것 가운데 동작이 바뀐 것은 이렇다.

- 사람 속성에서 고를 수 있는 사람은 어디서나 보드의 구성원이다(표의 칸은 전체 사용자를 내놓고 있었다).
- 표의 끌기도 진동 설정을 따른다.
- 속성 목록에서 읽는 뷰가 없던 스위치 둘("제목", "댓글 및 설명")을 뺐다. 새 표는 제목 열의 너비를 저장하지 않는다.
- 표에서 링크 값이 행마다 Tab을 세 번 세우던 것을 없앴다. 손으로 만든 라디오 묶음 넷과 탭 줄이 화살표 키를 받는다.
- 화면 조립에서 만들고 쓰지 않은 것을 지웠다(`BoardStore.cardTemplates`, `PendingWrites.busy`, `BodyEditor`의 내보낸 함수 둘, 메시지 셋).

남긴 것 가운데 정해야 했던 것은 둘이다.

- 선택지 편집이 카드 상세와 칸반에 두 벌인 것. 표의 칸에서는 선택지를 새로 만들지 못한다. 아직 정하지 않았다.
- 카드를 지울 때 카드 상세만 묻는 것. **정해졌다: 어느 뷰에서나 묻지 않는다**(사용자 결정, 2026-11-11 02:12 KST). 코드는 아직 카드 상세에서 묻는다.

## 서버의 계약을 기다리며 한 것

2026-11-10 19:40–20:50 KST에 했다. 서버의 계약이 나오기 전이었고, 결정을 반영하고 가짜 데이터로 만들 수 있는 화면 부품을 만들었다. **모든 부품은 값과 콜백을 인자로 받고 API·상태 모듈에 기대지 않는다.** 칸반·테이블 화면의 조립은 그 뒤 "화면 조립"에서 했다.

| 무엇                                               | 어디                                                                              | 기록                                                  |
| -------------------------------------------------- | --------------------------------------------------------------------------------- | ----------------------------------------------------- |
| 날짜 값의 와이어, 지역 설정의 값, 파일 주소의 전제 | `src/lib/logic/wire.ts`, `regional.ts`, `src/lib/editor/files.ts`                 | 위 "날짜 속성 값의 와이어 모양", "보는 사람의 시간대" |
| 글꼴: 한국어 세 파일, 한국어·일본어는 CJK 먼저     | `tools/fonts/`, `src/styles/app.css`, `e2e/fonts.spec.ts`                         | 위 "글꼴"                                             |
| 번역 결정, 한국어 검수 표                          | `messages/`, `docs/ko-review.md`                                                  | 아래 "번역"                                           |
| 내장 템플릿                                        | 저장소 루트의 `crates/server/assets/templates/`                                   | 아래 "내장 템플릿의 변환 산출물"                      |
| 모바일 화면 안(합의용)                             | `spikes/mobile/`, `docs/mobile/`(그림 64장), `docs/mobile-proposal.md`(질문 36개) | 그 문서                                               |
| 본문 읽기 전용 표시기                              | `src/lib/editor/render.ts`, `BodyView.svelte`, `drawing.ts`                       | `docs/spike-editor.md` 9절                            |
| 속성 편집기 15종                                   | `src/lib/components/properties/`, `stories/properties/`, `tests/ui/properties/`   | 아래, `docs/properties-test-mapping.md`               |
| 드래그: 결정 1–11, 열 끌기·여러 장·놓을 곳         | `src/lib/drag/`, `src/lib/logic/drop-target.ts`, `spikes/drag/`                   | `docs/spike-drag.md` "결정의 기록"                    |

### 스파이크

스파이크 화면은 `spikes/<이름>/`에 두고 제품 빌드와 따로 돈다(`vite.spikes.config.ts`, `playwright.spikes.config.ts`). 제품으로 가져갈 순수 로직은 처음부터 `src/lib` 아래에 두고 테스트한다.

| 스파이크           | 결과 문서              | 제품 쪽에 남은 코드                                                               |
| ------------------ | ---------------------- | --------------------------------------------------------------------------------- |
| 편집기와 본문 저장 | `docs/spike-editor.md` | `src/lib/editor/`(허용 목록과 검증기, Tiptap 확장 구성, 마크다운 변환, 파일 주소) |
| 드래그             | `docs/spike-drag.md`   | `src/lib/logic/drop-target.ts`, `src/lib/drag/`                                   |

### 모바일 화면 안

구현 전에 합의하려고 만든 자료다. 완성된 구현이 아니다. `spikes/mobile/`의 화면 여섯(사이드바, 칸반, 테이블, 카드 상세, 메뉴 넷, 뷰 머리글)마다 안 A와 B를 그렸다. 그림 64장과 권고, 정해야 할 **질문 36개**가 `docs/mobile-proposal.md`에 있다. 질문의 답은 그 뒤에 났고 같은 문서의 "결정" 절에 있다.

- 처음에는 좁은 화면 360×740과 넓은 화면 1280×800만 그렸다(2026-11-10 20:31 KST, 질문 28개).
- 그 뒤 **중간 단계**를 넣었다(2026-11-10 21:38 KST). 466×678, 626×890, 890×626, 820×1180에서 배치가 달라지는 것만 그림으로 남겼다(새 14장). 함께 더한 것은 이렇다.
  - 화면별로 어느 너비에서 배치가 바뀌는지의 표.
  - 경계 값의 안: 페이지 600·840px, 보드의 칸 544·992px, 카드 576px. 창이 아니라 **상자의 너비**를 컨테이너 쿼리로 본다.
  - 한가운데 세로선에 걸리는 안의 점검, 좌우 안전 영역, 쓰는 도중에 창 크기가 바뀔 때의 동작.
  - 질문 8개.
- 이 크기들은 어림값이고 실제 기기에서 확인하지 않았다.
- 한가운데 세로선 점검의 결과로, 카드 상세의 넓은 화면 권고(가운데 대화상자)를 거두고 질문으로 돌렸다(모바일 안의 Q8). 가운데 대화상자는 속성 값과 댓글 입력 칸이 모두 접히는 자리(가정)에 온다. 그 뒤의 결정은 A(가운데 대화상자)다. 접히는 자리에 걸리는 것(모바일 안의 Q35)은 받아들이고, 실제 기기에서 본 뒤 다시 정하기로 했다.
- 그림을 다시 찍는 명령: `FVOCI_MOBILE_SHOTS=1 pnpm --filter web run spikes:e2e shots --project desktop`. 그림은 커밋하지 않는 `target/screens/spikes/mobile/`에 생긴다. 약 1분 25초가 걸리고, 평소의 `spikes:e2e`에서는 건너뛴다. 같은 입력이면 같은 바이트가 나온다.

### 본문 읽기 전용 표시기

편집기(Tiptap) 없이 문서를 DOM으로 그린다. 허용 목록의 노드 15종과 마크 5종을 모두 그리고, 검증을 통과하지 못한 문서는 그리지 않으며, 링크의 `rel`·`target`은 그릴 때 붙인다. 글자는 텍스트 노드로만 넣는다.

- 편집기가 그린 것과의 비교(`tests/ui/body-compare.test.ts`): 템플릿 문서 58개와 손으로 만든 문서 16개, 스파이크 예제 1개, 모두 75개에서 정규화한 DOM이 같다. 비교에서 빼는 것 열 가지는 그 파일 머리에 적혀 있고, 한 번도 쓰이지 않는 항목이 생기면 테스트가 실패한다. 브라우저에서는 모든 요소의 위치·크기와 계산된 스타일 36가지를 견준다(`spikes/editor/view.spec.ts`).
- 번들 크기(2026-11-10 22:01 KST, 제품 설정으로 따로 묶음): `renderBody`(검증기 포함) 원본 8,132 / gzip 3,102 / **brotli 2,793**바이트. Svelte 부품 `BodyView`까지의 증가분은 brotli 4,191바이트. 같은 방법으로 잰 편집기 조각은 407,951 / 129,024 / 109,957이다. 스파이크 문서 6절의 편집기 수치(523,262 / 146,035 / 121,076)는 이 방법으로 재현되지 않았다. 공백을 지우지 않은 묶음을 잰 것으로 추정한다(확인하지 못함).
- 비교하다 찾아 고친 것: 편집기 쪽 CSS의 결함 셋(`editor.css`), 검증기가 `{"type":"constructor"}` 같은 마크를 통과시키던 것, 검증기가 문서 모양이 아닌 값에서 예외를 던지던 것, 굵은 글자 속 줄바꿈·멘션에 마크가 붙어 저장 문서가 검증에 걸리던 것(`toStoredDoc`이 뗀다).

### 속성 편집기 15종

`src/lib/components/properties/`의 `PropertyValue.svelte`가 속성 정의의 종류로 갈라 그린다. 쓰임은 둘이다: 표시(`mode: 'display'`)와 편집. 15종(text, number, select, multiSelect, date, person, multiPerson, checkbox, url, email, phone, createdTime, createdBy, updatedTime, updatedBy) 모두 표시와 편집(읽기 전용 넷은 표시)이 된다. 만들었을 때 종류마다 Storybook 이야기(75개, 겉 부품 8개 별도)와 상호작용 테스트(`tests/ui/properties/<종류>.test.ts`, 318건)가 있었다.

- **날짜**: 범위와 시각을 포함한다. 시각 없는 값은 달력 날짜, 시각 있는 값은 UTC 밀리초다. 입력과 표시는 **계정의 시간대**(인자 `regional.timeZone`)로 한다. 기기의 시간대를 읽지 않는다는 것을 테스트가 고정한다(기기 시간대를 읽게 바꾸면 28건이 실패하는 것을 확인했다). 서머타임으로 없는 시각은 건너뛴 만큼 뒤로, 두 번 있는 시각은 첫 번째로 읽는다. 순수 로직은 `src/lib/logic/date-entry.ts`.
- **좁은 화면에서 선택 목록과 날짜 선택기가 뜨는 방식**은 한 곳에 모았다: `src/lib/components/picker-surface/PickerSurface.svelte`. "넓으면 팝오버, 좁으면 바닥 시트"이고, 어느 쪽인지는 인자 `presentation` 하나가 정한다.
- **직접 쓴 공용 부품의 자리**가 생겼다: `src/lib/components/`(겉, 선택 목록, 선택지 배지). `src/lib/ui`는 생성물의 자리라 섞지 않았다.
- 선택지 색 10종의 토큰(`--option-*`)을 `app.css`에 더했고 대비 검사에 넣었다.
- 이메일과 숫자의 검증은 저장된 값이 아니라 입력한 값에 한다. 체크박스는 `'true'`만 체크로 읽는다. 모르는 사용자 ID는 "알 수 없는 사용자"로 보인다. 선택지의 이름을 바꿀 수 있다.
- 처음에는 `src/lib/features/properties/`에 두었다. 기능 폴더끼리는 서로 가져올 수 없어서, 화면 조립 때 `components/properties`로 내렸다.
- 남은 것: 수정 시각은 받은 카드의 `updateAt`만 본다(`docs/contract-assumptions.md`의 A8). 보드 밖 사용자의 검색·추가는 API가 있어야 한다.

### 드래그

결정 1–11의 기록과 상태는 `docs/spike-drag.md`의 "결정의 기록"에 있다.

- 그때 구현한 것: 조정 값을 한 곳에(`src/lib/drag/settings.ts`), 보드 밖에 놓으면 취소, 정렬된 보기에서는 열만 바뀌고 안내를 보임, 길게 누르기가 성립할 때 진동, 열 끌기(두 단계 정렬).
- 기반과 단위 테스트까지 한 것: 여러 장(`moveItems`, `selection`), 숨긴 열 같은 "놓을 곳"(`data-drag-zone`).
- 테스트는 순수 함수 129건, jsdom 25건, 브라우저 94건이었다.
- `Sortable`의 공개 형태가 바뀌었다(`active.to`가 목록·자리·놓을 곳의 합이 됐다).
- 표의 열 순서와 사이드바에는 화면 조립 때 붙였다.

### 화면 크기의 세 단계

2026-11-10 20:40 KST에 계획에 더해진 방침이다. 좁은 화면과 넓은 화면 사이에 중간 단계가 있다(접는 기기, 태블릿). 단계는 기기가 아니라 쓸 수 있는 너비로 나누고, 부품은 자기가 놓인 칸의 너비를 본다. 화면별 안, 경계 값의 안, 정할 질문은 `docs/mobile-proposal.md`에 있다.

그 방침에 비추어 그때의 코드를 확인하고 고친 것(2026-11-10 20:58 KST):

| 확인한 것                                         | 결과                                                                                                                        |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| 창 크기가 바뀔 때 진행 중인 드래그                | **고쳤다.** 너비가 달라지거나 높이가 120px 넘게 달라지면 취소한다. `docs/spike-drag.md`의 "창 크기가 바뀌면 취소"           |
| 창 크기가 바뀔 때 편집기의 글·커서·초점·열린 목록 | 유지된다(고칠 것이 없었다). `docs/spike-editor.md` 6.5절                                                                    |
| 드래그의 시작 방식이 너비로 갈리는가              | 아니다. 누름마다 포인터 종류(`pointerType`)로 갈린다. 좁은 창의 마우스와 넓은 화면의 손가락을 테스트로 고정했다             |
| 좌우 안전 영역을 한 값으로 주는 곳                | **고쳤다.** 속성 편집기의 바닥 시트가 왼쪽·오른쪽·아래를 각자의 값으로 준다(`PickerSurface.svelte`, 테스트 5건)(주 1)       |
| viewport 메타의 `viewport-fit=cover`              | 그때는 앱의 `index.html`에 없었다. 화면 조립 때 배치와 함께 넣었다(주 2)                                                    |
| 높이 단위                                         | 직접 쓴 부품은 `dvh`를 쓴다(시트 `85dvh`, 목록 `45dvh`). 편집기 스파이크의 JSON 칸 하나가 `70vh`인데 스파이크 화면의 것이다 |
| 종단 테스트의 뷰포트                              | 제품 e2e에 중간 크기 프로젝트 `middle`(626×890, 터치)을 더했다. 스파이크의 테스트는 파일 안에서 크기를 정한다               |

1. 그때 `src/` 아래에서 안전 영역을 쓰는 곳은 그 시트 하나였고 아래쪽 값만 주고 있었다. 생성 UI의 시트와 사이드바(`src/lib/ui`)는 안전 영역을 전혀 쓰지 않는다. 틀의 안전 영역은 화면 조립 때 넣었다(`docs/decisions.md`의 "작업 공간의 틀").
2. 그 값이 없으면 `env(safe-area-inset-*)`가 모두 0이라 여백이 효과가 없다. 넣으면 내용이 노치 아래까지 들어가므로 화면 전체에 안전 영역 여백을 함께 넣어야 한다.

계획에 적힌 크기들(466×678, 626×890, 890×626, 820×1180 등)은 **어림값이다.** 출처가 고르지 않고 서로 어긋나는 측정이 있어, 실제 기기에서 확인하기 전에는 확정값으로 쓰지 않는다. 테스트와 문서에서도 그렇게 적었다.

## 미결 사항

W0 때 남긴 것 가운데 아직 열려 있는 것이다. `{{.BaseURL}}`을 스크립트가 읽는 방법은 풀렸다(`docs/decisions.md`의 23번).

| 항목                             | 내용                                                                                                                                |
| -------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| 중국어의 문장부호 글꼴           | 중국어 두 로케일은 라틴 family가 앞이라 문장부호가 Noto Sans의 모양이다(말줄임표가 아래에 붙는다). 전각 따옴표 때문에 그렇게 정했다 |
| 비 CJK 언어에서의 CJK 순서       | `lang`이 ko·ja·zh가 아니면 KR, JP, SC, TC 순이다. 영어 화면에 일본어 글이 나오면 한자가 한국어 자형으로 그려진다                    |
| 브랜드 색                        | 무채색 그대로                                                                                                                       |
| Noto Serif의 용도                | 도구만 지원한다(`--family noto-serif,noto-serif-kr`). 토큰에는 없다                                                                 |
| 이탤릭                           | Noto Sans Italic은 별도 파일이라 넣지 않았다. 지금은 브라우저가 기울여 그린다                                                       |
| `OFL-1.1`을 `deny.toml`에 넣을지 | 위 "제3자 라이선스"                                                                                                                 |
| 웹 고지와 서버 `NOTICE` 합치기   | 패키징 단계에서                                                                                                                     |
| 번들 크기 상한                   | 정하지 않았다. 위 "번들 크기"                                                                                                       |

## 원본에서 옮겨 온 자료

원본의 번역과 내장 템플릿을 이 앱이 쓰는 모양으로 바꾼 일이다. 일 자체가 원본에 관한 것이라 이 절은 원본을 그대로 말한다. 원본의 폴더는 저장소 루트의 `webapp/`과 `server/`다. 그 폴더 없이도 검사와 빌드가 도는 것은 `docs/build-notes.md`에 있다.

### 번역

2026-11-10 19:00 KST에 변환했고 20:30 KST에 결정을 반영했다.

원본 `webapp/i18n/*.json`을 paraglide 메시지로 바꿨다. 변환 도구는 2026-11-11에 지웠고, 그 결과인 `messages/<태그>.json`이 원천이다. 통계, ICU 조사, 뺀 메시지, 덮어쓴 문구의 목록은 `docs/i18n-conversion.md`에, 고친 한국어와 채운 한국어의 검수 표는 `docs/ko-review.md`에 있다.

- 원본이 번들하는 로케일은 17개이고 **한국어는 거기에 없다**(`webapp/src/i18n.tsx`). 변환한 것은 그 17개와 `ko`, 모두 18개다.
- 메시지 ID는 그대로다. 점과 하이픈이 든 ID는 `m['BoardComponent.add-a-group']()`처럼 부른다.
- **문구를 고치는 곳은 `messages/<태그>.json` 하나다.** 이 앱만의 키도, 원본에서 온 문구도 거기에 있다. `check`가 키 집합(`check-keys.ts`)을, `test`(`tests/tools/i18n.test.ts`)가 번역이 영어 메시지의 인자·태그만 쓰고 괄호와 태그를 닫았는지를 본다.
- 키 집합 검사: `en`에 없는 키가 어느 로케일에든 있으면 실패, `ko`에 빠진 키가 있으면 실패. 다른 로케일의 빠진 키는 영어로 떨어지므로 개수와 이름만 보고한다.

사용자 결정(2026-11-10)을 반영한 것(`docs/i18n-conversion.md`, `docs/ko-review.md`에 상세):

| 결정                                                                       | 한 것                                                                                                                                                                                                                                                                                                                                          |
| -------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 제품 이름으로 쓰인 "Boards"는 fvoci로, 카테고리 이름은 그대로              | 영어 **4건**을 바꿨다(`BoardPage.newVersion`, `OnboardingTour.OpenACard.Body`, `error.invalid-read-only-board`, `error.not-logged-in`). 후보 다섯 가운데 `SidebarCategories.CategoryMenu.DeleteModal.Body`는 읽어 보니 제품 이름이 아니라 기본 카테고리의 이름과 보통명사여서 그대로 뒀다. 다른 로케일은 이름이 라틴 문자로 남은 23건을 바꿨다 |
| 네덜란드어 `medefvocigebruikers`                                           | `andere fvoci-gebruikers`로 고쳤다                                                                                                                                                                                                                                                                                                             |
| 이상한 기존 한국어를 고치고 용어를 통일                                    | 덮어쓴 한국어 130개(원 표의 id 53개 + 통일 때문에 더 고친 77개). 검수 표는 `docs/ko-review.md`(148행: 고친 130 + 채운 18)                                                                                                                                                                                                                      |
| 팀·관리자 메시지는 남기고, 게스트·모바일 안내·둘러보기의 한 단계는 뺀다    | 6개를 뺐다(`badge.guest`, `guest-no-board.*` 2, `Error.mobileweb`, `SidebarTour.SidebarCategories.*` 2)                                                                                                                                                                                                                                        |
| `ShareBoard.searchPlaceholder`, `OnboardingTour.CopyLink.Body`를 다시 쓴다 | 영어와 한국어를 다시 썼고 다른 로케일은 영어로 떨어진다                                                                                                                                                                                                                                                                                        |
| 로케일별로 나눠 받기는 조사만                                              | `docs/i18n-conversion.md`의 "로케일별로 나눠 받기". 빌드 설정은 바꾸지 않았다. W2에서 실제 화면으로 다시 재고 정한다                                                                                                                                                                                                                           |

검수에서 볼 것: checked를 "확인"도 "선택"도 아닌 "체크됨 / 체크 안 됨"으로 옮겼다(두 말이 각각 Confirm, Select와 겹친다). `id`·`it`의 `BoardPage.newVersion`은 단수 "Board"였는데 제품 이름으로 보고 바꿨다. 고친 한국어와 다른 로케일의 손본 문구는 그 언어를 쓰는 사람이 화면에서 본 적이 없다.

### 내장 템플릿의 변환 산출물

2026-11-10 20:09 KST에 원본의 템플릿 아카이브에서 변환했고, 변환기는 2026-11-11에 지웠다. 내장 템플릿 13개는 저장소 루트의 `crates/server/assets/templates/`에만 있다. 서버가 그 파일을 바이너리에 넣고, 메모리 구현(`src/lib/state/memory/seed.ts`)이 같은 파일을 읽는다. **글은 그 폴더의 템플릿 파일에서 고친다.** 모양과 고치는 법은 그 폴더의 `README.md`에 있다.

- 템플릿마다 보드, 뷰, 카드(본문 문서 포함)가 한 파일이다. 템플릿 13, 뷰 38, 카드 77, 본문 58.
- **변환이 글에서 바꾼 곳은 47곳**이고 목록이 그 폴더의 `CHANGES.md`에 있다: 원본의 두 이름 → `fvoci` 9, 제품을 가리키는 "Boards" → `fvoci` 5, 본문의 바깥 링크와 그 문장 1, 바깥 주소인 링크 속성 값 32. 링크 값을 지우거나 옮긴 규칙은 `docs/decisions.md`의 42번과 T1–T7에 있다.
- 날짜 속성 값 19건은 새 와이어 모양(`{"from": "2022-11-22"}`)이다.
- 테스트: `tests/tools/builtin-templates.test.ts`(수, 본문의 검증, 원본 이름·바깥 주소가 남지 않음, 날짜 값, 참조의 온전함).
- 서버는 온보딩 보드를 제목이 아니라 템플릿 파일의 `onboarding` 표식으로 찾는다(저장소 루트의 `docs/contract-pending.md` 5절).
- 미결: 그림을 넣을지, 템플릿 수를 줄일지(서버 설계 3.10), 글의 검수(바뀐 47곳과 `CHANGES.md`의 "바꾸지 않았지만 검수할 때 볼 것"), 온보딩의 "Share cards on Channels" 카드처럼 이 제품에 없는 기능을 설명하는 카드를 둘지.

## 원본과 견준 것

원본의 코드와 나란히 놓고 본 것을 모았다. 포팅의 흔적을 걷어 낼 때 이 절과 바로 앞 절을 떼어 내면 된다.

- 워크스페이스에 원본 `webapp/`은 들어가지 않는다.
- 순수 로직은 원본의 동작을 옮기되 구조는 새로 짰다. `PROPERTY_BEHAVIORS` 표는 원본의 종류별 클래스 16개를 대신한다.
- `{{.BaseURL}}`을 스크립트가 읽는 값으로 원본은 인라인 스크립트의 `window.baseURL`을 썼다.

### 모듈과 원본

| 모듈                       | 내용                                                                                        | 원본                                                                                                                                                                                                |
| -------------------------- | ------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `wire.ts`                  | 블록·카드·뷰·보드의 모양과 생성, 식별자, 날짜 값의 읽기·쓰기, 패치 생성                     | `blocks/*`, `utils.ts`(`createGuid`, `base32encode`, `blockTypeToIDType`), `octoUtils.tsx`(`hydrateBlock`), `properties/date/date.tsx`(`createDatePropertyFromString`, `datePropertyToString`)      |
| `filter.ts`                | 필터 그룹 판정, 필터를 만족하는 속성 값                                                     | `cardFilter.ts`                                                                                                                                                                                     |
| `sort.ts`                  | 수동 순서, 속성별 정렬, 뷰 이름순                                                           | `store/cards.ts:187-331`, `blocks/boardView.ts`(`sortBoardViewsAlphabetically`)                                                                                                                     |
| `search.ts`                | 검색어로 카드 거르기                                                                        | `store/cards.ts:333-369`                                                                                                                                                                            |
| `card-list.ts`             | 필터 → 검색 → 정렬을 잇는 한 함수                                                           | `store/cards.ts:371-392`의 선택자                                                                                                                                                                   |
| `group.ts`                 | 칸반 열(보이는 것, 숨긴 것, 사람별)                                                         | `boardUtils.ts`                                                                                                                                                                                     |
| `calculations.ts`          | 계산 20종                                                                                   | `components/calculations/calculations.ts`                                                                                                                                                           |
| `properties.ts`            | 속성 종류별 동작 표(그룹·필터 가능 여부, 계산 목록, 표시 값, 내보내기 값, 달력 기간)        | `properties/*/property.tsx`, `properties/types.tsx`, `properties/index.tsx`                                                                                                                         |
| `csv.ts`                   | CSV의 행 만들기                                                                             | `csvExporter.ts`의 `generateTableArray`, `encodeText`                                                                                                                                               |
| `undo.ts`                  | 실행 취소 관리자                                                                            | `undomanager.ts`                                                                                                                                                                                    |
| `block-tree.ts`            | 갱신 병합, 블록 트리 복제                                                                   | `octoUtils.tsx`(`mergeBlocks`, `duplicateBlockTree`)                                                                                                                                                |
| `filter-conditions.ts`     | 값 종류별 필터 조건과 그 이름                                                               | `octoUtils.tsx`(`filterConditionValidOrDefault`, `filterConditionDisplayString`)                                                                                                                    |
| `content-order.ts`         | 콘텐츠 블록의 2차원 재배치                                                                  | `components/cardDetail/cardDetailContentsUtility.ts`                                                                                                                                                |
| `dates.ts`                 | 달력 날짜(`CalendarDate`), 날짜 값(하루·시각·범위), 보는 사람의 시간대에서 시각이 속하는 날 | `properties/date/property.tsx`(`getDateFrom`, `getDateTo`), `date.tsx`·`fullCalendar.tsx`·`dateFilter.tsx`에 흩어진 `setHours(12)`와 시간대 보정. 원본과 다른 모델이다(위 "날짜: 내부 표현과 규칙") |
| `regional.ts`              | 보는 사람의 지역 설정(시간대, 주의 시작 요일, 12·24시간)과 "자동"을 푸는 함수               | 원본에 없음(원본은 어디서나 기기의 시간대와 `moment`의 로케일을 썼다)                                                                                                                               |
| `drop-target.ts`           | 드래그에서 놓을 자리(목록과 순번)를 좌표로부터 계산                                         | 원본에 없음(원본은 `react-dnd`의 hover 콜백 안에서 계산)                                                                                                                                            |
| `format.ts`                | 날짜·기간·크기 문구                                                                         | `utils.ts`(`displayDate`, `inputDate`, `displayDateTime`, `relativeDisplayDateTime`, `roundTo`, `humanFileSize`)                                                                                    |
| `text.ts`                  | 문자열 보조                                                                                 | `utils.ts`(`ensureProtocol`, `sanitizeFilename`, `htmlEncode`, `htmlDecode`, `hashCode`, `countCheckboxesInMarkdown`)                                                                               |
| `users.ts`                 | 표시 이름                                                                                   | `utils.ts`(`getUserDisplayName`, `getFullName`)                                                                                                                                                     |
| `urls.ts`                  | base path와 경로 채우기                                                                     | `utils.ts`(`getBaseURL`, `buildURL`, `showBoard`)                                                                                                                                                   |
| `versions.ts`, `arrays.ts` | 버전 비교, 배열 보조                                                                        | `utils.ts`(`compareVersions`, `arraysEqual`, `arrayMove`)                                                                                                                                           |
| `diagnostics.ts`           | 예상 밖의 데이터를 만났을 때의 보고                                                         | `utils.ts`(`assert`, `assertFailure`, `logError`)                                                                                                                                                   |

### 원본과 다르게 한 것

| 항목                        | 원본                                                                                                                                                                                   | 포트                                                                                                          | 이유                                                                                                |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| 실행 취소 중의 예외         | `undo`나 `redo`가 던지면 실행 중 표시가 켜진 채 남아 그 뒤의 모든 실행 취소와 기록이 조용히 무시된다                                                                                   | `try`/`finally`로 표시를 끈다. 실패한 변경은 그대로 "다음에 취소할 것"으로 남는다                             | 서버 오류 한 번으로 실행 취소가 영영 멈춘다. `tests/logic/undo-more.test.ts`                        |
| 실행 취소 관리자의 개수     | 모듈 전역 하나                                                                                                                                                                         | 클래스. 만드는 쪽이 정한다                                                                                    | 테스트가 서로의 상태에 기대지 않는다. 쓰이지 않던 `limit`(항상 0)은 뺐다                            |
| 정렬이 받은 배열            | 받은 배열을 제자리에서 정렬한다                                                                                                                                                        | 새 배열을 돌려준다                                                                                            | 호출부가 원본 배열을 다시 써도 안전하다                                                             |
| 기간과 상대 시각의 문구     | moment(`humanize`, `fromNow`): "a day", "a few seconds ago"                                                                                                                            | `Intl`: "1 day", "now". 반올림 구간은 moment와 같다                                                           | 의존성 하나(로케일 파일 포함)를 줄인다. 문구가 조금 다르다                                          |
| 번역 문구                   | `intl.formatMessage`를 안에서 부른다                                                                                                                                                   | 메시지(필터 조건 이름), 문자열(CSV의 제목 열 이름, 값 없는 그룹의 이름, 사라진 선택지의 표시)을 인자로 받는다 | 지시 사항                                                                                           |
| 값 없는 그룹의 이름         | 영어 고정 `No ${name}`                                                                                                                                                                 | 호출부가 주는 함수                                                                                            | 번역할 수 없었다                                                                                    |
| 새 보드의 기본 속성 이름    | 영어 고정 `Status`                                                                                                                                                                     | 인자(기본값 `Status`)                                                                                         | 같은 이유                                                                                           |
| 체크박스 세기               | marked로 전체를 렌더링하며 센다                                                                                                                                                        | 줄 단위로 작업 목록 항목을 센다(코드 펜스 안은 제외)                                                          | 마크다운 렌더러는 W4에서 정한다. 원본 테스트는 통과한다                                             |
| 복제할 때 트리 밖의 ID      | `undefined`로 바뀐다                                                                                                                                                                   | 그대로 둔다                                                                                                   | 직렬화하면 `null`이 된다                                                                            |
| 첨부 블록의 업로드 상태     | 블록 객체에 `isUploading`, `uploadingPercent`를 붙인다                                                                                                                                 | 붙이지 않는다                                                                                                 | 화면 상태다. 상태 모듈(W2)에 둔다                                                                   |
| 알 수 없는 조건·종류의 보고 | `Utils.assertFailure`(개발 빌드에서 콘솔)                                                                                                                                              | `diagnostics.report`(개발 빌드에서 콘솔, 테스트에서 감시 가능)                                                | 전역 `Utils`가 없다                                                                                 |
| 텍스트 조건에 목록 값       | `startsWith`·`endsWith`는 예외(`value.startsWith is not a function`), `contains`는 항목 하나와 전체가 같아야 참                                                                        | 항목 하나가 조건에 맞으면 참(Q2와 함께 정리)                                                                  | 여러 값 속성에 텍스트 조건이 걸린 경우                                                              |
| 형식이 어긋난 저장 값       | 예외로 끝나는 경우가 있다: 정렬에서 JSON이 아닌 날짜 값이나 `null`, 목록이 아닌 `contentOrder`, 사용자 정보 없이 여러 사람 속성으로 정렬, 텍스트가 든 여러 값 선택 속성의 고유 값 세기 | 빈 값으로 보고 계속한다                                                                                       | 값은 API와 가져오기로도 들어온다. 한 장의 카드 때문에 뷰 전체가 죽지 않게 한다                      |
| 날짜 속성에 남은 텍스트     | 숫자로 시작하면 `parseInt`로 읽어 1970년의 날짜로 보인다(`2021-01-01` → `January 01, 1970`). 날짜가 아닌 `from`은 `Invalid Date`로 보인다                                              | 날짜 값의 객체 형식이 아니면 빈 값. 잘못된 날짜는 빈 문자열                                                   | 속성 종류를 텍스트에서 날짜로 바꾸면 값이 그대로 남는다                                             |
| 상대 시각의 달·해 경계      | moment의 `fromNow`는 달력의 달로 센다                                                                                                                                                  | 평균 달 길이로 센다. 45일이나 18개월 근처에서 한 단위 다를 수 있다                                            | `Intl`에는 달력 차이 계산이 없다. 기간(`humanizeDuration`)의 구간은 moment와 같다(반올림한 뒤 비교) |
| 빈 보드 ID로의 이동         | `generatePath`가 예외를 던진다                                                                                                                                                         | 보드 없는 경로(`/team/t`)를 준다                                                                              | 보드 선택을 푸는 경우                                                                               |

### 옮기지 않은 원본 코드

W1에서 옮기지 않은 것이다.

| 원본                                                                                                                                                                                                                                     | 이유                                                                   |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `utils.ts`: `htmlFromMarkdown`, `htmlFromMarkdownWithRenderer`                                                                                                                                                                           | W4. 본문 저장 형식과 새니타이즈 방식이 정해진 뒤                       |
| `utils.ts`: `htmlToElement`, `getElementById`, `getTextWidth`, `getFontAndPadding*`, `setFavicon`, `replaceUrlQueryParam`, `selectLocalFile`, `copyTextToClipboard`, `isKeyPressed`, `isMac`, `cmdOrCtrlPressed`, `getReadToken`, `uuid` | DOM이나 `window`에 붙은 동작이다. 쓰는 화면을 만드는 단계에서          |
| `utils.ts`: `fixWSData`, `fixBlock`, `fixBoard`                                                                                                                                                                                          | W2(WebSocket 클라이언트). 블록을 맞추는 부분은 `normalizeBlock`이 한다 |
| `utils.ts`: `isMobile`(User-Agent 판별)                                                                                                                                                                                                  | 플랜이 뷰포트와 포인터 종류로 판별하기로 했다                          |
| `utils.ts`: `isDesktopApp`, `getDesktopVersion`, `isDesktop`, 옛 경로 판별                                                                                                                                                               | 네이티브 앱과 플러그인 전용. 범위 밖                                   |
| `utils.ts`: `isSystemAdmin`, `isTeamAdmin`, `isAdmin`, `spaceSeparatedStringIncludes`                                                                                                                                                    | 플러그인의 역할 문자열. 관리자 개념은 S6에서 새로 정한다               |
| `utils.ts`: `generateClassName`, `getProfilePicture`, `sleep`, `log`·`logWarn`                                                                                                                                                           | React 전용이거나 쓸 곳이 없다                                          |
| `properties/types.tsx`: `valueLength`, `valueClassName`                                                                                                                                                                                  | 테이블의 열 너비 자동 맞춤(캔버스 측정)과 CSS 클래스. W3               |
| `components/calculations/options.tsx`: 계산 이름의 번역                                                                                                                                                                                  | 메뉴 문구다. W3                                                        |
| `csvExporter.ts`: `exportTableCsv`(링크를 만들어 누르는 부분)                                                                                                                                                                            | W5                                                                     |
| `store/cards.ts`의 나머지(Redux 슬라이스와 선택자)                                                                                                                                                                                       | W2(상태)                                                               |

`compareVersions`는 옮겼지만 원본에서 쓰는 곳이 네이티브 앱 판별뿐이다. 쓸 곳이 생기지 않으면 지운다.

### 테스트

| 구분                                  | 건수 | 파일                                                                                                                                                                                                                                                                            |
| ------------------------------------- | ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 원본 `.test.ts`에서 옮김              | 128  | `filter.test.ts` 64, `utils.test.ts` 28, `wire.test.ts` 19, `content-order.test.ts` 12, `undo.test.ts` 3, `block-tree.test.ts` 2                                                                                                                                                |
| 원본 `calculations.test.tsx`에서 옮김 | 100  | `calculations.test.ts` (원본의 반복문이 만드는 케이스 포함, 전부)                                                                                                                                                                                                               |
| 새로 씀                               | 226  | `drop-target.test.ts` 55, `views.test.ts` 33, `values.test.ts` 25, `sort.test.ts` 21, `time-zones.test.ts` 20, `edges.test.ts` 19, `filter-rules.test.ts` 18, `wire-shapes.test.ts` 16, `regional.test.ts` 9, `sort-properties.test.ts` 7(속성 기반), `wire-boundary.test.ts` 3 |

원본 139케이스의 행방: 옮김 128, W2로 넘김 6(`octoClient` 4, `mutator` 2), W4로 넘김 3(`htmlFromMarkdown`), 옮기지 않음 2(텔레메트리 1, 네이티브 앱의 `htmlFromMarkdown` 1). 지시받은 132건보다 4건 적다. `htmlFromMarkdown`의 네 케이스는 marked로 만든 HTML 문자열(`onclick=""` 속성 포함)을 그대로 기대값으로 삼는데, 포트의 마크다운 표시는 W4에서 새니타이즈 방식과 함께 정하기로 되어 있어 지금 옮기면 버릴 구현을 고정하게 된다.

옮긴 방법: 케이스의 이름과 단언은 그대로 두고 호출만 새 함수로 바꿨다. `card.fields.properties.x = v` 같은 직접 대입은 `setCardProperty(card, 'x', v)`로 바꿨다. 스냅샷(`toMatchSnapshot`)을 쓰던 11케이스는 원본 `.snap` 파일의 값을 `toEqual`의 기대값으로 풀어 적었다. 아래 "테스트 매핑 (원본 139케이스)"의 표는 스크립트가 원본 파일에서 케이스 이름을 읽고 vitest의 결과(JSON)에서 같은 이름의 통과한 케이스를 찾아 만든 것이다(128건 모두 찾음).

- Q16을 고치기 전에는 `Pacific/Kiritimati`에서 원본에서 옮긴 3건이 실패했다.
- 원본에서 옮긴 케이스 가운데 기기의 현지 시각으로 날짜를 만들던 것(`cardFilter`의 날짜 케이스, `utils`의 날짜 표시 6건)은 이름과 단언은 그대로 두었다. 보는 사람을 `Asia/Seoul`로 정해 그 시간대의 시각으로 입력을 만든다(`tests/support/zones.ts`의 `at`).
- 원본에서 옮긴 `filter.test.ts`의 날짜 케이스는 이름과 단언은 그대로 두고 입력(카드의 값과 필터의 값)만 새 와이어 모양으로 바꿨다.
- 속성 기반 테스트의 같은 검사에 원본의 수동 정렬 비교자를 넣으면 실패한다. 그것도 테스트로 둔다.

### 독립 검토 (2026-11-10 18:05 KST)

포트를 쓴 뒤 별도의 에이전트(Claude Fable 5.2, 읽기 전용)에게 모듈마다 원본과 나란히 읽고 같은 입력에서 결과가 달라지는 곳을 찾게 했다. 정상적인 데이터에서의 차이는 나오지 않았고, 형식이 어긋난 값과 경계값에서 14건이 나왔다.

- 고친 것(원본과 같게): 값 목록이 없는 필터 조건에서의 예외, `properties`가 없는 보드의 패치 생성 예외, `null`인 속성 값을 검색할 때의 예외, `"null"`인 날짜 값을 표시할 때의 예외, 기간 문구의 반올림 구간(44.5–45초 등 네 곳), 만든 사람이 빈 그룹이 숨김 목록의 `''` 때문에 숨겨지던 것, 최소·최대가 무한대 값을 `0`으로 바꾸던 것. 날짜 계산이 `Math.min(...목록)`을 쓰던 것도 반복문으로 바꿨다(카드가 아주 많으면 인자 개수 한도에 걸린다).
- 원본과 다르게 둔 것: 위 "원본과 다르게 한 것" 표의 아래 다섯 행.
- 그대로 둔 것: `arrayMove`에 범위 밖 위치를 주면 원본은 `undefined`를 끼워 넣고 포트는 아무것도 하지 않는다(원본의 호출부는 유효한 위치만 준다). 사람별 그룹의 순서는 원본이 객체 키 순서라 사용자 ID가 정수 모양이면 숫자순이 되는데 포트는 처음 나온 순서다(ID는 정수 모양이 아니다).

확인하지 못한 것:

- 원본을 실행해 비교한 것은 아니다. 원본 쪽 동작은 소스를 읽어 판단했고, moment·react-router·react-intl의 동작만 원본 `webapp/node_modules`의 것을 실행해 확인했다.
- `countCheckboxesInMarkdown`을 marked와 나란히 돌려 보지 않았다. 정규식으로 보면 네 칸 넘게 들여쓴 항목을 세고(marked는 코드로 볼 수 있다) 인용문 안의 항목(`> - [ ] x`)은 세지 않는다.
- `fillPath`와 `generatePath`의 차이 가운데 URL 인코딩이 필요한 값.

### 원본의 결함 Q1–Q16

사용자 결정(2026-11-10): 호환을 끝냈으므로 보존하지 않고 전부 고친다. 2026-11-10 18:55 KST에 모두 고쳤다. 코드와 테스트의 `QUIRK` 표시는 지웠다. 고친 뒤의 동작과 그것을 고정하는 테스트는 위 "정렬, 필터, 검색, 계산의 규칙"에 같은 번호로 있다.

| #   | 원본의 동작                                                                       | 상태                |
| --- | --------------------------------------------------------------------------------- | ------------------- |
| Q1  | 수동 정렬 비교자가 비대칭(위치 없는 카드와 있는 카드를 어느 순서로 비교해도 "뒤") | **고침**            |
| Q2  | 텍스트 필터가 제목만 대소문자를 무시하고 속성 값은 구분                           | **고침**            |
| Q3  | 검색에서 여러 값 선택 속성만 선택지 이름 전체가 같아야 맞음                       | **고침**            |
| Q4  | 사람별 그룹에서 사람 없는 카드가 `undefined`라는 그룹이 됨                        | **고침**            |
| Q5  | 날짜 `is` 필터가 밀리초까지 같아야 맞음                                           | **고침**            |
| Q6  | 정렬 옵션이 둘 이상이면 마지막 것만 보임                                          | **고침**            |
| Q7  | 파일 이름 정리에서 금지 문자가 종류마다 첫 번째만 지워짐                          | **고침**            |
| Q8  | base path가 있으면 `buildURL`의 상대 결과에 앞 슬래시가 없음                      | **고침**            |
| Q9  | 체크박스 계산이 값 `false`를 체크로 셈                                            | **고침**            |
| Q10 | 숫자만 적힌 옛 형식의 날짜 값을 일부만 읽음                                       | **고침(지원 중단)** |
| Q11 | 달력 날짜를 저장할 때 시만 12로 맞추고 분·초가 남음                               | **고침**            |
| Q12 | CSV 머리글 행을 따옴표로 감싸지 않음                                              | **고침**            |
| Q13 | 필터 속성을 사람 종류로 바꾸면 조건이 늘 `includes`로 돌아감                      | **고침**            |
| Q14 | 버려도 되는 변경의 체크포인트가, 앞선 변경이 하나뿐이면 0                         | **고침**            |
| Q15 | 카드가 없을 때 체크 비율이 `NaN%`                                                 | **고침**            |
| Q16 | 생성·수정 시각 필터의 "하루"가 UTC 날짜                                           | **고침**            |

Q를 고치면서 함께 달라진 것:

- 원본은 제목 정렬을 뒤집으면 제목 없는 카드가 앞으로 왔다.
- 원본은 한 사람 속성을 ID로 정렬했다(여러 사람 속성과 만든 사람은 원본도 이름순).
- 실행 취소 관리자: 원본은 `undo`나 `redo`가 예외를 던지면 그 뒤로 기록과 실행 취소가 영영 무시된다.
- 날짜 값의 와이어: 앞서 적었던 안(시각을 RFC 3339 문자열로)은 채택되지 않았다. 옛 문자열 형식(`"{\"from\":1772712000000}"`)은 더는 읽지 않는다.

결함은 아니지만 알아 둘 것:

- `compareVersions`는 두 번째 인자가 더 새 판이면 1을 준다(정렬 비교자와 부호가 반대). 원본의 주석대로이고 원본 테스트 3건이 고정한다.
- 원본 `cardFilter.test.ts`의 `Person properties` 묶음은 이름과 내용이 완전히 같은 케이스가 둘이고, 이름은 "isEmpty clause"인데 조건은 `is`다. 그대로 옮겼다.
- `hydrateBlock`에는 `list-item`, `quote`, `video` 분기가 없다(원본의 새 편집기용 종류). 포트의 `normalizeBlock`도 같아서, 그런 블록은 보고하고 일반 블록으로 둔다.
- `createPatchesFromBlocks`는 값을 동일성으로 비교한다. 카드를 `createCard`로 복사하면 `properties` 객체가 새로 만들어지므로 패치에 `properties` 전체가 들어간다. 원본도 그렇게 보낸다.
- `propertiesThatMeetFilterGroup`은 중첩 그룹을 보지 않고, `or` 그룹에서는 첫 조건만 본다(원본에 TODO로 남아 있다).

### 속성 편집기

- 날짜의 시각 입력은 원본에 없던 기능이다.
- 종류별로 원본과 다른 점은 `docs/properties-test-mapping.md`의 "대응(동작 다름)" 12건에 이유와 함께 있다. 큰 것은 이렇다.
  - 원본은 이메일·숫자 검증을 저장된 값에 해서, 빈 이메일 속성에 값을 넣을 수 없었다.
  - 선택지 이름 바꾸기는 원본에 없었다.
- 수정 시각: 원본은 마지막 댓글·콘텐츠까지 본다.

### 테스트 매핑: 속성 편집기 (원본 렌더 테스트 77케이스)

원본의 속성 값 편집에 관한 렌더 테스트 15개 파일(`webapp/src/properties/**/*.test.tsx` 11개, `propertyValueElement`, `personSelector`, `cardDetailProperties`, `propertyMenu`)의 77케이스를 케이스 단위로 읽어 새 테스트에 대응시킨 표가 `docs/properties-test-mapping.md`에 있다(표는 스크립트가 만들었고 새 케이스 이름이 실제로 통과한 케이스인지 대조했다).

| 상태                                                                                               | 건수 |
| -------------------------------------------------------------------------------------------------- | ---: |
| 대응: 같은 조작과 같은 결과                                                                        |   27 |
| 대응: 같은 조작, 결과가 원본과 다름(이유를 적음)                                                   |   12 |
| 대응: 원본은 스냅샷뿐, 같은 상태를 단언으로                                                        |   19 |
| 옮기지 않음: 없앤 기능(손님 배지)                                                                  |    2 |
| 옮기지 않음: 범위 밖(`propertyMenu` 6, `cardDetailProperties` 8, `confirmPerson` 2, 필터의 "나" 1) |   17 |

범위 밖 17건은 속성 정의를 고치는 메뉴와 카드 상세의 조립에 관한 것이다. 카드 상세를 만든 뒤의 대응은 `docs/card-test-mapping.md`에 있다.

### 테스트 매핑 (원본 139케이스)

`calculations.test.tsx`는 139케이스 밖이라 이 표에 없다. 거기서 옮긴 100건 가운데 `countChecked for cards`, `countUnchecked for cards`, `percentChecked for cards`, `percentUnchecked for cards` 네 건은 Q9를 고치면서 기대값을 새 동작으로 바꿨다.

| #   | 원본 파일                                                 | 원본 케이스                                                                                                                                                        | 새 테스트 파일                      | 상태                                                               |
| --- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------- | ------------------------------------------------------------------ |
| 1   | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method › should be true with isNotEmpty clause                                                                                 | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 2   | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method › should be false with isEmpty clause                                                                                   | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 3   | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method › should be true with includes clause                                                                                   | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 4   | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method › should be true with includes and no values clauses                                                                    | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 5   | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method › should be false with notIncludes clause                                                                               | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 6   | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method › should be true with notIncludes and no values clauses                                                                 | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 7   | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - person property › should be true with isNotEmpty clause                                                               | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 8   | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - person property › should be false with isEmpty clause                                                                 | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 9   | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - person property › verify empty includes clause                                                                        | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 10  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - person property › verify includes clause                                                                              | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 11  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - person property › verify includes clause multiple values                                                              | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 12  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - person property › verify not includes clause                                                                          | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 13  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - multi-person property › should be true with isNotEmpty clause                                                         | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 14  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - multi-person property › should be false with isEmpty clause                                                           | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 15  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - multi-person property › verify empty includes clause                                                                  | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 16  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - multi-person property › verify includes clause                                                                        | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 17  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - multi-person property › verify includes clause 2                                                                      | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 18  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - multi-person property › verify includes clause multiple values                                                        | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 19  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - multi-person property › verify includes clause multiple values 2                                                      | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 20  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - multi-person property › verify not includes clause                                                                    | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 21  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - multi-person property › verify not includes clause, multiple values                                                   | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 22  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - (createdBy) person property › verify empty includes clause                                                            | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 23  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - (createdBy) person property › verify includes clause                                                                  | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 24  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - (createdBy) person property › verify includes clause multiple values                                                  | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 25  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - (createdBy) person property › verify not includes clause                                                              | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 26  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - single date property › should be true with isSet clause                                                               | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 27  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - single date property › should be false with notSet clause                                                             | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 28  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - single date property › verify isBefore clause                                                                         | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 29  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - single date property › verify isAfter clauses                                                                         | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 30  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - single date property › verify is clause                                                                               | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 31  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - date range property › verify isBefore clause                                                                          | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 32  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - date range property › verify isAfter clauses                                                                          | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 33  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - date range property › verify is clause                                                                                | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 34  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - (createdTime) date property › should be true with isSet clause                                                        | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 35  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - (createdTime) date property › should be false with notSet clause                                                      | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 36  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - (createdTime) date property › verify isBefore clause                                                                  | `tests/logic/filter.test.ts`        | 옮김 (Q16: 케이스는 그대로, 새 동작에서 모든 시간대 통과)          |
| 37  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - (createdTime) date property › verify isAfter clauses                                                                  | `tests/logic/filter.test.ts`        | 옮김 (Q16: 케이스는 그대로, 새 동작에서 모든 시간대 통과)          |
| 38  | `cardFilter.test.ts`                                      | src/cardFilter › verify isClauseMet method - (createdTime) date property › verify is clause                                                                        | `tests/logic/filter.test.ts`        | 옮김 (Q16: 케이스는 그대로, 새 동작에서 모든 시간대 통과)          |
| 39  | `cardFilter.test.ts`                                      | src/cardFilter › verify isFilterGroupMet method › should return true with no filter                                                                                | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 40  | `cardFilter.test.ts`                                      | src/cardFilter › verify isFilterGroupMet method › should return true with or operation and 2 filterCause, one is false                                             | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 41  | `cardFilter.test.ts`                                      | src/cardFilter › verify isFilterGroupMet method › should return true with or operation and 2 filterCause, 1 filtergroup in filtergroup, one filterClause is false  | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 42  | `cardFilter.test.ts`                                      | src/cardFilter › verify isFilterGroupMet method › should return false with or operation and two filterCause, two are false                                         | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 43  | `cardFilter.test.ts`                                      | src/cardFilter › verify isFilterGroupMet method › should return false with and operation and 2 filterCause, one is false                                           | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 44  | `cardFilter.test.ts`                                      | src/cardFilter › verify isFilterGroupMet method › should return true with and operation and 2 filterCause, two are true                                            | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 45  | `cardFilter.test.ts`                                      | src/cardFilter › verify isFilterGroupMet method › should return true with or operation and 2 filterCause, 1 filtergroup in filtergroup, one filterClause is false  | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 46  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertyThatMeetsFilterClause method › should return Utils.assertFailure and filterClause propertyId                                       | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 47  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertyThatMeetsFilterClause method › should return filterClause propertyId with non-select template and isNotEmpty clause                | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 48  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertyThatMeetsFilterClause method › should return filterClause propertyId with select template , an option and isNotEmpty clause        | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 49  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertyThatMeetsFilterClause method › should return filterClause propertyId with select template , no option and isNotEmpty clause        | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 50  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertyThatMeetsFilterClause method › should return filterClause propertyId with template, and includes clause with values                | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 51  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertyThatMeetsFilterClause method › should return filterClause propertyId with template, and includes clause with no values             | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 52  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertyThatMeetsFilterClause method › should return filterClause propertyId with template, and notIncludes clause                         | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 53  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertyThatMeetsFilterClause method › should return filterClause propertyId with template, and isEmpty clause                             | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 54  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertyThatMeetsFilterClause method - Person properties › should return filterClause propertyId with template, and isEmpty clause         | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 55  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertyThatMeetsFilterClause method - Person properties › should return filterClause propertyId with template, and isEmpty clause         | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 56  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertiesThatMeetFilterGroup method › should return {} with undefined filterGroup                                                         | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 57  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertiesThatMeetFilterGroup method › should return {} with filterGroup without filter                                                    | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 58  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertiesThatMeetFilterGroup method › should return {} with filterGroup, or operation and no template                                     | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 59  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertiesThatMeetFilterGroup method › should return a result with filterGroup, or operation and template                                  | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 60  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertiesThatMeetFilterGroup method › should return {} with filterGroup, and operation and no template                                    | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 61  | `cardFilter.test.ts`                                      | src/cardFilter › verify propertiesThatMeetFilterGroup method › should return a result with filterGroup, and operation and template                                 | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 62  | `cardFilter.test.ts`                                      | src/cardFilter › verify applyFilterGroup method › should return array with card1                                                                                   | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 63  | `cardFilter.test.ts`                                      | src/cardFilter › verfiy applyFilterGroup method for case-sensitive search › should return array with card1 when search by test as Card1                            | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 64  | `cardFilter.test.ts`                                      | src/cardFilter › verify applyFilter for title › should not return array with card1                                                                                 | `tests/logic/filter.test.ts`        | 옮김                                                               |
| 65  | `utils.test.ts`                                           | utils › assureProtocol › should passthrough on valid short protocol                                                                                                | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 66  | `utils.test.ts`                                           | utils › assureProtocol › should passthrough on valid long protocol                                                                                                 | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 67  | `utils.test.ts`                                           | utils › assureProtocol › should passthrough on valid short protocol                                                                                                | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 68  | `utils.test.ts`                                           | utils › assureProtocol › should add a https for empty protocol                                                                                                     | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 69  | `utils.test.ts`                                           | utils › createGuid › should create 27 char random id for workspace                                                                                                 | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 70  | `utils.test.ts`                                           | utils › createGuid › should create 27 char random id for board                                                                                                     | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 71  | `utils.test.ts`                                           | utils › createGuid › should create 27 char random id for card                                                                                                      | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 72  | `utils.test.ts`                                           | utils › createGuid › should create 27 char random id                                                                                                               | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 73  | `utils.test.ts`                                           | utils › htmlFromMarkdown › should not allow XSS on links href on the webapp                                                                                        | —                                   | W4: 마크다운 표시와 새니타이즈                                     |
| 74  | `utils.test.ts`                                           | utils › htmlFromMarkdown › should not allow XSS on links href on the desktop app                                                                                   | —                                   | 옮기지 않음: 네이티브 앱 브리지(`openInNewBrowser`)는 포팅 범위 밖 |
| 75  | `utils.test.ts`                                           | utils › htmlFromMarkdown › should encode links                                                                                                                     | —                                   | W4: 마크다운 표시와 새니타이즈                                     |
| 76  | `utils.test.ts`                                           | utils › htmlFromMarkdown › should not double encode title and href                                                                                                 | —                                   | W4: 마크다운 표시와 새니타이즈                                     |
| 77  | `utils.test.ts`                                           | utils › countCheckboxesInMarkdown › should count checkboxes                                                                                                        | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 78  | `utils.test.ts`                                           | utils › test - buildURL › buildURL, no base                                                                                                                        | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 79  | `utils.test.ts`                                           | utils › test - buildURL › buildURL, base no slash                                                                                                                  | `tests/logic/utils.test.ts`         | 옮김 (Q8: 새 동작으로 기대값을 고침)                               |
| 80  | `utils.test.ts`                                           | utils › test - buildURL › buildUrl, base with slash                                                                                                                | `tests/logic/utils.test.ts`         | 옮김 (Q8: 새 동작으로 기대값을 고침)                               |
| 81  | `utils.test.ts`                                           | utils › display date › should show month and day for current year                                                                                                  | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 82  | `utils.test.ts`                                           | utils › display date › should show month, day and year for previous year                                                                                           | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 83  | `utils.test.ts`                                           | utils › input date › should show mm/dd/yyyy for current year                                                                                                       | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 84  | `utils.test.ts`                                           | utils › input date › should show dd/mm/yyyy for current year, es local                                                                                             | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 85  | `utils.test.ts`                                           | utils › display date and time › should show month, day and time for current year                                                                                   | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 86  | `utils.test.ts`                                           | utils › display date and time › should show month, day, year and time for previous year                                                                            | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 87  | `utils.test.ts`                                           | utils › compare versions › should return one if b > a                                                                                                              | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 88  | `utils.test.ts`                                           | utils › compare versions › should return zero if a = b                                                                                                             | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 89  | `utils.test.ts`                                           | utils › compare versions › should return minus one if b < a                                                                                                        | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 90  | `utils.test.ts`                                           | utils › showBoard test › should switch boards                                                                                                                      | `tests/logic/utils.test.ts`         | 옮김                                                               |
| 91  | `utils.test.ts`                                           | utils › getUserDisplayName test › should display username, by default                                                                                              | `tests/logic/utils.test.ts`         | 지움: 이름은 보일 이름 하나(`logic/users.ts`)                      |
| 92  | `utils.test.ts`                                           | utils › getUserDisplayName test › should display nickname                                                                                                          | `tests/logic/utils.test.ts`         | 지움: 이름은 보일 이름 하나(`logic/users.ts`)                      |
| 93  | `utils.test.ts`                                           | utils › getUserDisplayName test › should display fullname                                                                                                          | `tests/logic/utils.test.ts`         | 지움: 이름은 보일 이름 하나(`logic/users.ts`)                      |
| 94  | `utils.test.ts`                                           | utils › getUserDisplayName test › should display username                                                                                                          | `tests/logic/utils.test.ts`         | 지움: 이름은 보일 이름 하나(`logic/users.ts`)                      |
| 95  | `utils.test.ts`                                           | utils › getUserDisplayName test › should display full name, no nickname                                                                                            | `tests/logic/utils.test.ts`         | 지움: 이름은 보일 이름 하나(`logic/users.ts`)                      |
| 96  | `utils.test.ts`                                           | utils › getUserDisplayName test › should display username, no nickname, no full name                                                                               | `tests/logic/utils.test.ts`         | 지움: 이름은 보일 이름 하나(`logic/users.ts`)                      |
| 97  | `blocks/block.test.ts`                                    | block tests › correctly generate patches from two blocks › should generate two empty patches for the same block                                                    | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 98  | `blocks/block.test.ts`                                    | block tests › correctly generate patches from two blocks › should add fields on the new fields added and remove it in the undo                                     | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 99  | `blocks/block.test.ts`                                    | block tests › correctly generate patches from two blocks › should remove field on the new block added and add it again in the undo                                 | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 100 | `blocks/block.test.ts`                                    | block tests › correctly generate patches from two blocks › should update propertie on the main object and revert it back on the undo                               | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 101 | `blocks/board.test.ts`                                    | board tests › correctly generate patches from two boards › should generate two empty patches for the same board                                                    | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 102 | `blocks/board.test.ts`                                    | board tests › correctly generate patches from two boards › should add properties on the update patch and remove them on the undo                                   | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 103 | `blocks/board.test.ts`                                    | board tests › correctly generate patches from two boards › should add card properties on the redo and remove them on the undo                                      | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 104 | `blocks/board.test.ts`                                    | board tests › correctly generate patches from two boards › should add card properties on the redo and undo if they exists in both, but differ                      | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 105 | `blocks/board.test.ts`                                    | board tests › correctly generate patches from two boards › should add card properties on the redo and undo if they exists in both, but their options are different | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 106 | `blocks/board.test.ts`                                    | board tests › correctly generate patches for boards and blocks › should generate two empty patches for the same board and block                                    | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 107 | `blocks/board.test.ts`                                    | board tests › correctly generate patches for boards and blocks › should add fields on update and remove it in the undo                                             | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 108 | `blocks/filterClause.test.ts`                             | filterClause tests › create filter clause                                                                                                                          | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 109 | `blocks/filterClause.test.ts`                             | filterClause tests › test filter clauses are equal                                                                                                                 | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 110 | `blocks/filterClause.test.ts`                             | filterClause tests › test filter clauses are Not equal property ID                                                                                                 | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 111 | `blocks/filterClause.test.ts`                             | filterClause tests › test filter clauses are Not equal condition                                                                                                   | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 112 | `blocks/filterClause.test.ts`                             | filterClause tests › test filter clauses are Not equal values                                                                                                      | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 113 | `blocks/boardView.test.ts`                                | boardView: sort with ASCII                                                                                                                                         | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 114 | `blocks/boardView.test.ts`                                | boardView: sort with leading emoji                                                                                                                                 | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 115 | `blocks/boardView.test.ts`                                | boardView: sort with non-latin characters                                                                                                                          | `tests/logic/wire.test.ts`          | 옮김                                                               |
| 116 | `components/cardDetail/cardDetailContentsUtility.test.ts` | components/cardDetail/cardDetailContentsUtility › Testing moving first item in the row to the end                                                                  | `tests/logic/content-order.test.ts` | 옮김                                                               |
| 117 | `components/cardDetail/cardDetailContentsUtility.test.ts` | components/cardDetail/cardDetailContentsUtility › Testing moving last item in the row to the beginning                                                             | `tests/logic/content-order.test.ts` | 옮김                                                               |
| 118 | `components/cardDetail/cardDetailContentsUtility.test.ts` | components/cardDetail/cardDetailContentsUtility › Testing moving item from beginning of row to the middle of row                                                   | `tests/logic/content-order.test.ts` | 옮김                                                               |
| 119 | `components/cardDetail/cardDetailContentsUtility.test.ts` | components/cardDetail/cardDetailContentsUtility › Testing swapping two items in the same row by moving second item to the left of the first item                   | `tests/logic/content-order.test.ts` | 옮김                                                               |
| 120 | `components/cardDetail/cardDetailContentsUtility.test.ts` | components/cardDetail/cardDetailContentsUtility › Testing swapping two items in the same row by moving the first item to the right of the second item              | `tests/logic/content-order.test.ts` | 옮김                                                               |
| 121 | `components/cardDetail/cardDetailContentsUtility.test.ts` | components/cardDetail/cardDetailContentsUtility › Testing moving a single item in the first row into the middle of second row using left operation                 | `tests/logic/content-order.test.ts` | 옮김                                                               |
| 122 | `components/cardDetail/cardDetailContentsUtility.test.ts` | components/cardDetail/cardDetailContentsUtility › Testing moving a single item in the first row into the middle of second row using right operation                | `tests/logic/content-order.test.ts` | 옮김                                                               |
| 123 | `components/cardDetail/cardDetailContentsUtility.test.ts` | components/cardDetail/cardDetailContentsUtility › Testing moving a single item in the last row into the middle of first row                                        | `tests/logic/content-order.test.ts` | 옮김                                                               |
| 124 | `components/cardDetail/cardDetailContentsUtility.test.ts` | components/cardDetail/cardDetailContentsUtility › Testing moving a single item in the last row above the first row                                                 | `tests/logic/content-order.test.ts` | 옮김                                                               |
| 125 | `components/cardDetail/cardDetailContentsUtility.test.ts` | components/cardDetail/cardDetailContentsUtility › Testing moving an item out of a row                                                                              | `tests/logic/content-order.test.ts` | 옮김                                                               |
| 126 | `components/cardDetail/cardDetailContentsUtility.test.ts` | components/cardDetail/cardDetailContentsUtility › Testing moving an item out of a row and creating a new row with a single item at the end                         | `tests/logic/content-order.test.ts` | 옮김                                                               |
| 127 | `components/cardDetail/cardDetailContentsUtility.test.ts` | components/cardDetail/cardDetailContentsUtility › Testing moving an item out of a row and creating a new row with a single item at the beginning                   | `tests/logic/content-order.test.ts` | 옮김                                                               |
| 128 | `undoManager.test.ts`                                     | Basic undo/redo                                                                                                                                                    | `tests/logic/undo.test.ts`          | 옮김                                                               |
| 129 | `undoManager.test.ts`                                     | Basic undo/redo response dependant                                                                                                                                 | `tests/logic/undo.test.ts`          | 옮김                                                               |
| 130 | `undoManager.test.ts`                                     | Grouped undo/redo                                                                                                                                                  | `tests/logic/undo.test.ts`          | 옮김                                                               |
| 131 | `octoUtils.test.ts`                                       | duplicateBlockTree: Card                                                                                                                                           | `tests/logic/block-tree.test.ts`    | 옮김                                                               |
| 132 | `octoUtils.test.ts`                                       | filterConditionValidOrDefault                                                                                                                                      | `tests/logic/block-tree.test.ts`    | 옮김                                                               |
| 133 | `octoClient.test.ts`                                      | OctoClient: get blocks                                                                                                                                             | —                                   | W2                                                                 |
| 134 | `octoClient.test.ts`                                      | OctoClient: insert blocks                                                                                                                                          | —                                   | W2                                                                 |
| 135 | `octoClient.test.ts`                                      | OctoClient: importFullArchive                                                                                                                                      | —                                   | W2                                                                 |
| 136 | `octoClient.test.ts`                                      | OctoClient: GetFileInfo                                                                                                                                            | —                                   | W2                                                                 |
| 137 | `mutator.test.ts`                                         | Mutator › changePropertyValue                                                                                                                                      | —                                   | W2                                                                 |
| 138 | `mutator.test.ts`                                         | Mutator › duplicateCard                                                                                                                                            | —                                   | W2                                                                 |
| 139 | `telemetry/telemetryClient.test.ts`                       | trackEvent › should call Rudder\                                                                                                                                   | —                                   | 옮기지 않음: 텔레메트리는 포팅 범위 밖                             |

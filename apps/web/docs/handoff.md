# 인계: 웹앱 트랙 (W0, W1, 스파이크까지)

작성: 2026-11-10 19:40 KST. 브랜치 `fvoci/web`, 기준 커밋 `a038f06b`. 푸시한 것은 없다.

다른 문서에 없는 것만 적는다. 이미 적힌 것은 위치만 가리킨다.

| 알고 싶은 것                                                | 있는 곳                            |
| ----------------------------------------------------------- | ---------------------------------- |
| 무엇을 왜 그렇게 했는가, 고정한 판, 미결 사항               | `PORTING.md`                       |
| 원본 결함 Q1–Q16의 새 동작, 날짜와 시간대 규칙              | `PORTING.md`의 "W1. 순수 로직"     |
| 글꼴 도구와 Chromium에서 확인한 규칙, 두 대안의 측정과 권고 | `PORTING.md`의 "글꼴", "글꼴 측정" |
| shadcn-svelte 생성 명령, CLI가 바꾼 것, 손댄 파일           | `src/lib/ui/README.md`             |
| 편집기 스파이크                                             | `docs/spike-editor.md`             |
| 드래그 스파이크                                             | `docs/spike-drag.md`               |
| 번역 변환                                                   | `docs/i18n-conversion.md`          |
| 스파이크를 두는 법                                          | `spikes/README.md`                 |

결정을 기다리는 것(글꼴 두 권고, 본문 한도, 드래그의 질문들, 번역의 "Boards" 표기)은 각 문서의 미결 절에 있다. 서버 쪽에 전달할 것은 `PORTING.md`의 "날짜 속성 값의 와이어 모양"과 "보는 사람의 시간대"의 마지막 항목, `docs/spike-editor.md` 전체다.

## 도구가 있는 곳

- **Node**: 시스템의 것(v26.13.0). `.ts`를 그대로 실행한다.
- **pnpm**: 시스템에 없다. 워크트리의 `target/tools/node`에 설치해 두었다. 명령마다 앞에 붙인다.

  ```bash
  cd <워크트리 루트>
  export PATH=$PWD/target/tools/node/node_modules/.bin:$PATH
  ```

  `target/`은 커밋되지 않는다. 폴더가 없으면 `PORTING.md`의 "검증 명령" 마지막 줄대로 다시 설치한다.

- **브라우저**: Playwright의 Chromium headless shell 하나뿐이다(`~/.cache/ms-playwright/chromium_headless_shell-1254`, ffmpeg 포함 288 MB). Firefox와 WebKit은 받지 않았다.
- **에이전트 세션의 셸**: 명령 사이에 `PATH`가 남지 않고, 작업 폴더는 남기도 하고 돌아가기도 한다. 명령마다 `cd`와 `export PATH`를 다시 적는 것이 안전했다.

## 검증 명령: 순서와 걸리는 시간

워크스페이스 루트에서, 이 순서로 돌린다. 시간은 2026-11-10 19:36 KST에 이 기기에서 잰 것이다(캐시가 있는 상태).

| 순서 | 명령                                    | 시간     | 비고                                                    |
| ---- | --------------------------------------- | -------- | ------------------------------------------------------- |
| 1    | `pnpm install --frozen-lockfile`        | 1초 미만 | `node_modules`가 이미 있을 때                           |
| 2    | `pnpm --filter web run check`           | 43초     | svelte-check와 ESLint(타입 정보 사용)가 대부분          |
| 3    | `pnpm --filter web run test`            | 6초      | 세 시간대, 각 743건. 한 번만이면 `test:once` 2초        |
| 4    | `pnpm --filter web run build`           | 2초      | 글꼴을 다시 만들면 약 2분이 더 든다                     |
| 5    | `pnpm --filter web run e2e`             | 2초      | **`build`가 먼저 있어야 한다.** `dist/`를 서빙해서 본다 |
| 6    | `pnpm --filter web run spikes:e2e`      | 21초     | 개발 서버를 스스로 띄운다. `build`와 무관               |
| 7    | `pnpm --filter web run build-storybook` | 7초      | 필요할 때만                                             |

- `check`, `test`, `build`는 모두 첫 단계로 메시지를 컴파일한다(`src/lib/i18n/paraglide`, 커밋하지 않음). 따로 돌릴 필요는 없다. `vitest`나 `svelte-check`를 `pnpm exec`로 직접 부를 때만, 새 체크아웃이라면 먼저 `pnpm --filter web run i18n`을 한 번 돌린다.
- `check`의 마지막 두 단계는 원본 `webapp/i18n`을 읽는다(`docs/i18n-conversion.md`).

### 글꼴을 다시 만들어야 하는 경우

글꼴 산출물(`src/styles/fonts/`)은 커밋하지 않고 `build`가 만든다. 다시 만드는 때는 다음 셋이다.

- 새 체크아웃이거나 `src/styles/fonts/`가 비었을 때.
- `tools/fonts/` 아래의 **어느 파일이든** 바뀌었을 때(주석 한 줄도). `tools/zip.ts`처럼 그 밖의 파일은 영향이 없다.
- `target/web-fonts/src`(원본 글꼴과 Unihan, 143 MB)가 없으면 내려받기부터 한다. 이때는 네트워크가 필요하고 2분보다 오래 걸린다.

새 체크아웃에서 `build` 없이 `dev`, `spikes`, `storybook`을 띄우면 글꼴 규칙 파일이 없다. `src/lib/fonts.ts`가 glob으로 가져오므로 오류 없이 시스템 글꼴로 보일 것이다(직접 확인하지는 않았다). 먼저 `pnpm --filter web run fonts`를 돌린다.

## 화면을 띄우는 법과 포트

모두 이 기기 안에서만 열린다(앱 개발 서버는 Vite 기본값인 `localhost`, 나머지는 `127.0.0.1`).

| 무엇                | 명령                              | 포트                                                                                          | 주소                               |
| ------------------- | --------------------------------- | --------------------------------------------------------------------------------------------- | ---------------------------------- |
| 앱 개발 서버        | `pnpm --filter web run dev`       | 5173 (Vite 기본. 쓰이고 있으면 다음 번호로 밀린다)                                            | `/`                                |
| 스파이크 개발 서버  | `pnpm --filter web run spikes`    | 5173 고정(`strictPort`). 이미 쓰이고 있으면 밀리지 않고 실패한다. 바꾸려면 `-- --port <번호>` | `/spikes/editor/`, `/spikes/drag/` |
| 스파이크 Playwright | `spikes:e2e`                      | 18320, `FVOCI_SPIKES_PORT`로 바꿈                                                             | 스스로 띄우고 끈다                 |
| 빌드 결과 미리 보기 | `pnpm --filter web run preview`   | 4173, 인자로 바꿈: `node tools/preview.ts <포트> <기본 경로>`                                 | `/`                                |
| 앱 Playwright       | `e2e`                             | 18310                                                                                         | 스스로 띄우고 끈다                 |
| Storybook           | `pnpm --filter web run storybook` | 6006                                                                                          | `/`                                |

- 스파이크 Playwright를 두 곳에서 같이 돌리면 포트가 겹친다. 하위 에이전트에게는 `FVOCI_SPIKES_PORT=18331`처럼 다른 포트를 주었다.
- 실제 Rust 서버로 빌드 결과를 볼 때 쓴 설정이 `target/web-smoke/config.json`에 남아 있다(포트 **18320**, `webpath`가 이 워크트리의 `apps/web/dist`). 서버 트랙 워크트리의 릴리스 바이너리에 `--config`로 넘겨 띄웠다(읽기만 했다). 스파이크 Playwright의 기본 포트와 같으니 둘을 같이 쓰려면 한쪽을 바꾼다. 그 서버는 지금 떠 있지 않다.
- Storybook 화면은 눈으로 확인하지 않았다. `build-storybook`이 통과하는 것만 확인했다.

## 걸렸던 함정

`PORTING.md`의 "이 기기에서 걸린 것"(닫힌 포트, Storybook 빌드, 글꼴 해시), `src/lib/ui/README.md`(CLI가 판 범위를 `^`로 바꾸고 패키지를 더하는 것, `--preset` 없이는 멈추는 것), `PORTING.md`의 "글꼴"(Chromium이 `font-weight` 기술자로 규칙 묶음을 먼저 고르는 것, `unicode-range`를 겹치지 않게 적는 것), `docs/spike-drag.md`의 "스파이크를 하며 걸린 것"에 적힌 것은 되풀이하지 않는다. 아래는 어디에도 적지 않은 것이다.

### 판과 의존성

- `npm view <이름> time.modified`는 그 패키지의 **마지막 변경 시각**이지 최신 판의 공개 시각이 아니다. 이것으로 "24시간이 지났다"고 판단해 pnpm을 한 판 낮게 설치한 적이 있다. `time[<판>]`으로 본다.
- shadcn-svelte CLI를 다시 돌리기 전의 `package.json`과 잠금 파일 사본이 `target/web-package.before-cli.json`, `target/web-lock.before-cli.yaml`에, 생성 직후의 UI 사본이 `target/web-ui-pristine/`에 있다. CLI를 다시 돌리면 같은 방법으로 사본을 뜨고 `diff -r`로 비교한 뒤 `src/lib/ui/README.md`의 표대로 되돌린다.
- 저장소 루트의 `.editorconfig`가 js·json·html을 4칸으로 정한다. `apps/web/.editorconfig`(`root = true`)가 이를 끊는다. 지우면 편집기가 Prettier와 싸운다.

### 시간대

- **`lib/logic`에서 `Date`를 다루는 코드를 쓰면 린트가 먼저 막는다**(`fvoci/no-device-time`). 막히면 규칙을 끄지 말고 시간대를 인자로 받는다. 규칙은 `src/lib/logic/**`에만 걸려 있다. 다른 층(`state`, `features`)에서 기기 시각을 쓰는 것은 린트가 아니라 `tests/tools/layers.test.ts`의 마지막 테스트가 일부(시간대를 묻는 표현 세 가지)만 잡는다. W2에서 층이 늘면 규칙의 적용 범위를 넓힐지 정해야 한다.
- **테스트에서 시각을 만들 때 `new Date(2026, 2, 5)`를 쓰지 않는다.** 테스트 폴더에는 린트 규칙이 걸려 있지 않아 통과하지만, 세 시간대 실행에서 한 곳만 실패하는 테스트가 된다. `tests/support/zones.ts`의 `at('Asia/Seoul', 2026, 3, 5, 9, 0)`이나 `Date.UTC`를 쓴다.
- 세 시간대 실행에서 **한 시간대만 실패하면** 그 테스트나 코드가 프로세스의 시간대를 읽고 있다는 뜻이다. 코드가 틀렸는지 테스트 입력이 틀렸는지부터 가른다. 실패를 한 시간대에서만 다시 보려면 `TZ=Pacific/Kiritimati pnpm exec vitest run <파일>`.
- `DisplayContext.now`와 표시 함수의 인자는 `Date`가 아니라 에포크 밀리초다. "올해" 판단이 `now`와 시간대에 달려 있으므로, 기대값에 연도가 붙고 안 붙고를 검사하는 테스트는 `now`를 반드시 넘긴다(안 넘기면 해가 바뀔 때 깨진다). 원본에서 옮긴 `utils.test.ts`의 날짜 표시 6건만은 원본대로 실제 현재 연도를 쓴다.
- 생성·수정 시각의 `dateRange`는 달력 날 수가 아니라 지나간 시간이다. 처음에 반대로 알고 테스트를 썼다가 고쳤다.
- 서머타임 날짜를 테스트에 적을 때는 브라우저(Node)의 시간대 자료를 믿는다. 뉴욕 2026-03-08·2026-11-01, 아바나 2026-03-08, 로드하우 2026-10-04, 사모아 2011-12-30은 이 기기의 Node 26에서 확인한 값이다. 자료가 다른 런타임에서는 달라질 수 있다.

### 테스트 매핑 표

`PORTING.md` 끝의 "테스트 매핑" 표는 스크립트가 만든 것인데, **그 스크립트는 커밋되어 있지 않다**(`target/scratch/mapping.py`, 입력은 `target/scratch/vitest.json`). 원본에서 옮긴 케이스의 이름을 바꾸면 표와 어긋난다. 표를 다시 만들려면 다음과 같이 한다. `target/`을 지웠다면 스크립트를 새로 써야 한다: 원본 테스트 파일에서 `describe`/`it`/`test`의 이름을 들여쓰기로 읽고, vitest JSON 결과에서 같은 이름의 통과한 케이스를 찾는 80줄쯤의 파이썬이다.

```bash
cd apps/web
pnpm exec vitest run --reporter=json --outputFile=../../target/scratch/vitest.json
python3 ../../target/scratch/mapping.py > ../../target/scratch/mapping.md   # 마지막 줄에 건수 요약
```

### 측정에 쓴 일회용 스크립트

측정 스크립트는 남기지 않았다. 다시 재려면 새로 써야 하므로 방법만 적는다.

- **글꼴 크기**: `tools/fonts`의 모듈(`charsets`, `families`, `sfnt`, `unicode-range`)과 `subset-font`를 가져다 `target/web-fonts/src`의 원본에서 부분집합을 만들어 `target/font-lab/`에 썼다(약 2분). 주의 둘: 라틴 가변 글꼴은 `wdth: 100`으로 축을 고정하지 않으면 실제 빌드(112,876 B)보다 크게 나온다(실험에서는 180,268 B가 나왔고 이 수치는 보고에 쓰지 않았다). Unihan 18.0에는 `kFrequency`가 없어서 번체의 빈도 집합은 만들 수 없었다.
- **첫 화면의 글꼴 요청과 글자 모양**: Playwright에서 `context.route`로 존재하지 않는 호스트(`fonts.test`)로 가는 요청을 가로채 글꼴 파일을 물려 주고, `page.setContent`로 `@font-face` 규칙만 바꿔 가며 요청을 셌다. 글자 위치와 폭은 canvas의 `measureText`로 쟀다. 비교 그림이 `target/font-lab/latin-vs-cjk-fonts.png`에 있다. 로컬 포트를 열지 않으므로 닫힌 포트 문제와 무관하다.
- **번들 크기**(편집기): Vite의 가상 진입점으로는 재지 못했다. `apps/web/.size/` 아래에 실제 진입 파일을 잠깐 만들어 빌드하고 지웠다. 점으로 시작하는 폴더라도 **`check` 전에 지워야 한다**(ESLint, Prettier, 이름 검사가 읽는다).
- **템플릿 변환 집계**: `tools/templates/convert.ts`는 커밋되어 있다. 결과물이 `target/templates-converted/`에 있다.

### 그 밖

- `pkill -f "playwright test"`는 그 글자가 명령줄에 들어 있는 자기 셸까지 죽인다. `ps -eo pid,args | grep '[p]laywright'`로 PID를 뽑아 `kill`한다.
- ESLint 종료 코드를 보일 때 `| tail`을 붙이면 `$?`가 `tail`의 것이 된다. 출력을 파일로 받고 `$?`를 따로 찍는다.
- 파일을 문자열 치환 스크립트로 고친 직후 Prettier가 줄을 다시 나누면, 같은 스크립트의 다음 치환이 대상 문자열을 못 찾는다. 치환 전에 실제 글자를 `grep`으로 확인하거나, 치환을 모두 끝낸 뒤 Prettier를 한 번 돌린다.
- 이름 검사(`tools/names/check.ts`)는 `apps/web` 아래 모든 글 파일을 본다. **문서에 주 체크아웃의 절대 경로를 적으면 실패한다**(폴더 이름에 금지 문자열이 들어 있다). 이 문서도 그래서 상대 경로만 쓴다.
- `messages/*.json`을 직접 고치면 `check`가 실패한다. 고칠 곳은 `messages-own/`(이 앱의 키)과 `messages-fill/ko.json`(채운 한국어)이고 `pnpm --filter web run i18n:convert`로 다시 만든다.

## 하위 에이전트를 쓴 방식

세 번 썼다.

| 일                  | 방식                                                                                           | 결과                                                                                                          |
| ------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| W1 로직의 독립 검토 | 읽기 전용. 원본과 포트의 위치, 이미 알고 있는 차이의 목록을 주고 그 밖에 갈리는 곳을 찾게 했다 | 깨진 데이터에서 원본과 갈리는 곳 14군데를 찾았다. 8군데를 고치고 나머지는 `PORTING.md`의 "독립 검토"에 적었다 |
| 드래그 스파이크     | 격리된 워크트리, 커밋 하나                                                                     | `a467a663`으로 체리픽                                                                                         |
| 번역 변환           | 격리된 워크트리, 커밋 하나                                                                     | `4bc863d5`로 체리픽, `package.json`의 `scripts`에서 충돌 한 번                                                |

두 구현 작업은 내가 편집기 스파이크와 Q1–Q16을 하는 동안 나란히 돌았다. 각각 45분, 32분쯤 걸렸다.

주의점:

- **격리 워크트리가 `fvoci/web`의 원하는 커밋에서 만들어진다고 믿지 않는다.** 지시문 첫머리에 "먼저 `git reset --hard <커밋>`으로 옮긴다"와 "첫 줄의 커밋 제목을 확인하고, 그 커밋을 모르면 멈추고 보고한다"를 적었다. 두 에이전트 모두 `1faf53c0`에서 시작하게 했다.
- **갈라진 뒤의 커밋은 에이전트가 모른다.** 드래그 에이전트는 Q16을 고치기 전에서 갈라졌기 때문에 키리티마티 시간대에서 3건이 실패한다고 보고했다(알려 둔 실패였다). 에이전트의 "통과" 보고는 그 기준 커밋에서의 것이므로, 합친 뒤 `check`, `test`, `spikes:e2e`를 내가 다시 돌렸다.
- **겹치는 파일을 지시문에서 나눈다.** 드래그 쪽에는 `package.json`과 `PORTING.md`를 건드리지 말라고 했고, 번역 쪽에는 `package.json`의 `scripts` 블록만 고치라고 했다. 그래도 `scripts`의 `check`와 `test` 두 줄이 내 변경과 부딪혔다. 손으로 합쳤다(`test`는 세 시간대 실행을, `check`에는 번역 검사 두 단계를 남김).
- **에이전트는 이 대화를 보지 못한다.** 다음을 지시문에 직접 적었다: 금지 문자열과 바깥 링크 금지, 판을 범위 없이 고정, 참고 저장소의 코드를 복사하지 않기, 시각은 KST까지, 닫힌 포트에 매달리는 문제와 포트 번호, pnpm의 위치, 커밋 메시지의 끝줄, 푸시 금지.
- **워크트리마다 `pnpm install`이 필요하다.** 번역 쪽 워크트리는 `node_modules`까지 575 MB다.
- **지시하지 않은 것을 더해 온다.** 번역 에이전트는 `check`에 "메시지 파일이 최신인가" 검사를 넣었고(쓸모가 있어 남겼다), 한국어가 원본의 17개에 없다는 것을 찾아 18개로 만들었다. 보고의 "지시와 다른 점"을 읽고 남길지 정한다.
- **보고의 수치는 직접 확인한다.** 키 집합 검사 출력과 채운 한국어 18건은 합친 뒤 내가 다시 뽑아 보았다. 드래그의 프레임 시간 측정(카드 500–5,000장)과 번역 문서의 번들 크기 변화는 다시 재지 않았다.

## 남아 있는 것

### 프로세스

내가 띄운 것 가운데 남은 것은 없다(2026-11-10 19:37 KST에 `ps`로 확인: Vite, Playwright, Storybook, 미리 보기 서버 모두 없음).

- 포트 8002의 `fvoci-server`(16:01 KST 시작, 서버 트랙 워크트리의 설정)는 서버 세션의 것이다. 건드리지 않았다.
- `127.0.0.1:18333`에 듣는 것이 하나 있다. 소유 프로세스를 `ss`로 볼 수 없었고 누가 띄웠는지 모른다. 드래그 에이전트가 쓴 포트는 18331이다. 건드리지 않았다.

### 커밋하지 않은 파일

없다. `git status`가 비어 있다. `apps/web` 아래의 무시되는 산출물은 `dist/`(26 MB), `dist-spikes/`, `storybook-static/`, `test-results/`, `src/styles/fonts/`, `src/lib/i18n/paraglide/`이고 모두 다시 만들 수 있다.

### 캐시와 임시 파일 (`target/`, 커밋되지 않음)

| 경로                                                  | 크기   | 무엇                                                                  | 지워도 되는가                                  |
| ----------------------------------------------------- | ------ | --------------------------------------------------------------------- | ---------------------------------------------- |
| `target/tools/node`                                   | 64 MB  | pnpm 12.18.0                                                          | 지우면 다시 설치                               |
| `target/web-fonts/src`                                | 143 MB | 글꼴 원본과 Unihan(SHA-256 검사를 거친 것)                            | 지우면 다음 글꼴 빌드가 다시 받는다            |
| `target/font-lab`                                     | 8.9 MB | 글꼴 측정의 실험 글꼴과 비교 그림                                     | 글꼴 결정이 난 뒤 지운다                       |
| `target/scratch`                                      | 520 KB | 매핑 스크립트, 마지막 검증 출력(`check.out` 등), 잡다한 측정 스크립트 | `mapping.py`만 위에 적은 이유로 아깝다         |
| `target/templates-converted`                          | 320 KB | 템플릿 13개를 변환한 결과                                             | 된다(`node tools/templates/convert.ts`로 다시) |
| `target/web-smoke`                                    | 6.7 MB | Rust 서버 확인용 설정, DB, 로그                                       | 된다                                           |
| `target/web-ui-pristine`, `target/web-*.before-cli.*` | 1 MB   | CLI 실행 전후의 사본                                                  | 된다                                           |

### 에이전트의 워크트리와 브랜치

주 체크아웃(이 워크트리의 상위 폴더)의 `.claude/worktrees/` 아래에 이번 세션의 두 워크트리가 남아 있다. 커밋은 이미 `fvoci/web`에 체리픽했으므로 내용은 필요 없다. 지우는 것은 사용자의 확인을 받고 한다.

| 워크트리                           | 브랜치                             | 커밋       | 크기   |
| ---------------------------------- | ---------------------------------- | ---------- | ------ |
| `agent-a2f2d188c2efbf14c` (드래그) | `worktree-agent-a2f2d188c2efbf14c` | `1176b34a` | 88 MB  |
| `agent-adafca84768398db1` (번역)   | `worktree-agent-adafca84768398db1` | `510957f6` | 575 MB |

같은 폴더의 다른 `agent-…` 워크트리 열 개는 이 세션이 만든 것이 아니다.

### 세션 밖에 적어 둔 것

사용자 메모리에 이 세션에서 한 건을 더했다: 이 기기에서 닫힌 로컬 포트로의 연결이 매달린다는 것. 다음 세션에 자동으로 실린다.

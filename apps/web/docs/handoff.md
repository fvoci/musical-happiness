# 인계: 웹앱 (화면은 조립됐고, 바뀐 서버에 붙이는 중)

고쳐 쓴 때: 2026-11-11 15:57 KST(브랜치 `fvoci/web-adapt`). 경로는 따로 말하지 않으면 `apps/web` 기준이다.

다른 문서에 없는 것만 적는다. 이미 적힌 것은 위치만 가리킨다.

## 지금 상태

- **화면은 끝까지 조립돼 있고, 바뀐 서버에 붙었다.** 서버가 서빙하는 앱은 서버를 부르는 구현(`src/lib/api/`)으로 돌고, 서버 없이 빌드를 보는 도구(`preview`, `look`)와 개발 서버는 메모리 구현으로 돈다(`docs/decisions.md`의 연결 1번). 서버에 붙은 것: 로그인, 보드 읽기와 열린 보드의 참여, 보드 만들기(첫 뷰와 한 요청)·고치기·지우기·되살리기, 뷰 만들기, 공유(구성원과 역할, 팀에 열기와 최소 역할, 공개 링크, 사람 찾기), 비밀번호 바꾸기, 템플릿(목록, 템플릿에서 보드, 보드에서 템플릿)과 새 계정의 환영 보드, 아카이브(보드와 팀의 내보내기, 가져오기), 보드 검색, 카드 만들기·고치기(속성 단위)·옮기기·지우기, 카드의 본문(409 포함), 댓글 쓰기·고치기·지우기, 첨부(올리기 세션)와 본문의 이미지, 실시간, 사이드바(카테고리, 보드의 자리와 숨기기), 보일 이름(사람은 어디서나 "보일 이름 (@로그인 이름)"), 계정 설정의 언어, 휴지통(사이드바의 "휴지통"과 보드 위 막대의 "지운 카드"). `not_wired`(연결 3번)로 거절하는 조작은 남지 않았다. 장치는 다음 조작을 위해 둔다.
- **계약**은 저장소 루트의 `contracts/`(`openapi.json`, `websocket.md`, `body-vectors.json`)와 `docs/contract-pending.md`다. 아직 열린 가정과 질문, 계약의 빈틈은 `docs/contract-assumptions.md`에 있다.
- **사용자 결정을 기다리는 것**은 `docs/contract-assumptions.md` 1절에 있다.
- **서버와 한 브랜치에 있다.** 저장소는 `fvoci/musical-happiness`(비공개)이고, 브랜치 `master`에 서버(저장소 루트)와 웹앱(`apps/web`)이 함께 있다. 일마다 `master`에서 워크트리를 새로 갈라 일하고, 끝나면 `master`에 합친다. 양쪽에 걸친 안내는 저장소 루트의 `docs/handoff.md` 맨 앞에 있다.
- **올라간 커밋은 고쳐 쓰지 않는다.** 고칠 것은 새 커밋으로 얹는다.

### 바뀐 서버에 맞추는 일: tracer-bullet 목록

브랜치 `fvoci/web-adapt`. 하나는 기능 하나가 화면, 저장소, 서버를 부르는 구현, 실제 서버, 종단 테스트까지 끝에서 끝으로 도는 것이다.

- [x] 1. 새 서버에서 지금 있는 것이 돈다: 속성 단위 패치(지우기 포함), 날짜 값, 두 사람의 동시 고치기
- [x] 2. 카드의 본문을 서버에 쓰고 읽는다(낡은 판의 409, 본문 벡터)
- [x] 3. 댓글을 쓰고 고치고 지운다
- [x] 4. 파일을 올리기 세션으로 올리고, 첨부를 붙이고 내려받는다
- [x] 5. 본문에 이미지를 넣는다
- [x] 6. 열린 보드의 참여 단계
- [x] 7. 익명 독자의 멘션을 "알 수 없는 사용자"로 그리고, 그 본문을 다시 저장하지 않는다
- [x] 8. 서버의 답을 반영한다(카테고리 메시지, 세션이 끝난 뒤의 연결, 답이 난 가정 지우기)

## 어디에 무엇이 있는가

| 알고 싶은 것                                     | 있는 곳                                                                                                          |
| ------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------- |
| 무엇을 왜 그렇게 했는가, 고정한 판, 미결 사항    | `PORTING.md`                                                                                                     |
| 화면 조립에서 한 일의 목록                       | `PORTING.md`의 "화면 조립: 서버의 계약 없이, 메모리 구현 위에서"                                                 |
| 그 전에 한 일의 목록                             | `PORTING.md`의 "서버의 계약을 기다리며 한 것"                                                                    |
| 묻지 않고 스스로 정한 것, 검토에서 남긴 것       | `docs/decisions.md`                                                                                              |
| 서버에 대해 가정한 것과 물을 것                  | `docs/contract-assumptions.md`                                                                                   |
| 모바일 화면 안, 경계 값, 질문 36개와 그 결정     | `docs/mobile-proposal.md`(결정은 "결정" 절), 그림은 `docs/mobile/`                                               |
| 캘린더 뷰의 안, 질문 24개                        | `docs/calendar-proposal.md`, 그림은 `docs/calendar/`                                                             |
| 조립된 화면의 그림                               | 저장소에 든 것은 `docs/screens/<화면>/`. 새로 찍으면 `target/screens/<화면>/`(커밋하지 않는다, `e2e/screens.ts`) |
| 글꼴의 결정과 수치                               | `PORTING.md`의 "글꼴"                                                                                            |
| 날짜 값의 와이어, 지역 설정의 값                 | `PORTING.md`의 "날짜 속성 값의 와이어 모양", "보는 사람의 시간대: 계정 설정"                                     |
| 번역 결정, 한국어 검수 표                        | `docs/i18n-conversion.md`, `docs/ko-review.md`                                                                   |
| 내장 템플릿과 변환이 바꾼 곳                     | 저장소 루트의 `crates/server/assets/templates/`의 `README.md`, `CHANGES.md`                                      |
| 편집기, 읽기 전용 표시기, 번들 크기              | `docs/spike-editor.md`(표시기는 9절, 창 크기는 6.5절, 수식과 다이어그램은 10절)                                  |
| 드래그의 결정 1–11, 공개 형태, 창 크기가 바뀔 때 | `docs/spike-drag.md`                                                                                             |
| 속성 편집기, 카드 상세와 원본 테스트의 대응      | `docs/properties-test-mapping.md`, `docs/card-test-mapping.md`, `src/lib/components/README.md`                   |
| 빌드의 `.zst`, 원본 폴더 없이 도는 것            | `docs/build-notes.md`                                                                                            |
| 화면 크기의 세 단계라는 방침                     | `PORTING.md`의 "화면 크기의 세 단계", 저장소 밖의 계획 `~/.claude/plans/fvoci-web-svelte.md`                     |
| 도구의 위치, 화면을 띄우는 법, 함정              | 이 문서                                                                                                          |

## 결정을 기다리는 것

`docs/contract-assumptions.md` 1절에 있다(서버가 "사용자 결정 필요"로 남긴 것).

## 이 브랜치의 마지막 검증

2026-11-11 16:37–16:52 KST(커밋 `362f1b5`; 그 뒤는 이 표의 고침뿐이고 같은 명령을 다시 돌렸다), 저장소 루트에서 `pnpm --filter web run <명령>`:

| 명령       | 결과                                                                                          |
| ---------- | --------------------------------------------------------------------------------------------- |
| `check`    | 통과(svelte-check 오류 0, ESLint, Prettier, 문구의 키)                                        |
| `test`     | 2609 통과, 41 건너뜀(원본 번역과 템플릿 아카이브의 재변환처럼 원본이 있어야 하는 것), 파일 99 |
| `build`    | 통과                                                                                          |
| `e2e`      | 532 통과, 179 건너뜀(`FVOCI_SCREENS`의 그림과 프로젝트마다 맞지 않는 것), 실패 0              |
| `e2e:live` | 27 통과, 실패 0(지난 묶음의 11개가 그대로 있다. 서버는 테스트가 띄우고 끈다)                  |

스크린샷 테스트(`FVOCI_SCREENS`와 스파이크의 그림)는 이 묶음에서 돌리지 않았다.

## 다음에 할 일

1. **서버의 결정 묶음(`server-decide`) 뒤의 일.** 남은 것 없음: 웹앱에 이어졌다(`web-two-a`, `web-two-b`). 타입은 계약에서 만든다(`pnpm --filter web run api:types`).
2. **손으로 적은 임시 타입이 남아 있다**(`src/lib/state/memory/seed.ts`의 템플릿 파일). 화면의 카드에는 옛 `contentOrder`가 남아 있지만(`logic/wire.ts`) 서버로 가지 않는다.
3. **검토에서 남긴 것을 본다.** 화면 조립 때의 것은 `docs/decisions.md`의 "검토에서 나온 것과 처리"에, 이 묶음의 것은 각 리뷰 커밋의 본문("Not taken")에 있다.
4. **캘린더와 간트에 끌기가 있다**(V5: `calendar/card-drag.svelte.ts`, `gantt/bar-drag.svelte.ts`, 머리글의 "새로 만들기"는 `lib/state/new-card.ts`). 간트는 서버의 관계 값 검증(V6)이 남았다.
5. **가끔 실패하는 드래그 테스트의 원인을 가른다**(아래).
6. **실제 기기에서 본다.** 터치 드래그, 한글 조합 입력, 진동, 접기·회전, 안전 영역은 아직 실제 기기에서 본 적이 없다.

## 도구가 있는 곳

- Node는 시스템의 것이다(v26.13.0).
- `pnpm`은 이 기기에 전역으로 없다. 저장소 루트에서 한 번 설치하고, 명령마다 `PATH`를 앞에 붙인다.

  ```bash
  cd <워크트리 루트>
  npm install --prefix target/tools/node pnpm@12.18.0
  export PATH=$PWD/target/tools/node/node_modules/.bin:$PATH
  ```

- 브라우저는 Playwright의 Chromium headless shell 하나뿐이다.
- 셸은 명령 사이에 `PATH`와 작업 폴더가 남지 않는다고 보고, 명령마다 다시 적는다.
- **저장소 루트의 `.gitignore`가 `target/`과 `node_modules/`를 무시한다.** `master`로 합치면서 생겼다. 그 전의 `web` 브랜치에는 없어서 `target/`이 추적되지 않는 파일로 보였다.

## 화면을 띄우는 법과 포트

| 무엇                                   | 명령                              | 포트                       | 주소               |
| -------------------------------------- | --------------------------------- | -------------------------- | ------------------ |
| **앱, 스파이크, Storybook을 한꺼번에** | `pnpm --filter web run look`      | 18410, 모든 인터페이스     | `/`가 목록이다     |
| 앱 개발 서버                           | `pnpm --filter web run dev`       | 5173                       | `/`                |
| 스파이크 개발 서버                     | `pnpm --filter web run spikes`    | 5173 고정                  | `/spikes/<이름>/`  |
| 스파이크 Playwright                    | `spikes:e2e`                      | 18320, `FVOCI_SPIKES_PORT` | 스스로 띄우고 끈다 |
| 빌드 결과 미리 보기                    | `pnpm --filter web run preview`   | 4173                       | `/`                |
| 앱 Playwright                          | `e2e`                             | 18310, `FVOCI_E2E_PORT`    | 스스로 띄우고 끈다 |
| 실제 서버의 Playwright                 | `e2e:live`(아래)                  | 18510, `FVOCI_LIVE_PORT`   | 스스로 띄우고 끈다 |
| Storybook 개발 서버                    | `pnpm --filter web run storybook` | 6006                       | `/`                |

`e2e:live`에 관해: 먼저 저장소 루트에서 `cargo build -p fvoci-server --bins --examples`와 `pnpm --filter web run build`를 한다. 그 뒤 `pnpm --filter web run e2e:live`가 그 서버를 SQLite로 `target/web-live/<시각>-<포트>/`에 띄우고(그 다음 포트에 서버 테스트의 가짜 OIDC 공급자도) 끝나면 멈춘다. 테스트는 `e2e-live/`, 정한 것은 `docs/decisions.md`의 연결 8번.

`look`에 관해:

- `build`, `spikes:build`, `build-storybook`을 한 뒤 `tools/serve.ts`로 `dist/`, `dist-spikes/`, `storybook-static/`을 서빙한다. 정적 파일뿐이고 API는 없다.
- 목록의 주소는 `/app/`(조립된 앱), `/screens/spikes/calendar/`, `/screens/spikes/editor/`, `/screens/spikes/drag/`, `/screens/spikes/mobile/`, `/storybook/`이다.
- 앱은 서버가 하듯 base path `/app` 아래에서, 콘텐츠 보안 정책을 붙여서 나간다.
- `/app/?dev=1`로 열면 화면 구석에 창의 너비와 지금의 단계가 보인다.
- 앱의 데이터는 메모리 구현의 것이라 새로 고치면 처음으로 돌아간다. 표시 설정만 기기에 남는다.
- 요청마다 파일을 읽고, 캐시하지 않게 보낸다. **서버를 띄워 둔 채 빌드만 다시 하면 새 파일이 나간다.**
- 스파이크 화면과 Storybook도 앱의 글꼴을 싣는다.
- **Windows의 브라우저에서는 `http://localhost:18410/`로 연다.** 사용자가 PC에서 이 주소로 직접 열어 확인했다.

### LAN 주소로는 Windows 밖에서 닿지 않는다

이 기기는 WSL2이고 네트워크 모드가 mirrored다(`wslinfo --networking-mode`). 2026-11-10 22:00 KST에 읽기만 해서 확인했다.

| 어디서                    | 주소                   | 결과           |
| ------------------------- | ---------------------- | -------------- |
| WSL 안                    | 기기의 LAN 주소, 18410 | 200            |
| Windows(`curl.exe`)       | `localhost:18410`      | 200            |
| Windows(`curl.exe`)       | 기기의 LAN 주소, 18410 | 답이 없다      |
| 휴대폰 등 LAN의 다른 기기 |                        | 해 보지 못했다 |

- 알아낸 원인: WSL의 Hyper-V 방화벽이 들어오는 연결을 기본으로 막는다(`Get-NetFirewallHyperVVMSetting`의 `DefaultInboundAction: Block`). LAN에 열린 규칙은 22번과 6768번 포트 둘뿐이다(`Get-NetFirewallHyperVRule`). 18410번의 규칙은 없다. 그래서 휴대폰에서는 닿지 않을 것으로 본다.
- 가르지 못한 것: Windows가 자기 LAN 주소로 건 연결이 답이 없는 것이 같은 이유인지, mirrored 모드에서 원래 그런 것인지.
- 열려면 관리자 PowerShell에서 규칙을 더해야 한다: `New-NetFirewallHyperVRule -Name fvoci-look -DisplayName "fvoci look - LAN" -Direction Inbound -VMCreatorId '{40E0AC32-46A5-438A-A0B2-2B479E8F2E90}' -Protocol TCP -LocalPorts 18410`. 지울 때는 `Remove-NetFirewallHyperVRule -Name fvoci-look`.
- **방화벽과 Windows의 설정은 바꾸지 않았고, 바꾸지 않기로 했다**(사용자 결정). 휴대폰에서 터치 드래그와 한글 입력을 보는 방법은 정해지지 않았다.

## 가끔 실패하는 것, 확인하지 못한 것

### 스파이크의 드래그 테스트 하나 (풀지 못했다)

`spikes:e2e`의 `drag/drag-decisions.spec.ts` "a sorted view › mouse › a card dropped on another column goes where the order puts it…"이 같은 코드에서 통과하기도 하고 실패하기도 한다.

- **본 것.** 2026-11-10 22:04 KST의 전체 실행에서 한 번 실패했다(222건 가운데 1건). 드래그 테스트만 `--repeat-each 3`으로 돌린 22:08 KST의 실행에서 또 한 번 실패했다(312건 가운데 1건). 두 번 모두 기기가 바쁠 때였다. 그 뒤 두 번의 전체 실행(2026-11-11 00:47, 01:42 KST)과, 드래그 테스트만 `--repeat-each 3`으로 돌린 두 번(2026-11-12 02:32와 02:52 KST, 바꾸기 뒤와 앞, 312건씩, 부하 평균 2–3)에서는 실패하지 않았다.
- **증상.** `hover(page, 'ready-5', …)` 뒤에 보드가 `data-outside`(포인터가 보드 밖)라고 답했다.
- **해 본 것.** 글꼴이 늦게 도착해 배치가 밀리는 것이 원인이라고 보고, `spikes/drag/support.ts`의 `open()`이 `load`와 `document.fonts.ready`를 기다리게 해 봤다. 그 상태에서 같은 테스트가 또 실패했다(부하 평균 25). 원인이 글꼴이라는 것은 확인되지 않았다. 그 수정은 되돌렸고 커밋에 없다.
- **가르지 못한 것.** 테스트의 문제인지 제품 코드의 문제인지다. 테스트라면 부하가 걸렸을 때 `page.mouse.move`의 걸음과 프레임의 순서가 어긋나는 것이다. 제품이라면 "보드 밖" 판정이 가로 자동 스크롤 중에 한 프레임 틀리는 것이다.
- **다음에 볼 곳.** 실패한 실행의 추적(`test-results/spikes/…/trace.zip`, `pnpm exec playwright show-trace`), `sortable.svelte.ts`에서 `outside`를 정하는 자리, 조용한 기기에서 `--repeat-each 10`.

### 종단 테스트가 부하에서 드러낸 것 둘 (고쳤다)

시트가 닫히자마자 다음 시트가 열릴 때 방문 기록이 어긋나는 것(`docs/decisions.md` 58번)과, 메모리 구현에서 포기한 업로드가 테스트가 주문한 실패를 가로채는 것이다. 경위는 `PORTING.md`의 "합친 뒤에 드러나 고친 것"에 있다. 고친 뒤 전체를 두 번씩 돌린 1,064건과 마지막 실행 둘(532건씩)에서는 실패가 없었다. **그보다 많이 되풀이해 보지는 않았다.**

### 확인하지 못한 것

- 실제 기기, Firefox, Safari, 화면 낭독기, 한글 조합 입력으로 본 것은 없다. 종단 테스트는 Chromium의 세 크기(1280×720, 626×890 터치, Pixel 7)다. 사용자가 PC의 브라우저로 화면을 연 것까지다.
- 화면의 그림(`docs/screens/`)에서 이모지가 빈 네모로 나온다. 이 기기에 이모지 글꼴이 없어서다.
- "움직임 줄이기"에서 다이어그램이 잘리던 것과 같은 함정이 다른 곳에도 있을 수 있다. 전환이 0.01ms로 줄어든 채 "놓자마자 재는" 코드가 그것이다. 찾아본 것은 다이어그램뿐이다.

## 걸렸던 함정

각 스파이크 문서의 "걸린 것"에 있는 것은 되풀이하지 않는다.

### 이 기기

- **닫힌 로컬 포트로의 연결이 거절되지 않고 매달린다**(WSL2). `curl`은 `-m`으로 끊는다. Playwright는 `webServer.url`이나 `port`가 있으면 서버를 띄우기 전에 그 주소가 비었는지 연결해 보는데, 여기서 2분 넘게 멈췄다. 그래서 `url` 대신 `wait: { stdout: … }`로 서버의 준비 출력을 기다리게 했다.

### 빌드와 서빙

- **스파이크의 빌드는 상대 주소를 써야 폴더 아래에서 서빙된다.** 처음에는 `/assets/…`로 나와서 `/screens/` 아래에서 스크립트가 404였다. HTTP 200만 보고는 몰랐고, 브라우저로 열어 실패한 요청을 세어 알았다. 고친 곳은 `vite.spikes.config.ts`의 `base: './'`다.
- **`src/styles/app.css`가 `spikes/`를 Tailwind의 클래스 스캔에서 뺀다.** 스파이크마다 자기 CSS에서 `@source './*.svelte'`(파일 글롭)로 되살려야 한다. 편집기 스파이크는 그것이 없어서 배치 클래스가 생성되지 않고 있었다(`spikes/editor/spike.css`로 고쳤다).
- **Storybook 빌드에는 앱의 산출물 플러그인을 빼야 한다.** `.storybook/main.ts`의 `viteFinal`에서 `fvoci:` 플러그인, `renderBuiltUrl`, 해시 파일명 설정을 걷어 낸다.
- **글꼴 산출물은 커밋되지 않는다.** 새 워크트리는 `src/styles/fonts/`가 비어 있고, `build`가 다시 만들려면 원본 143 MB를 받는다. 사본을 두고 워크트리마다 복사해 쓴다(아래 "추적되지 않는 것").
- **글꼴 도구의 건너뛰기 판단은 `tools/fonts/` 아래 모든 파일의 해시다.** 주석 하나만 고쳐도 다음 `build`가 글꼴을 다시 만든다(약 2분). 사본도 낡으므로 다시 떠야 한다.
- **스파이크와 Storybook이 앱의 글꼴을 싣게 하자 브라우저 테스트가 흔들렸다.** 글꼴은 늦게 도착한다. 규칙은 브라우저가 한가할 때, 파일은 글자가 처음 필요할 때 온다. 도착하면 줄이 바뀐다. 두 쪽을 차례로 재서 견주는 테스트는 그 사이에 걸린다. `editor/view.spec.ts`는 `networkidle`과 `document.fonts.ready`를 기다린 뒤, 재는 동안 아무것도 움직이지 않았는지까지 본다.

### 로직과 타입

- **`PropertyValue`는 넓다**(날짜 값이 객체다). 글자나 목록을 기대하는 자리는 `plainPropertyValue`를 거친다. `String(값)`을 쓰면 린트(`no-base-to-string`)가 잡는다.
- **`Date.UTC`는 0–99년을 1900–1999년으로 읽는다.** 서버가 받는 연도는 1–9999라서 `dates.ts`가 `setUTCFullYear`로 만든다.
- **`formatDateValue`는 빈 값에 `undefined`를 준다.** 테스트에서는 `tests/logic/support/factory.ts`의 `storedDate`를 쓴다.
- **Tiptap(ProseMirror)의 `marks: ''`는 "이 노드 안에 올 수 있는 마크"다.** 노드 자신이 마크를 갖는지는 부모가 정한다. 줄바꿈에 마크가 붙지 않게 하려고 그 설정을 넣어 봤지만 효과가 없었다. 저장할 때 `toStoredDoc`이 글자가 아닌 노드의 마크를 뗀다.
- **검증기(`validateDoc`)는 어떤 값에도 예외를 던지지 않아야 한다.** 표시기의 비교 테스트가 던지는 경우들을 찾았다. 새 규칙을 더할 때 `tests/editor/schema.test.ts`의 "a value that is not shaped like a document"에 경우를 더한다.

### 문서와 커밋

- **문서에 시각을 미리 적지 않는다.** 일이 끝날 시각을 어림해 적었다가 여러 번 고쳤다. `date`를 찍고 그 값을 적는다.
- **이름 검사는 산출물도 본다.** 템플릿의 "바뀐 곳 목록"(`CHANGES.md`)은 이전 문구에 원본의 이름이 들어가므로 `{old product name}`, `{old company name}`으로 가려서 썼다.
- **생성물을 Prettier가 다시 접는다.** `JSON.stringify(…, null, 2)`의 배열 모양은 Prettier의 것과 다르다. 도구가 쓰는 파일은 `.prettierignore`에 넣고, "최신인가"는 검사가 본다(`src/lib/api/contract.ts`).
- **충돌을 풀며 체리픽한 커밋은 `git cherry`가 "같은 변경"으로 알아보지 못한다.** 화면 조립 때 메시지 파일의 충돌을 풀며 들어온 커밋이 그랬다. 워크트리를 지워도 되는지는 워킹 트리가 깨끗한지와 내용이 들어왔는지를 따로 확인한다.
- **다른 커밋의 트리를 `target/` 아래에 풀고 거기서 `pnpm exec`를 부르면 안 된다.** pnpm이 심볼릭 링크를 타고 `apps/web/node_modules`에 잠금 파일 없이 설치한다. 한 번 겪었고, 지우고 `--frozen-lockfile`로 다시 설치했다. 추적되는 파일에는 영향이 없었다.

## 추적되지 않는 것

- 무시되는 산출물은 `dist/`, `dist-spikes/`, `storybook-static/`, `test-results/`, `src/styles/fonts/`, `src/lib/i18n/paraglide/`다. 모두 다시 만들 수 있다.
- 다시 만들기에 오래 걸리거나 한 번만 만든 것은 저장소 밖 `~/atlas/musical-happiness-carry/web/`에 옮겨져 있다. 새 워크트리에 놓는 명령은 그 폴더의 `README.md` 3절에 있다. 든 것은 이렇다.
  - 글꼴 산출물과 그 원본(`src/styles/fonts/`, `target/web-fonts/`). 이것이 있으면 `build`가 네트워크 없이 지나간다.
  - 마지막 검증의 출력과 측정 스크립트(`target/scratch/`). 번들 크기를 잰 `measure-sizes.mjs`와 `measure.ts`, 테스트 매핑 표를 만든 `from-agents/make-mapping.mjs`가 여기 있다.
  - 하위 에이전트에게 준 지시서(`target/briefs/`, `target/agent-brief-common.md`). 안의 경로와 기준 커밋은 옛 워크트리의 것이라 고쳐 써야 한다.
- 계획 문서는 저장소 밖 `~/.claude/plans/fvoci-web-svelte.md`에 있다.

# 인계: 웹앱 (화면 조립까지 끝났고, 서버 연결은 시작하지 않았다)

고쳐 쓴 때: 2026-11-11 02:50 KST. 두 번에 걸쳐 쓴 인계(2026-11-10 22:10 KST, 2026-11-11 01:50 KST)를 하나로 고쳐 썼다. 2026-11-11 03:52 KST에 서버와 한 브랜치(`master`)로 합치면서 서버 쪽 파일을 가리키는 말을 고쳤다. 경로는 따로 말하지 않으면 `apps/web` 기준이다.

다른 문서에 없는 것만 적는다. 이미 적힌 것은 위치만 가리킨다.

## 지금 상태

- **화면은 끝까지 조립돼 있다.** 서버가 서빙하는 앱은 서버를 부르는 구현(`src/lib/api/`)으로 돌고, 서버 없이 빌드를 보는 도구(`preview`, `look`)와 개발 서버는 메모리 구현으로 돈다(`docs/decisions.md`의 연결 1번). 서버에 붙은 것은 아래 목록까지이고, 나머지 조작은 `not_wired`로 거절한다(연결 3번).
- **서버의 계약이 나왔다**(2026-11-11 02:11 KST). 저장소 루트의 `contracts/openapi.json`이다. 계약과 맞춰 볼 가정과 질문은 `docs/contract-assumptions.md`에 있다.
- **마지막 검증은 모두 통과했다**(2026-11-11 01:29–01:47 KST). 그 뒤의 커밋은 그림과 문서뿐이다. 명령, 결과, 걸린 시간은 `PORTING.md`의 "검증 명령"과 "마지막 검증"에 있다.
- **서버와 한 브랜치에 있다.** 저장소는 `fvoci/musical-happiness`(비공개)이고, 브랜치 `master`에 서버(저장소 루트)와 웹앱(`apps/web`)이 함께 있다. 일마다 `master`에서 워크트리를 새로 갈라 일하고, 끝나면 `master`에 합친다. 양쪽에 걸친 안내(브랜치와 푸시, 양쪽의 검증 명령, 보관 폴더, 컨테이너)는 저장소 루트의 `docs/handoff.md` 맨 앞에 있다.
- **올라간 커밋은 고쳐 쓰지 않는다.** 고칠 것은 새 커밋으로 얹는다.

### 서버에 붙이는 일: tracer-bullet 목록

2026-11-11 04:13 KST에 브랜치 `fvoci/web-wire`에서 시작했다. 하나는 기능 하나가 화면, 저장소, 서버를 부르는 구현, 실제 서버, 종단 테스트까지 끝에서 끝으로 도는 것이다. 차례대로 하고, 끝난 것에 `[x]`를 친다.

- [x] 1. 가입하고 로그인하면 빈 보드 목록이 보이고, 로그아웃하면 로그인 화면으로 돌아간다
- [x] 2. 보드 목록과 보드 열기(뷰, 카드, 구성원)
- [x] 3. 카드 만들기
- [x] 4. 카드 고치기(제목과 속성)
- [x] 5. 카드 옮기기(칸반의 열과 순서)
- [x] 6. 실시간: 다른 사용자가 바꾼 것이 보이고, 재연결 뒤에는 보드를 다시 읽는다
- [x] 7. 카드 지우기와 휴지통에서 되살리기(실행 취소로. 휴지통 화면은 없다)
- [x] 8. 카드 상세에서도 지울 때 묻지 않는다
- [x] 9. 표시 설정의 이름 `theme`, `haptics`
- [x] 10. 스크린샷을 커밋하지 않는 폴더(`target/screens/`)에 찍는다

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
| 템플릿 산출물과 바뀐 곳                          | `tools/templates/output/README.md`, `CHANGES.md`                                                                 |
| 편집기, 읽기 전용 표시기, 번들 크기              | `docs/spike-editor.md`(표시기는 9절, 창 크기는 6.5절, 수식과 다이어그램은 10절)                                  |
| 드래그의 결정 1–11, 공개 형태, 창 크기가 바뀔 때 | `docs/spike-drag.md`                                                                                             |
| 속성 편집기, 카드 상세와 원본 테스트의 대응      | `docs/properties-test-mapping.md`, `docs/card-test-mapping.md`, `src/lib/components/README.md`                   |
| 빌드의 `.zst`, 원본 폴더 없이 도는 것            | `docs/build-notes.md`                                                                                            |
| 화면 크기의 세 단계라는 방침                     | `PORTING.md`의 "화면 크기의 세 단계", 저장소 밖의 계획 `~/.claude/plans/fvoci-web-svelte.md`                     |
| 도구의 위치, 화면을 띄우는 법, 함정              | 이 문서                                                                                                          |

## 정해졌고 아직 코드에 없는 것

없다. 셋 다 이번 묶음에서 넣었다:

- 카드 상세에서 지울 때도 묻지 않는 것은 코드에 넣었다(tracer-bullet 8). 되돌리기는 실행 취소와 휴지통(30일)이 맡는다.
- 표시 설정의 이름 `theme`, `haptics`는 코드에 넣었다(tracer-bullet 9). 값과 타입은 그대로다(`docs/contract-assumptions.md`의 F1).
- 스크린샷은 커밋하지 않는 `target/screens/`에 찍는다(tracer-bullet 10): `e2e/*.spec.ts`의 `FVOCI_SCREENS` 갈래는 `target/screens/<화면>/`, 스파이크의 `shots.spec.ts` 둘과 `spikes/editor/embeds.spec.ts`는 `target/screens/spikes/<이름>/`. 저장소에 든 307장은 그대로 두었다. 문서에 새 그림이 필요하면 손으로 옮긴다.

## 다음에 할 일

1. **서버를 부르는 구현을 붙인다.** `src/lib/state/backend.ts`의 인터페이스를 둘째 구현으로 채운다. 어느 구현을 쓸지는 `src/app/backend.ts`가 고른다. `window.fvociDev`는 메모리 구현에만 붙인다.
2. **가정을 계약과 맞춘다.** `docs/contract-assumptions.md`의 질문부터 본다. 답이 난 것 넷(A6, A7, 카드 상세의 질문 1, F1)은 그 문서의 머리에 있다.
3. **타입을 계약에서 만든다.** 손으로 적은 임시 타입(`tools/templates/output.ts`, `wire.ts`의 날짜 값)을 거기에 맞춘다.
4. **서버 쪽에 전달한다.** 온보딩 보드를 제목이 아니라 템플릿 산출물의 `onboarding` 표식으로 찾아야 한다. `docs/contract-assumptions.md`의 5절 "내장 템플릿의 산출물"에 넷이 있다.
5. **위 표의 셋을 코드에 넣는다.**
6. **검토에서 남긴 것을 본다.** `docs/decisions.md`의 "검토에서 나온 것과 처리"에 까닭과 함께 있다. 정해야 할 것은 선택지 편집을 저장소로 모으는 일이다.
7. **캘린더 뷰는 안만 있다.** 질문 24개의 답이 오면 만든다.
8. **가끔 실패하는 드래그 테스트의 원인을 가른다**(아래).
9. **실제 기기에서 본다.** 터치 드래그, 한글 조합 입력, 진동, 접기·회전, 안전 영역은 아직 실제 기기에서 본 적이 없다.

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

`e2e:live`에 관해: 먼저 저장소 루트에서 `cargo build -p fvoci-server`(템플릿 아카이브를 놓아야 컴파일된다, 루트 `docs/handoff.md`)와 `pnpm --filter web run build`를 한다. 그 뒤 `pnpm --filter web run e2e:live`가 그 서버를 SQLite로 `target/web-live/<시각>-<포트>/`에 띄우고 끝나면 멈춘다. 테스트는 `e2e-live/`, 정한 것은 `docs/decisions.md`의 연결 8번.

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

- **본 것.** 2026-11-10 22:04 KST의 전체 실행에서 한 번 실패했다(222건 가운데 1건). 드래그 테스트만 `--repeat-each 3`으로 돌린 22:08 KST의 실행에서 또 한 번 실패했다(312건 가운데 1건). 두 번 모두 기기가 바쁠 때였다. 그 뒤 두 번의 전체 실행(2026-11-11 00:47, 01:42 KST)에서는 실패하지 않았다.
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
- **이름 검사는 산출물도 본다.** 템플릿의 "바뀐 곳 목록"은 이전 문구에 원본의 이름이 들어가므로 `{old product name}`, `{old company name}`으로 가려서 쓴다(`tools/templates/rewrite.ts`의 `maskOldNames`).
- **생성물을 Prettier가 다시 접는다.** `JSON.stringify(…, null, 2)`의 배열 모양은 Prettier의 것과 다르다. 도구가 쓰는 파일은 `.prettierignore`에 넣고, "최신인가"는 테스트가 본다(`messages/`, `tools/templates/output/`).
- **체리픽한 뒤 생성물을 다시 만든다.** 두 작업이 `messages-own/`에 키를 더하고 `messages/`를 각자 생성한 적이 있다. 합친 뒤 `pnpm --filter web run i18n:convert`와 `node tools/i18n/review.ts`를 돌려 차이가 없는지 본다.
- **충돌을 풀며 체리픽한 커밋은 `git cherry`가 "같은 변경"으로 알아보지 못한다.** 화면 조립 때 메시지 파일의 충돌을 풀며 들어온 커밋이 그랬다. 워크트리를 지워도 되는지는 워킹 트리가 깨끗한지와 내용이 들어왔는지를 따로 확인한다.
- **다른 커밋의 트리를 `target/` 아래에 풀고 거기서 `pnpm exec`를 부르면 안 된다.** pnpm이 심볼릭 링크를 타고 `apps/web/node_modules`에 잠금 파일 없이 설치한다. 한 번 겪었고, 지우고 `--frozen-lockfile`로 다시 설치했다. 추적되는 파일에는 영향이 없었다.

## 하위 에이전트를 쓴 방식

화면 조립 전에 여섯 번 썼다. 다섯을 나란히 돌렸고, 하나는 이어서 한 번 더 시켰다. 모두 격리된 워크트리에서 일하고 커밋을 냈고, 주 세션이 웹앱 브랜치에 체리픽했다. 화면 조립 때는 여덟 개의 워크트리에서 같은 방식으로 했다.

| 일                          | 걸린 시간 | 얹은 커밋 수 | 충돌               |
| --------------------------- | --------- | ------------ | ------------------ |
| 번역 결정과 한국어 검수 표  | 38분      | 1            | 없음               |
| 드래그 결정과 기반의 일반화 | 55분      | 3            | 없음               |
| 모바일 화면 안(첫 판)       | 39분      | 2            | 없음               |
| 속성 편집기 15종            | 47분      | 3            | 없음(자동 병합 둘) |
| 본문 읽기 전용 표시기       | 38분      | 3            | 없음               |
| 모바일 화면 안(중간 단계)   | 45분      | 2            | 없음               |

그동안 주 세션이 직접 한 것은 날짜 와이어와 지역 설정, 글꼴, 템플릿 산출물, 정적 서버, 합치기와 검증, 창 크기에 관한 드래그·편집기의 확인이다.

나눈 방식:

- **공통 지시를 파일 하나에 두었다.** 내용은 기준 커밋으로 `reset --hard`하고 제목을 확인할 것, pnpm의 경로, 글꼴 사본, 금지 문자열, 건드리지 않는 파일, 닫힌 포트와 포트 번호, 프로세스를 죽이는 법, 커밋 메시지의 끝줄, 보고의 틀이다. 작업별 지시문은 그 파일을 먼저 읽으라고만 하고 그 작업의 것만 적었다.
- **파일의 주인을 지시문에서 나눴다.** `package.json`, `PORTING.md`, 잠금 파일, `tools/fonts/`, `app.css`는 아무도 건드리지 않게 했다. 예외는 속성 편집기가 `app.css`에 색 토큰을 더한 것 하나다. 작업마다 "네 파일"을 적었다. `messages-own/`은 속성 편집기만 쓰고, 번역 작업은 새 폴더 `messages-override/`를 썼다. 충돌이 없었던 것은 이 덕이다.
- **`PORTING.md`에 들어갈 것은 조각 문서로 받았다**(속성 편집기의 `docs/properties-test-mapping.md`). 합친 뒤 주 세션이 `PORTING.md`에 이었다.
- **한 작업이 다른 작업의 결과를 써야 하면 먼저 그 접점만 커밋하고 그 커밋에서 시작하게 했다.** 표시기가 템플릿 문서 58개를 읽을 수 있게, 변환기를 라이브러리로 나눈 커밋을 먼저 냈다.
- **포트를 작업마다 주었다**(18331, 18341, 18351, 18361, 6016).
- **이어서 시킬 때는 같은 에이전트에게 보냈다.** 모바일 안의 중간 단계는 첫 판을 만든 에이전트가 문맥을 가지고 있었다. 새 기준 커밋으로 `reset --hard`하게 하고 요청을 그대로 옮겨 보냈다.

합칠 때의 주의점:

- **에이전트의 "통과"는 그 기준 커밋에서의 것이다.** 합친 뒤 `check`와 `test`를 다시 돌린다. 합친 상태에서만 드러난 것이 있었다. 편집기 스파이크에 글꼴을 싣자 표시기의 브라우저 비교가 실패했다.
- **보고의 수치는 직접 뽑아 본다.** 보고와 실제가 어긋난 곳은 없었다. 그래도 완료 조건을 확인하는 쪽은 보고가 아니라 주 세션의 출력을 요구했다. 종류별 이야기 수와 테스트 수, 드래그 결정 표의 행, 검수 표의 행과 원 표의 대응, 비교 테스트의 문서 수는 합친 뒤 다시 세었다.
- **"지시와 다른 점"과 "남긴 발견"을 읽는다.** 쓸모 있는 것이 거기에 있었다.
  - 표시기: 검증기가 예외를 던지는 경우, 줄바꿈의 마크.
  - 번역: "Boards"는 5건이 아니라 4건이라는 것, 원 표가 44행이라는 것.
  - 속성 편집기: 기능 폴더끼리 가져올 수 없어 조립 방식을 정해야 한다는 것.
  - 모바일: 넓은 화면의 가운데 대화상자가 접히는 자리에 걸린다는 것.
- **에이전트가 자기 보고에서 문서의 문장을 고쳐야 한다고 알려 오는 경우가 있다.** "모두 열어 봤다"고 썼지만 실제로는 일부였던 것이 그랬다. 합치면서 고쳤다.
- **스크린샷처럼 환경에 달린 산출물은 합친 뒤 주 세션에서 다시 만든다.** 같은 입력이면 같은 바이트가 나오는 것을 확인했다.

## 추적되지 않는 것

- 무시되는 산출물은 `dist/`, `dist-spikes/`, `storybook-static/`, `test-results/`, `src/styles/fonts/`, `src/lib/i18n/paraglide/`다. 모두 다시 만들 수 있다.
- 다시 만들기에 오래 걸리거나 한 번만 만든 것은 저장소 밖 `~/atlas/musical-happiness-carry/web/`에 옮겨져 있다. 새 워크트리에 놓는 명령은 그 폴더의 `README.md` 3절에 있다. 든 것은 이렇다.
  - 글꼴 산출물과 그 원본(`src/styles/fonts/`, `target/web-fonts/`). 이것이 있으면 `build`가 네트워크 없이 지나간다.
  - 마지막 검증의 출력과 측정 스크립트(`target/scratch/`). 번들 크기를 잰 `measure-sizes.mjs`와 `measure.ts`, 테스트 매핑 표를 만든 `from-agents/make-mapping.mjs`가 여기 있다.
  - 하위 에이전트에게 준 지시서(`target/briefs/`, `target/agent-brief-common.md`). 안의 경로와 기준 커밋은 옛 워크트리의 것이라 고쳐 써야 한다.
- 계획 문서는 저장소 밖 `~/.claude/plans/fvoci-web-svelte.md`에 있다.

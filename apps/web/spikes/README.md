# 스파이크

비용이 불확실한 부품을 제품에 넣기 전에 따로 만들어 재 보는 화면이다. 제품 빌드(`vite.config.ts`)에는 들어가지 않는다.

- 화면: `spikes/<이름>/index.html` + `main.ts`. 앱의 스타일(`src/styles/app.css`), 공용 UI, 로직을 그대로 가져다 쓴다.
- 브라우저 테스트: `spikes/<이름>/*.spec.ts` (`playwright.spikes.config.ts`).
- 결과 문서: `docs/spike-<이름>.md`.
- 스파이크에서 제품으로 가져갈 순수 로직은 처음부터 `src/lib/logic`에 두고 `tests/logic`에서 테스트한다.

```bash
pnpm --filter web run spikes         # 개발 서버. http://127.0.0.1:5173/spikes/<이름>/
pnpm --filter web run spikes:build   # dist-spikes/ 로 빌드 (크기 측정용)
pnpm --filter web run spikes:e2e     # 스파이크의 Playwright 테스트
```

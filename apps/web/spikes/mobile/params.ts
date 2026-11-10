/**
 * Which mock-up the page shows, read from its address:
 *
 *   /spikes/mobile/?screen=kanban&variant=a
 *
 * Without `screen` the page is the chooser. This file has no DOM in it, so
 * the browser tests read the same list.
 */

export type Variant = 'a' | 'b';

export interface ScreenInfo {
  id: string;
  title: string;
  /** What the two proposals are; one entry when there is nothing to choose. */
  variants: Partial<Record<Variant, string>>;
}

export const SCREENS = [
  {
    id: 'sidebar',
    title: '사이드바',
    variants: { a: '왼쪽에서 미는 서랍', b: '보드 전환 시트' },
  },
  {
    id: 'kanban',
    title: '칸반',
    variants: { a: '한 열씩 넘기기(스냅과 열 탭)', b: '좁은 열을 자유롭게 스크롤' },
  },
  {
    id: 'table',
    title: '테이블',
    variants: { a: '고정 열과 가로 스크롤', b: '행을 카드처럼 쌓기' },
  },
  {
    id: 'card',
    title: '카드 상세',
    variants: { a: '전체 화면 페이지', b: '거의 전체 높이의 바닥 시트' },
  },
  {
    id: 'menu-view',
    title: '메뉴: 보기 설정(필터·정렬·그룹)',
    variants: { a: '바닥 시트', b: '팝오버 그대로' },
  },
  {
    id: 'menu-card',
    title: '메뉴: 카드의 "…" 메뉴',
    variants: { a: '바닥 시트', b: '팝오버 그대로' },
  },
  {
    id: 'menu-select',
    title: '메뉴: 선택 속성의 선택지 고르기',
    variants: { a: '바닥 시트', b: '팝오버 그대로' },
  },
  {
    id: 'menu-date',
    title: '메뉴: 날짜 선택기',
    variants: { a: '바닥 시트', b: '팝오버 그대로' },
  },
  {
    id: 'header',
    title: '뷰 머리글의 축약',
    variants: { a: '한 줄과 "…"', b: '두 줄' },
  },
] as const satisfies readonly ScreenInfo[];

export type ScreenId = (typeof SCREENS)[number]['id'];

/** What is open when the page loads. `main` is the overlay the screen is about. */
export type Opened = 'none' | 'main' | 'more' | 'views';

export interface Params {
  /** `undefined` for the chooser. */
  screen: ScreenId | undefined;
  variant: Variant;
  opened: Opened;
  theme: 'dark' | 'light' | undefined;
  /** Stand-ins for the notch and home indicator, and for the on-screen keyboard. */
  fakeSafeArea: boolean;
  fakeKeyboard: boolean;
  /** Kanban: the column in view. Table: how far it is scrolled sideways, in pixels. */
  column: number;
  scrollX: number;
}

function wholeNumber(text: string | null, fallback: number): number {
  return text !== null && /^\d+$/.test(text) ? Number(text) : fallback;
}

export function readParams(search: string): Params {
  const query = new URLSearchParams(search);
  const screen = SCREENS.find((entry) => entry.id === query.get('screen'))?.id;
  const open = query.get('open');
  const theme = query.get('theme');
  return {
    screen,
    variant: query.get('variant') === 'b' ? 'b' : 'a',
    opened: open === '0' ? 'none' : open === 'more' || open === 'views' ? open : 'main',
    theme: theme === 'dark' || theme === 'light' ? theme : undefined,
    fakeSafeArea: query.get('safe') === '1',
    fakeKeyboard: query.get('keyboard') === '1',
    column: wholeNumber(query.get('column'), 1),
    scrollX: wholeNumber(query.get('x'), 0),
  };
}

/** The address of one mock-up, relative to the spike's page. */
export function addressOf(screen: ScreenId, variant: Variant, extra = ''): string {
  return `?screen=${screen}&variant=${variant}${extra}`;
}

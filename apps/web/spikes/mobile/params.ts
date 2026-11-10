/**
 * Which mock-up the page shows, read from its address:
 *
 *   /spikes/mobile/?screen=kanban&variant=a
 *
 * Without `screen` the page is the chooser. This file has no DOM in it, so
 * the browser tests read the same list.
 */

/**
 * The sizes the proposals are drawn at, in CSS pixels, by their width. All
 * but the first and the last are rough figures for a folding phone (outer
 * screen, inner screen upright and on its side) and a tablet held upright;
 * none was measured on a device.
 */
export const SIZES = {
  360: { width: 360, height: 740 },
  466: { width: 466, height: 678 },
  626: { width: 626, height: 890 },
  820: { width: 820, height: 1180 },
  890: { width: 890, height: 626 },
  1280: { width: 1280, height: 800 },
} as const;

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
    variants: {
      a: '서랍(좁음), 띠와 겹쳐 뜨는 서랍(중간)',
      b: '보드 전환 시트(좁음), 띠 없이 겹쳐 뜨는 서랍(중간)',
    },
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
    variants: {
      a: '전체 화면(좁음), 옆 판(중간), 가운데 대화상자(넓음)',
      b: '바닥 시트(좁음), 옆 판(중간), 보드 옆의 칸(넓음)',
    },
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
  /**
   * Stand-ins for what a headless browser lacks. The edges a device keeps:
   * `notch` at the top and bottom of a phone held upright, `right` at the
   * right edge only. The on-screen keyboard. The line a screen folds along,
   * drawn down the middle.
   */
  fakeSafeArea: 'none' | 'notch' | 'right';
  fakeKeyboard: boolean;
  foldLine: boolean;
  /** The fixed sidebar of a wide window starts as a strip. */
  sidebarAsStrip: boolean;
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
  const safe = query.get('safe');
  return {
    screen,
    variant: query.get('variant') === 'b' ? 'b' : 'a',
    opened: open === '0' ? 'none' : open === 'more' || open === 'views' ? open : 'main',
    theme: theme === 'dark' || theme === 'light' ? theme : undefined,
    fakeSafeArea: safe === '1' ? 'notch' : safe === 'right' ? 'right' : 'none',
    fakeKeyboard: query.get('keyboard') === '1',
    foldLine: query.get('fold') === '1',
    sidebarAsStrip: query.get('sidebar') === 'strip',
    column: wholeNumber(query.get('column'), 1),
    scrollX: wholeNumber(query.get('x'), 0),
  };
}

/** The address of one mock-up, relative to the spike's page. */
export function addressOf(screen: ScreenId, variant: Variant, extra = ''): string {
  return `?screen=${screen}&variant=${variant}${extra}`;
}

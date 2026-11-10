/**
 * Which mock-up the page shows, read from its address:
 *
 *   /spikes/calendar/?screen=month&variant=a
 *
 * Without `screen` the page is the chooser. This file has no DOM in it, so
 * the browser tests read the same list. The arguments are those of the
 * mobile mock-ups (`screen`, `variant`, `open`, `theme`, `safe`, `fold`,
 * `sidebar`), and three of the calendar's own (`week`, `zone`, `tint`).
 */
// By its path and not by `$lib`: the browser tests read this file outside of Vite.
import { WEEK_START_SETTINGS, type WeekStartSetting } from '../../src/lib/logic/regional.ts';
import { SIZES as ALL_SIZES } from '../mobile/params.ts';

/**
 * The sizes the proposals are drawn at, by their width: a phone, the inner
 * screen of a folding phone upright and on its side, a tablet upright, a
 * desktop. Those in between are the rough figures of the mobile proposals.
 */
export const SIZES = {
  360: ALL_SIZES[360],
  626: ALL_SIZES[626],
  820: ALL_SIZES[820],
  890: ALL_SIZES[890],
  1280: ALL_SIZES[1280],
} as const;

export type Variant = 'a' | 'b';

export interface ScreenInfo {
  id: string;
  title: string;
  /** What the two proposals are; one entry when there is nothing to choose. */
  variants: Partial<Record<Variant, string>>;
  /** The two are not rival proposals but two readers of the same board. */
  cases?: boolean;
}

export const SCREENS = [
  {
    id: 'month',
    title: '월 보기',
    variants: {
      a: '좁으면 줄인 격자(점·개수)와 그날의 목록',
      b: '좁으면 날짜별 일정 목록과 주 띠',
    },
  },
  {
    id: 'more',
    title: '"+N 더 보기"가 여는 것',
    variants: { a: '칸 옆의 팝오버', b: '격자 곁의 "그날의 목록"' },
  },
  {
    id: 'range',
    title: '기간이 있는 카드',
    variants: { a: '막대(좁으면 선)', b: '좁은 화면의 일정 목록에서' },
  },
  {
    id: 'move',
    title: '날짜를 바꾸는 방법',
    variants: { a: '끌어서 옮기기(손가락은 길게 눌러)', b: '카드 메뉴의 "날짜 바꾸기"' },
  },
  {
    id: 'create',
    title: '빈 날에 카드 만들기',
    variants: { a: '칸을 누르면', b: '칸의 "+"' },
  },
  {
    id: 'display',
    title: '표시 기준 속성과 날짜 없는 카드',
    variants: { a: '"날짜 없음" 서랍', b: '격자 아래의 "날짜 없음" 줄' },
  },
  {
    id: 'week',
    title: '주 보기',
    variants: { a: '한 주를 화면 높이로' },
  },
  {
    id: 'regional',
    title: '주의 시작 요일과 시간대',
    variants: { a: '일요일 시작, 서울', b: '월요일 시작, 로스앤젤레스' },
    cases: true,
  },
] as const satisfies readonly ScreenInfo[];

export type ScreenId = (typeof SCREENS)[number]['id'];

/**
 * What is open when the page loads. `main` is what the screen is about;
 * `menu` the menu of a card; `by` the list of properties to place cards by.
 */
export type Opened = 'none' | 'main' | 'menu' | 'by';

export interface Params {
  /** `undefined` for the chooser. */
  screen: ScreenId | undefined;
  variant: Variant;
  opened: Opened;
  theme: 'dark' | 'light' | undefined;
  /** As in the mobile mock-ups: room a device keeps at its edges, drawn in. */
  fakeSafeArea: 'none' | 'notch' | 'right';
  /** The fixed sidebar of a wide window starts as a strip. */
  sidebarAsStrip: boolean;
  /** The reader's settings; `undefined` is what the screen itself wants to show. */
  weekStart: WeekStartSetting | undefined;
  timeZone: string | undefined;
  /** Bars take the colour of the card's status instead of one neutral fill. */
  tinted: boolean;
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
    opened: open === '0' ? 'none' : open === 'menu' || open === 'by' ? open : 'main',
    theme: theme === 'dark' || theme === 'light' ? theme : undefined,
    fakeSafeArea: safe === '1' ? 'notch' : safe === 'right' ? 'right' : 'none',
    sidebarAsStrip: query.get('sidebar') === 'strip',
    weekStart: WEEK_START_SETTINGS.find((known) => known === query.get('week')),
    timeZone: query.get('zone') ?? undefined,
    tinted: query.get('tint') === '1',
  };
}

/** The address of one mock-up, relative to the spike's page. */
export function addressOf(screen: ScreenId, variant: Variant, extra = ''): string {
  return `?screen=${screen}&variant=${variant}${extra}`;
}

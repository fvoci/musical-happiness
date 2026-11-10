/**
 * The made-up board of the mobile mock-ups, with dates: nothing is loaded and
 * nothing is saved. Every value is written out, the moment that counts as
 * "now" included, so that a picture of it never changes.
 *
 * A date is what the product stores (`$lib/logic/dates.ts`): a day on the
 * calendar, the same day for every reader, or a moment, which falls on the
 * day it has in the reader's time zone. Three cards carry a time for that
 * reason.
 */
import { calendarDate, daysOf, type DatePoint, type DateValue } from '$lib/logic/dates.ts';

import type { Hue } from '../mobile/data.ts';

import type { Span } from './grid.ts';

/** The moment every mock-up believes it is: a Tuesday morning in Seoul, Monday afternoon in Los Angeles. */
export const NOW = Date.parse('2026-11-10T09:00:00+09:00');

export interface Entry {
  id: string;
  title: string;
  status: string;
  assignee: string | undefined;
  /** The value of the property the calendar places cards by; none for a card without a date. */
  value: DateValue | undefined;
}

const day = (month: number, date: number): DatePoint => ({ date: calendarDate(2026, month, date) });
const at = (text: string): DatePoint => ({ instant: Date.parse(text) });
const on = (month: number, date: number): DateValue => ({ from: day(month, date) });
const span = (from: DatePoint, to: DatePoint): DateValue => ({ from, to });

// The ids and titles up to c14 are those of the mobile mock-ups.
export const ENTRIES: readonly Entry[] = [
  {
    id: 'r1',
    title: '10월 회고 정리',
    status: 'done',
    assignee: 'daeun',
    value: span(day(10, 29), day(11, 2)),
  },
  {
    id: 'c13',
    title: 'QA',
    status: 'done',
    assignee: 'daeun',
    value: span(day(11, 2), day(11, 7)),
  },
  {
    id: 'c14',
    title: '베타 테스터 모집 공지',
    status: 'done',
    assignee: 'minjae',
    value: on(11, 3),
  },
  { id: 'c12', title: '릴리스 노트 초안', status: 'done', assignee: 'daeun', value: on(11, 6) },
  {
    id: 'c1',
    title: '로그인 화면 다시 그리기',
    status: 'doing',
    assignee: 'seoyeon',
    value: span(day(11, 9), day(11, 13)),
  },
  {
    // 08:00 on the 10th in Seoul; still the 9th in Los Angeles.
    id: 't1',
    title: '주간 회의',
    status: 'todo',
    assignee: 'junho',
    value: { from: at('2026-11-10T08:00:00+09:00') },
  },
  {
    id: 'c10',
    title: '결제 화면 문구 검수',
    status: 'review',
    assignee: 'jiwoo',
    value: on(11, 11),
  },
  {
    id: 'c3',
    title: '다크 모드',
    status: 'doing',
    assignee: 'haneul',
    value: span(day(11, 12), day(11, 24)),
  },
  {
    id: 'c4',
    title: '오프라인에서 쓴 댓글이 다시 연결됐을 때 두 번 올라가는 문제',
    status: 'doing',
    assignee: 'minjae',
    value: on(11, 12),
  },
  {
    // 01:00 on the 12th in Seoul; 08:00 on the 11th in Los Angeles.
    id: 't2',
    title: '미국 팀과 화상 회의',
    status: 'todo',
    assignee: 'seoyeon',
    value: { from: at('2026-11-12T01:00:00+09:00') },
  },
  { id: 'n1', title: '스토어 스크린샷 교체', status: 'todo', assignee: 'jiwoo', value: on(11, 12) },
  {
    id: 'n2',
    title: '번역 누락 문자열 정리',
    status: 'todo',
    assignee: 'haneul',
    value: on(11, 12),
  },
  {
    id: 'n3',
    title: '크래시 리포트 분류',
    status: 'review',
    assignee: 'minjae',
    value: on(11, 12),
  },
  {
    id: 'c11',
    title: '검색 결과에서 보관된 카드를 구분해 보여 주기',
    status: 'review',
    assignee: 'haneul',
    value: on(11, 13),
  },
  {
    id: 'c2',
    title: '푸시 알림 권한을 요청하는 시점을 첫 실행에서 첫 카드 생성 직후로 옮기기',
    status: 'doing',
    assignee: 'junho',
    value: on(11, 17),
  },
  { id: 'n4', title: '디자인 리뷰', status: 'todo', assignee: 'seoyeon', value: on(11, 19) },
  {
    // A range between two moments: from the evening of the 20th to the next morning, in Seoul.
    id: 't3',
    title: '릴리스 4.0 배포와 모니터링',
    status: 'todo',
    assignee: 'minjae',
    value: span(at('2026-11-20T22:00:00+09:00'), at('2026-11-21T06:00:00+09:00')),
  },
  { id: 'n5', title: '코드 프리즈', status: 'todo', assignee: undefined, value: on(11, 24) },
  {
    id: 'c6',
    title: '태블릿 가로 화면에서 사이드바와 카드 상세를 나란히 보여 주는 배치 검토',
    status: 'todo',
    assignee: 'seoyeon',
    value: span(day(11, 26), day(11, 28)),
  },
  { id: 'n6', title: '스토어 심사 제출', status: 'todo', assignee: 'junho', value: on(11, 27) },
  {
    id: 'c8',
    title: '접근성 점검: 화면 낭독기로 카드 만들기부터 완료 처리까지',
    status: 'todo',
    assignee: 'daeun',
    value: span(day(11, 30), day(12, 4)),
  },
  {
    id: 'c9',
    title: '이미지 첨부 용량 제한 안내',
    status: 'todo',
    assignee: 'jiwoo',
    value: on(12, 5),
  },
  // Without a date: they have no place in the grid.
  {
    id: 'c5',
    title: '보드 목록 당겨서 새로 고침',
    status: 'todo',
    assignee: 'junho',
    value: undefined,
  },
  { id: 'c7', title: '앱 아이콘 교체', status: 'todo', assignee: undefined, value: undefined },
  { id: 'n7', title: '설정 화면 정리', status: 'todo', assignee: 'haneul', value: undefined },
];

export const UNDATED: readonly Entry[] = ENTRIES.filter((entry) => !entry.value);

export function entryOf(id: string): Entry | undefined {
  return ENTRIES.find((entry) => entry.id === id);
}

/** The days each dated card is on, for a reader in `timeZone`. */
export function spansFor(timeZone: string): Span[] {
  return ENTRIES.flatMap((entry) => {
    const days = entry.value && daysOf(entry.value, timeZone);
    return days ? [{ id: entry.id, ...days }] : [];
  });
}

/** The date properties of the board: what the calendar can place cards by. */
export interface DateProperty {
  id: string;
  name: string;
  /** Filled in by the server; a card cannot be moved to another day by it. */
  readOnly: boolean;
}

export const DATE_PROPERTIES: readonly DateProperty[] = [
  { id: 'schedule', name: '일정', readOnly: false },
  { id: 'due', name: '마감일', readOnly: false },
  { id: 'created', name: '만든 날짜', readOnly: true },
  { id: 'updated', name: '수정한 날짜', readOnly: true },
];

export const DISPLAY_BY: DateProperty = DATE_PROPERTIES[0] ?? {
  id: '',
  name: '',
  readOnly: false,
};

/** The fill of a bar when bars are tinted by status. Whole class names, so that they are built. */
export const STATUS_FILLS: Record<Hue, string> = {
  gray: 'bg-option-gray',
  blue: 'bg-option-blue',
  green: 'bg-option-green',
  yellow: 'bg-option-yellow',
  orange: 'bg-option-orange',
  red: 'bg-option-red',
  purple: 'bg-option-purple',
};

/** The cards the screens single out. */
export const SHOWN = {
  /** Its menu or its date picker is open; it is the one being dragged. On the 17th. */
  moved: 'c2',
  /** Where the dragged card is about to be dropped. */
  dropOn: calendarDate(2026, 11, 19),
  /** The day with more cards than a cell has room for. */
  crowded: calendarDate(2026, 11, 12),
  /** A day with no card, where a new one is made. */
  empty: calendarDate(2026, 11, 25),
  /** The card that crosses two ends of a week. */
  long: 'c3',
} as const;

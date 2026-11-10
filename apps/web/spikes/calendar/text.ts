/**
 * Every word the calendar mock-ups show that is not made-up data or taken
 * from the mobile mock-ups (`../mobile/text.ts`). A spike does not go through
 * the message files; the product will.
 *
 * The names of months and weekdays are not here: they come from `Intl` in
 * the language of the page (`names.ts`), as they will in the product.
 */
export const T = {
  // The row above the grid
  previousMonth: '이전 달',
  nextMonth: '다음 달',
  previousWeek: '이전 주',
  nextWeek: '다음 주',
  today: '오늘',
  month: '월',
  week: '주',
  monthView: '월 보기',
  weekView: '주 보기',
  span: '보는 범위',
  displayBy: '표시 기준',
  displayByHint: '카드를 놓을 날짜 속성을 고릅니다.',
  readOnly: '끌어서 옮길 수 없음',
  back: '뒤로',

  // A day
  cardsOn: (label: string, count: number) => (count ? `${label}, 카드 ${count}개` : label),
  moreOn: (count: number) => `+${count}`,
  moreOnLabel: (label: string, count: number) => `${label}의 카드 ${count}개 더 보기`,
  newCardOn: (label: string) => `${label}에 새 카드`,
  newCard: '새 카드',
  noCards: '이 날에는 카드가 없습니다.',
  untitled: '제목 없음',
  closeDay: '그날의 목록 닫기',
  continued: (nth: number, of: number) => `${of}일 중 ${nth}일째`,
  daysLong: (days: number) => `${days}일`,
  agenda: '날짜별 카드',
  weekStrip: '이 주의 날짜',

  // A card
  cardMenu: (title: string) => `${title} 메뉴`,
  changeDate: '날짜 바꾸기',
  setDate: '날짜 정하기',
  openCard: '카드 열기',
  dragging: (label: string) => `${label}에 놓기`,
  resizeStart: '시작일 끌기',
  resizeEnd: '종료일 끌기',

  // Cards without a date
  undated: '날짜 없음',
  undatedCount: (count: number) => `날짜 없음 ${count}`,
  undatedHint: '격자에 놓을 날짜가 없는 카드입니다.',
  closeUndated: '날짜 없음 닫기',
  showUndated: '날짜 없는 카드 보기',
  hideUndated: '날짜 없는 카드 접기',

  // The chooser
  chooserTitle: '캘린더 뷰의 화면 안',
  chooserLead:
    '화면과 크기를 고르면 두 안이 그 크기의 틀에 나란히 보입니다. 틀보다 큰 크기는 "이 창에서 열기"로 엽니다. 격자는 놓인 칸의 너비를 따릅니다: 544px 미만이면 줄인 격자나 일정 목록, 그 이상이면 막대가 있는 격자. 가짜 데이터의 "지금"은 2026년 11월 10일 09:00(서울)입니다.',
  chooserCase: (variant: string) => `그림 ${variant === 'a' ? 1 : 2}`,
} as const;

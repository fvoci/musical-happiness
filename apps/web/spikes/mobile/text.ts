/**
 * Every word the mock-ups show that is not made-up data. A spike does not go
 * through the message files; the product will.
 */
export const T = {
  logo: 'fvoci',

  // Sidebar and board switcher
  openSidebar: '사이드바 열기',
  findBoard: '보드 찾기',
  switchBoard: '보드 전환',
  switchBoardHint: '보드 이름으로 찾습니다.',
  noBoardFound: '찾는 보드가 없습니다.',
  addBoard: '보드 추가',
  settings: '설정',
  account: '내 계정',
  share: '공유',
  categoryMenu: (name: string) => `${name} 카테고리 메뉴`,
  boardMenu: (name: string) => `${name} 보드 메뉴`,

  // View header
  views: '뷰',
  addView: '뷰 추가',
  viewMenu: '뷰 메뉴',
  viewSettings: '보기 설정',
  renameView: '뷰 이름 바꾸기',
  duplicateView: '뷰 복제',
  exportCsv: 'CSV로 내보내기',
  deleteView: '뷰 삭제',
  search: '카드 검색',
  filter: '필터',
  sort: '정렬',
  group: '그룹',
  groupBy: '그룹 기준',
  properties: '속성',
  newCard: '새로 만들기',
  newCardShort: '새 카드',
  addFilter: '필터 추가',
  removeFilter: '필터 지우기',
  addSort: '정렬 추가',
  removeSort: '정렬 지우기',

  // Kanban
  columns: '열',
  addGroup: '그룹 추가',
  newCardIn: (column: string) => `${column}에 새 카드`,
  columnMenu: (column: string) => `${column} 열 메뉴`,
  cardMenu: '카드 메뉴',
  duplicate: '복제',
  copyLink: '링크 복사',
  moveToColumn: '다른 열로 이동',
  saveAsTemplate: '템플릿으로 저장',
  deleteCard: '삭제',

  // Properties
  title: '제목',
  status: '상태',
  assignee: '담당자',
  due: '마감일',
  priority: '우선순위',
  tags: '분류',
  hours: '예상 시간',
  empty: '비어 있음',
  addProperty: '속성 추가',
  hoursOf: (hours: number) => `${hours}시간`,
  countOf: (count: number) => `개수 ${count}`,
  sumOf: (hours: number) => `합계 ${hours}시간`,

  // Editors of a property
  options: '선택지',
  searchOptions: '선택지 찾기 또는 만들기',
  editOption: (label: string) => `${label} 선택지 고치기`,
  createOption: (label: string) => `"${label}" 선택지를 새로 만듭니다.`,
  clearValue: '값 지우기',
  endDate: '종료일',
  clearDate: '지우기',
  today: '오늘',
  done: '완료',

  // Card
  back: '뒤로',
  close: '닫기',
  body: '본문',
  attachments: '첨부',
  attachmentMenu: (name: string) => `${name} 메뉴`,
  comments: '댓글',
  writeComment: '댓글 쓰기',
  send: '보내기',

  // Stand-ins for the device
  fakeKeyboard: '가상 키보드 자리 (가짜)',

  // The chooser
  chooserTitle: '모바일 화면 안',
  chooserLead:
    '화면을 고르면 좁은 화면(360×740)의 두 안이 나란히 보입니다. 넓은 화면은 "이 창에서 열기"로 엽니다. 창의 너비가 768px보다 좁으면 좁은 화면의 배치로, 넓으면 넓은 화면의 배치로 그려집니다.',
  chooserDark: '어두운 화면',
  chooserOpen: '이 창에서 열기',
  chooserClosed: '닫힌 상태로 열기',
  chooserVariant: (variant: string) => `안 ${variant.toUpperCase()}`,
} as const;

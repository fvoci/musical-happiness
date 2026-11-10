/**
 * A made-up workspace: nothing is loaded and nothing is saved. Every value is
 * written out here, dates included, so that a screenshot of it never changes.
 */

export type Hue = 'gray' | 'blue' | 'green' | 'yellow' | 'orange' | 'red' | 'purple';

export interface Option {
  id: string;
  label: string;
  hue: Hue;
}

export interface Person {
  id: string;
  name: string;
  hue: Hue;
}

export interface Card {
  id: string;
  title: string;
  status: string;
  assignee: string | undefined;
  /** As it is shown. */
  due: string | undefined;
  priority: string | undefined;
  tags: readonly string[];
  hours: number | undefined;
  comments: number;
  attachments: number;
}

export interface BoardLink {
  id: string;
  name: string;
  hue: Hue;
}

export interface Category {
  id: string;
  name: string;
  boards: readonly BoardLink[];
}

export type ViewKind = 'kanban' | 'table' | 'gallery' | 'calendar';

export interface ViewLink {
  id: string;
  name: string;
  kind: ViewKind;
}

const FALLBACK_OPTION: Option = { id: '', label: '', hue: 'gray' };

export const STATUSES: readonly Option[] = [
  { id: 'todo', label: '할 일', hue: 'gray' },
  { id: 'doing', label: '진행 중', hue: 'blue' },
  { id: 'review', label: '검토 중', hue: 'yellow' },
  { id: 'done', label: '완료', hue: 'green' },
];

export const PRIORITIES: readonly Option[] = [
  { id: 'high', label: '높음', hue: 'red' },
  { id: 'normal', label: '보통', hue: 'orange' },
  { id: 'low', label: '낮음', hue: 'gray' },
];

export const TAGS: readonly Option[] = [
  { id: 'design', label: '디자인', hue: 'purple' },
  { id: 'front', label: '프런트엔드', hue: 'blue' },
  { id: 'server', label: '서버', hue: 'green' },
  { id: 'qa', label: 'QA', hue: 'yellow' },
  { id: 'docs', label: '문서', hue: 'gray' },
];

export const PEOPLE: readonly Person[] = [
  { id: 'seoyeon', name: '김서연', hue: 'purple' },
  { id: 'junho', name: '박준호', hue: 'blue' },
  { id: 'haneul', name: '이하늘', hue: 'green' },
  { id: 'minjae', name: '최민재', hue: 'orange' },
  { id: 'daeun', name: '정다은', hue: 'red' },
  { id: 'jiwoo', name: '한지우', hue: 'yellow' },
];

export const ME: Person = PEOPLE[0] ?? { id: '', name: '', hue: 'gray' };

export function optionOf(options: readonly Option[], id: string | undefined): Option | undefined {
  return options.find((option) => option.id === id);
}

export function statusOf(card: Card): Option {
  return optionOf(STATUSES, card.status) ?? FALLBACK_OPTION;
}

export function personOf(id: string | undefined): Person | undefined {
  return PEOPLE.find((person) => person.id === id);
}

export const CATEGORIES: readonly Category[] = [
  {
    id: 'favorites',
    name: '즐겨찾기',
    boards: [
      { id: 'roadmap', name: '2026 하반기 로드맵', hue: 'blue' },
      { id: 'bugs', name: '버그 트래커', hue: 'red' },
    ],
  },
  {
    id: 'product',
    name: '제품 개발',
    boards: [
      { id: 'app', name: '모바일 앱 개편', hue: 'purple' },
      { id: 'onboarding', name: '온보딩 개선 실험', hue: 'green' },
      { id: 'debt', name: '기술 부채 정리', hue: 'gray' },
      { id: 'minutes', name: '주간 회의록', hue: 'yellow' },
    ],
  },
  {
    id: 'marketing',
    name: '마케팅',
    boards: [
      { id: 'campaign', name: '가을 캠페인', hue: 'orange' },
      { id: 'content', name: '블로그·뉴스레터 콘텐츠 캘린더 (2026년 4분기)', hue: 'blue' },
      { id: 'beta', name: '베타 테스터 모집', hue: 'green' },
    ],
  },
  {
    id: 'personal',
    name: '개인',
    boards: [
      { id: 'books', name: '읽을 책', hue: 'yellow' },
      { id: 'moving', name: '이사 준비', hue: 'gray' },
    ],
  },
];

export const BOARD: BoardLink = { id: 'app', name: '모바일 앱 개편', hue: 'purple' };
export const BOARD_DESCRIPTION = '앱 4.0에 들어갈 작업을 한곳에서 봅니다.';

export const VIEWS: readonly ViewLink[] = [
  { id: 'progress', name: '진행 현황', kind: 'kanban' },
  { id: 'all', name: '전체 작업', kind: 'table' },
  { id: 'drafts', name: '시안 갤러리', kind: 'gallery' },
  { id: 'schedule', name: '일정', kind: 'calendar' },
];

// Titles of different lengths on purpose: one word to three lines on a phone.
export const CARDS: readonly Card[] = [
  {
    id: 'c1',
    title: '로그인 화면 다시 그리기',
    status: 'doing',
    assignee: 'seoyeon',
    due: '11월 14일',
    priority: 'high',
    tags: ['design', 'front'],
    hours: 6,
    comments: 3,
    attachments: 2,
  },
  {
    id: 'c2',
    title: '푸시 알림 권한을 요청하는 시점을 첫 실행에서 첫 카드 생성 직후로 옮기기',
    status: 'doing',
    assignee: 'junho',
    due: '11월 17일',
    priority: 'normal',
    tags: ['front'],
    hours: 4,
    comments: 5,
    attachments: 0,
  },
  {
    id: 'c3',
    title: '다크 모드',
    status: 'doing',
    assignee: 'haneul',
    due: '11월 21일',
    priority: 'normal',
    tags: ['design'],
    hours: 12,
    comments: 1,
    attachments: 1,
  },
  {
    id: 'c4',
    title: '오프라인에서 쓴 댓글이 다시 연결됐을 때 두 번 올라가는 문제',
    status: 'doing',
    assignee: 'minjae',
    due: '11월 12일',
    priority: 'high',
    tags: ['server', 'front'],
    hours: 8,
    comments: 7,
    attachments: 0,
  },
  {
    id: 'c5',
    title: '보드 목록 당겨서 새로 고침',
    status: 'todo',
    assignee: 'junho',
    due: undefined,
    priority: 'low',
    tags: ['front'],
    hours: 3,
    comments: 0,
    attachments: 0,
  },
  {
    id: 'c6',
    title: '태블릿 가로 화면에서 사이드바와 카드 상세를 나란히 보여 주는 배치 검토',
    status: 'todo',
    assignee: 'seoyeon',
    due: '11월 28일',
    priority: 'normal',
    tags: ['design'],
    hours: undefined,
    comments: 2,
    attachments: 1,
  },
  {
    id: 'c7',
    title: '앱 아이콘 교체',
    status: 'todo',
    assignee: undefined,
    due: undefined,
    priority: undefined,
    tags: [],
    hours: 1,
    comments: 0,
    attachments: 0,
  },
  {
    id: 'c8',
    title: '접근성 점검: 화면 낭독기로 카드 만들기부터 완료 처리까지',
    status: 'todo',
    assignee: 'daeun',
    due: '12월 2일',
    priority: 'high',
    tags: ['qa'],
    hours: 10,
    comments: 1,
    attachments: 0,
  },
  {
    id: 'c9',
    title: '이미지 첨부 용량 제한 안내',
    status: 'todo',
    assignee: 'jiwoo',
    due: '12월 5일',
    priority: 'low',
    tags: ['docs', 'front'],
    hours: 2,
    comments: 0,
    attachments: 0,
  },
  {
    id: 'c10',
    title: '결제 화면 문구 검수',
    status: 'review',
    assignee: 'jiwoo',
    due: '11월 11일',
    priority: 'normal',
    tags: ['docs'],
    hours: 2,
    comments: 4,
    attachments: 1,
  },
  {
    id: 'c11',
    title: '검색 결과에서 보관된 카드를 구분해 보여 주기',
    status: 'review',
    assignee: 'haneul',
    due: '11월 13일',
    priority: 'low',
    tags: ['front', 'server'],
    hours: 5,
    comments: 2,
    attachments: 0,
  },
  {
    id: 'c12',
    title: '릴리스 노트 초안',
    status: 'done',
    assignee: 'daeun',
    due: '11월 6일',
    priority: 'normal',
    tags: ['docs'],
    hours: 3,
    comments: 1,
    attachments: 1,
  },
  {
    id: 'c13',
    title: 'QA',
    status: 'done',
    assignee: 'daeun',
    due: '11월 7일',
    priority: 'high',
    tags: ['qa'],
    hours: 16,
    comments: 9,
    attachments: 3,
  },
  {
    id: 'c14',
    title: '베타 테스터 모집 공지',
    status: 'done',
    assignee: 'minjae',
    due: '11월 3일',
    priority: 'low',
    tags: [],
    hours: 1,
    comments: 0,
    attachments: 0,
  },
];

export function cardsIn(status: string): Card[] {
  return CARDS.filter((card) => card.status === status);
}

/** The card the detail screens open: its title takes three lines on a phone. */
export const OPEN_CARD: Card = CARDS.find((card) => card.id === 'c2') ?? {
  id: '',
  title: '',
  status: 'todo',
  assignee: undefined,
  due: undefined,
  priority: undefined,
  tags: [],
  hours: undefined,
  comments: 0,
  attachments: 0,
};

export const BODY = {
  paragraphs: [
    '지금은 앱을 처음 켜자마자 알림 권한을 묻는다. 무엇을 알려 주는 앱인지 모르는 상태라 거절하는 사람이 많다(지난달 허용 비율 38%).',
    '첫 카드를 만든 직후, "이 카드에 댓글이 달리면 알려 드릴까요?"라는 안내와 함께 묻도록 옮긴다. 거절하면 설정 화면에서 다시 켤 수 있다는 것을 한 번만 알려 준다.',
  ],
  checklist: [
    { text: '권한 요청 전 안내 화면 문구', done: true },
    { text: '거절했을 때의 다음 안내 시점', done: false },
    { text: 'Android 13 미만 기기에서의 동작 확인', done: false },
  ],
} as const;

export const ATTACHMENTS = [
  { name: '알림_권한_안내_시안_v3.pdf', size: '2.4MB' },
  { name: '허용 비율 추이(10월).xlsx', size: '86KB' },
] as const;

export const COMMENTS = [
  {
    author: 'seoyeon',
    when: '어제 오후 4:12',
    text: '안내 문구 시안 올렸습니다. 두 번째 안이 더 짧아서 그쪽으로 가면 좋겠어요.',
  },
  {
    author: 'minjae',
    when: '어제 오후 5:40',
    text: 'iOS는 한 번 거절하면 시스템 창을 다시 띄울 수 없어요. 그래서 우리 안내 화면에서 먼저 "나중에"를 고를 수 있게 해야 합니다.',
  },
  { author: 'junho', when: '오늘 오전 9:05', text: '확인했습니다. 목요일까지 반영할게요.' },
] as const;

export const FILTERS = [
  { property: '담당자', condition: '다음을 포함', value: '박준호, 김서연' },
  { property: '우선순위', condition: '다음이 아님', value: '낮음' },
] as const;

export const SORTS = [
  { property: '마감일', direction: '오름차순' },
  { property: '우선순위', direction: '내림차순' },
] as const;

export const GROUPABLE = ['상태', '담당자', '우선순위', '분류'] as const;

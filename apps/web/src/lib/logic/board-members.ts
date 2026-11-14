/** Direct and inherited board roles as carried by membership responses. */
export type MemberRole = 'viewer' | 'commenter' | 'editor' | 'admin' | '';

export interface BoardMember {
  boardId: string;
  userId: string;
  roles?: string;
  minimumRole: MemberRole;
  schemeAdmin: boolean;
  schemeEditor: boolean;
  schemeCommenter: boolean;
  schemeViewer: boolean;
  synthetic: boolean;
  inheritedRole?: Exclude<MemberRole, 'admin' | ''>;
}

/**
 * The diary the hub of today was asked to write today in: its page, opened
 * next, puts the caret in today's day once (where the pane is wide enough for
 * a keyboard to be the likely one).
 */
let asked: string | undefined;

export function askToWriteToday(diaryId: string): void {
  asked = diaryId;
}

/** Whether `diaryId` was asked to be written in today; asked once. */
export function takeWriteToday(diaryId: string): boolean {
  if (asked !== diaryId) return false;
  asked = undefined;
  return true;
}

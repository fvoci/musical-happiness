/**
 * Which tabs of the views fit in a row, each at its whole width.
 *
 * `widths` are the tabs' own widths in their order, `room` the width they and
 * the button of the rest ("2 more views") may take together, `more` that
 * button's width. Gives the indexes of the tabs to show, in their order. The
 * tab of the view that is shown is always among them: where the room ends
 * before it, it takes the place of the last that fits. Where even it alone
 * does not fit, it is shown alone (and is cut short), and so it is where the
 * room measured is none at all.
 *
 * Nothing measured yet (a tab of no width: a page that lays nothing out):
 * all of them.
 */
export function fittingTabs(
  widths: readonly number[],
  room: number,
  more: number,
  current: number,
): number[] {
  const all = widths.map((_, index) => index);
  if (widths.some((width) => width <= 0)) return all;
  // A fraction of a pixel of rounding is not a reason to fold a tab away.
  const total = widths.reduce((sum, width) => sum + width, 0);
  if (total <= room + 0.5) return all;

  const left = room - more + 0.5;
  const shown: number[] = [];
  let used = 0;
  for (const [index, width] of widths.entries()) {
    if (used + width > left) break;
    shown.push(index);
    used += width;
  }
  if (current < 0 || current >= widths.length || shown.includes(current)) return shown;
  const own = widths[current] ?? 0;
  while (shown.length > 0 && used + own > left) {
    used -= widths[shown.pop() ?? 0] ?? 0;
  }
  return [...shown, current];
}

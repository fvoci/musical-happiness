/**
 * Shows a comment that is on its way: the card or page it is on opens, its
 * comments are read, and then it is there (`data-comment` of the list). Once
 * it is, it is scrolled into the middle and marked for a moment
 * (`data-revealed`). A comment that does not come within `wait` (gone since,
 * or the card did not open) is let go of.
 */

/** How long the mark stays on the comment. */
const MARKED = 2000;

/** The function that stops waiting. */
export function revealComment(commentId: string, wait = 10_000): () => void {
  const selector = `[data-comment="${CSS.escape(commentId)}"]`;
  const show = (comment: Element): void => {
    const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
    comment.scrollIntoView({ block: 'center', behavior: still ? 'auto' : 'smooth' });
    comment.setAttribute('data-revealed', '');
    setTimeout(() => {
      comment.removeAttribute('data-revealed');
    }, MARKED);
  };
  const there = document.querySelector(selector);
  if (there) {
    show(there);
    return () => undefined;
  }
  const observer = new MutationObserver(() => {
    const comment = document.querySelector(selector);
    if (!comment) return;
    stop();
    show(comment);
  });
  const timer = setTimeout(stop, wait);
  function stop(): void {
    observer.disconnect();
    clearTimeout(timer);
  }
  observer.observe(document.body, { childList: true, subtree: true });
  return stop;
}

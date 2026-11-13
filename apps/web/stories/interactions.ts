import { expect, userEvent, waitFor } from 'storybook/test';

/** Portals belong to the story's document, not to its canvas element. */
export async function storyElement(canvas: HTMLElement, selector: string): Promise<HTMLElement> {
  return waitFor(() => {
    const element = canvas.ownerDocument.querySelector<HTMLElement>(selector);
    if (!element) throw new Error(`Story element not ready: ${selector}`);
    return element;
  });
}

export async function clickStory(canvas: HTMLElement, selector: string): Promise<void> {
  await userEvent.click(await visibleStoryElement(canvas, selector));
}

/** Surface elements exist before their opening transition makes them visible. */
export async function visibleStoryElement(
  canvas: HTMLElement,
  selector: string,
): Promise<HTMLElement> {
  const element = await storyElement(canvas, selector);
  await waitFor(() => expect(element).toBeVisible());
  return element;
}

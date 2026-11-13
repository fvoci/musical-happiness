import { userEvent, waitFor } from 'storybook/test';

/** Portals belong to the story's document, not to its canvas element. */
export async function storyElement(canvas: HTMLElement, selector: string): Promise<HTMLElement> {
  return waitFor(() => {
    const element = canvas.ownerDocument.querySelector<HTMLElement>(selector);
    if (!element) throw new Error(`Story element not ready: ${selector}`);
    return element;
  });
}

export async function clickStory(canvas: HTMLElement, selector: string): Promise<void> {
  await userEvent.click(await storyElement(canvas, selector));
}

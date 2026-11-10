/**
 * What is added when a body is drawn and is no part of the stored document.
 *
 * The editor (extensions.ts) and the read-only view (render.ts) both draw
 * from here, so that a link opens the same way and an image loads the same
 * way wherever a body is shown.
 */

/**
 * A link opens in a new tab, and the page it opens gets no handle on this
 * one and no credit for the link.
 */
export const LINK_ATTRIBUTES = { target: '_blank', rel: 'noopener noreferrer nofollow' } as const;

/** An image is fetched when it comes near the screen and decoded off the main thread. */
export const IMAGE_ATTRIBUTES = { loading: 'lazy', decoding: 'async' } as const;

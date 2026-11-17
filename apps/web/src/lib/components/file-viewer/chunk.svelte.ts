/**
 * The file viewer, fetched the first time a file is opened.
 *
 * Most visits view no file, so the viewer is not part of what the first
 * screen loads; the renderers and their libraries are further away still, in
 * the frame's own files (src/viewer-frame/), fetched by the frame. Once here
 * the viewer stays for the rest of the visit.
 */
import type FileViewer from './FileViewer.svelte';

class ViewerChunk {
  /** The viewer component, once it has arrived. */
  component = $state.raw<typeof FileViewer | undefined>(undefined);
  #loading: Promise<boolean> | undefined;

  /** Asks for the viewer unless it is here or on its way; says whether it arrived. */
  load(): Promise<boolean> {
    if (this.component) return Promise.resolve(true);
    this.#loading ??= import('./FileViewer.svelte')
      .then((module) => {
        this.component = module.default;
        return true;
      })
      .catch(() => false)
      .finally(() => {
        this.#loading = undefined;
      });
    return this.#loading;
  }
}

export const viewerChunk = new ViewerChunk();

/**
 * What the remote-update spike's page offers its browser tests, as `window.spike`:
 * an editor bound to a Y document that holds a body at the limit of nodes, and a
 * second document, the server's, whose letters reach it as updates.
 */

/** The times of several runs of one measurement, in milliseconds. */
export interface Runs {
  least: number;
  middle: number;
  most: number;
}

/** What the page made, once it is ready. */
export interface Opened {
  /** The nodes of the editor's document, the document itself included. */
  nodes: number;
  /** The size of the body as stored JSON, in bytes. */
  bytes: number;
  /** Whether the editor checks the content of every transaction from the shared document. */
  check: boolean;
}

/** What one letter from the server costs the editor that receives it. */
export interface Measured {
  /** `Y.applyUpdate` of the letter, with the transaction it makes in the editor and all that runs in it. */
  applied: Runs;
  /** From the update to the second frame after it, where the browser drew it. */
  drawn: Runs;
  /** The editor's transactions, and its `update` events, for each letter. */
  transactions: number;
  updates: number;
  /** Whether every letter is in the editor's document. */
  arrived: boolean;
}

/** The page's handle for the tests. */
export interface RemoteSpike {
  /** Resolves once the editor shows the whole body. */
  ready: Promise<Opened>;
  /** Sends `runs` letters, after `warm` that are not counted, one at a time. */
  measure: (runs: number, warm: number) => Promise<Measured>;
  /** `doc.check()` of the editor's document alone, as the content check calls it. */
  checked: (runs: number) => Runs;
  /** The editor's document as it would be stored (`toStoredDoc` of `getJSON`), alone. */
  stored: (runs: number) => Runs;
  /** The editor's content errors so far. */
  contentErrors: () => string[];
}

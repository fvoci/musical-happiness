/** tools/build/pdfjs-data.ts: pdf.js's data files, each its bytes in Base64, loaded on demand, by file name. */
declare module 'virtual:pdfjs-data' {
  export const cMaps: Readonly<Record<string, () => Promise<string>>>;
  export const decoders: Readonly<Record<string, () => Promise<string>>>;
}

/** pdf.js's worker module: loaded for its effect, which sets the worker up. */
declare module 'pdfjs-dist/build/pdf.worker.mjs';

/** tools/build/rhwp-wasm.ts: the HWP renderer's WebAssembly, its bytes as hexadecimal text. */
declare module 'virtual:rhwp-wasm' {
  const hex: string;
  export default hex;
}

import { createEngine } from "./wasm.js";
let engine;
self.onmessage = async ({ data }) => {
  try {
    // Keep already-open tabs from the previous deployment playable as well.
    const legacy = typeof data === "string";
    engine ||= createEngine(legacy ? undefined : data.module);
    self.postMessage({
      id: data.id,
      result: (await engine)(3, legacy ? data : data.snapshot),
    });
  } catch (error) {
    self.postMessage({ id: data.id, error: error.message });
  }
};

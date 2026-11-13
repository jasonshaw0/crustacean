import { createEngine } from "./wasm.js";
const engine = createEngine();
self.onmessage = async ({ data }) => {
  try {
    self.postMessage({ result: (await engine)(3, data) });
  } catch (error) {
    self.postMessage({ error: error.message });
  }
};

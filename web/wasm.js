let compiledModule;
export function loadEngineModule() {
  compiledModule ||= (async () => {
    const response = await fetch(new URL("./crustacean.wasm", import.meta.url));
    if (!response.ok)
      throw new Error("The engine could not be downloaded. Please reload.");
    return WebAssembly.compile(await response.arrayBuffer());
  })().catch((error) => {
    compiledModule = null;
    throw error;
  });
  return compiledModule;
}

export async function createEngine(module) {
  const instance = await WebAssembly.instantiate(
    module || (await loadEngineModule()),
    {
      env: { now_ms: () => performance.now() },
    },
  );
  const api = instance.exports;
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  return (operation, input = "", json = true) => {
    const bytes = encoder.encode(input);
    if (bytes.length > 4 * 1024 * 1024) throw new Error("Game is too large.");
    const ptr = api.input_buffer(bytes.length);
    new Uint8Array(api.memory.buffer, ptr, bytes.length).set(bytes);
    const resultPtr = api.request(operation);
    const text = decoder.decode(
      new Uint8Array(api.memory.buffer, resultPtr, api.output_len()),
    );
    if (!json) return text;
    const result = JSON.parse(text);
    if (result.error) throw new Error(result.error);
    return result;
  };
}

import { native } from "./runtime.js";
export const browserEngine = !native;

export async function createTransport(onChange, onError) {
  if (native) {
    const request = async (path, options = {}) => {
      const response = await fetch(path, {
        ...options,
        signal: AbortSignal.timeout(5000),
      });
      const data = await response.json();
      if (!response.ok)
        throw new Error(data.error || "Could not read the game.");
      return data;
    };
    return {
      state: () => request("./api/state"),
      action: (name, fields) =>
        request("./api/action", {
          method: "POST",
          body: new URLSearchParams({ action: name, ...fields }),
        }),
    };
  }
  const { createEngine } = await import("./wasm.js");
  const engine = await createEngine();
  let worker = null;
  let timer;
  let state = engine(0);
  const needsEngine = () =>
    !state.finished &&
    (state.thinking ||
      (state.running &&
        (state.mode === "engines" || state.side !== state.human)));
  function stop() {
    clearTimeout(timer);
    worker?.terminate();
    worker = null;
  }
  function schedule() {
    if (!needsEngine()) return;
    state = { ...state, thinking: true };
    const version = state.version;
    timer = setTimeout(() => {
      worker = new Worker(new URL("./engine-worker.js", import.meta.url), {
        type: "module",
      });
      const current = worker;
      const fail = (error) => {
        if (worker !== current) return;
        stop();
        state = engine(1, "action=pause");
        onChange(state);
        onError(error);
      };
      worker.onerror = () =>
        fail(new Error("Engine stopped. Resume the game to try again."));
      worker.onmessage = ({ data }) => {
        if (worker !== current || state.version !== version) return;
        if (data.error) return fail(new Error(data.error));
        try {
          state = engine(
            4,
            new URLSearchParams({ ...data.result, version }).toString(),
          );
          stop();
          schedule();
          onChange(state);
        } catch (error) {
          fail(error);
        }
      };
      worker.postMessage(engine(2, "", false));
    }, 220);
  }
  window.addEventListener("pagehide", stop);
  return {
    state: async () => state,
    action: async (name, fields) => {
      // Validate before cancelling the current search: rejected inputs change nothing.
      const next = engine(
        1,
        new URLSearchParams({ action: name, ...fields }).toString(),
      );
      stop();
      state = next;
      schedule();
      return state;
    },
  };
}

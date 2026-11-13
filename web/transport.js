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
  const { createEngine, loadEngineModule } = await import("./wasm.js");
  const module = await loadEngineModule();
  const engine = await createEngine(module);
  let worker = null;
  let searching = false;
  let searchId = 0;
  let timer;
  let state = engine(0);
  const needsEngine = () =>
    !state.finished &&
    (state.thinking ||
      (state.running &&
        (state.mode === "engines" || state.side !== state.human)));
  function stop(dispose = false) {
    clearTimeout(timer);
    searchId++;
    if (searching || dispose) {
      worker?.terminate();
      worker = null;
    }
    searching = false;
  }
  function schedule() {
    if (!needsEngine()) return;
    state = { ...state, thinking: true };
    const version = state.version;
    const id = ++searchId;
    timer = setTimeout(() => {
      worker ||= new Worker(new URL("./engine-worker.js", import.meta.url), {
        type: "module",
      });
      searching = true;
      const current = worker;
      const fail = (error) => {
        if (worker !== current || id !== searchId) return;
        stop(true);
        state = engine(1, "action=pause");
        onChange(state);
        onError(error);
      };
      worker.onerror = () =>
        fail(new Error("Engine stopped. Resume the game to try again."));
      worker.onmessage = ({ data }) => {
        if (
          worker !== current ||
          state.version !== version ||
          data.id !== id ||
          id !== searchId
        )
          return;
        if (data.error) return fail(new Error(data.error));
        try {
          state = engine(
            4,
            new URLSearchParams({ ...data.result, version }).toString(),
          );
          searching = false;
          schedule();
          onChange(state);
        } catch (error) {
          fail(error);
        }
      };
      worker.postMessage({ id, module, snapshot: engine(2, "", false) });
    }, 220);
  }
  window.addEventListener("pagehide", () => {
    stop(true);
    state = engine(1, "action=pause");
  });
  // Back/forward cache restores the JS heap too; expose the cancelled state on return.
  window.addEventListener("pageshow", (event) => {
    if (event.persisted) onChange(state);
  });
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

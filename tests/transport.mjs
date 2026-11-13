import { readFile } from "node:fs/promises";
import { Worker as NodeWorker } from "node:worker_threads";
import { setTimeout as delay } from "node:timers/promises";
import { test } from "node:test";
import assert from "node:assert/strict";
import { createTransport } from "../web/transport.js";

test("reuse browser workers and compiled wasm while cancelling obsolete searches", async () => {
  const binary = await readFile(
    new URL("../dist/crustacean.wasm", import.meta.url),
  );
  const workers = [];
  let downloads = 0;
  let posts = 0;
  const errors = [];
  const updates = [];
  globalThis.fetch = async () => {
    downloads++;
    return new Response(binary);
  };
  globalThis.window = new EventTarget();
  // Run the real browser worker and Rust search in a Node worker thread.
  globalThis.Worker = class {
    constructor(url) {
      this.thread = new NodeWorker(
        `
        const { parentPort } = require('node:worker_threads');
        globalThis.self = { postMessage: data => parentPort.postMessage(data) };
        import(${JSON.stringify(url.href)}).then(() => {
          parentPort.on('message', data => self.onmessage({ data }));
        }).catch(error => { throw error; });
      `,
        { eval: true },
      );
      this.thread.on("message", (data) => this.onmessage?.({ data }));
      this.thread.on("error", (error) => this.onerror?.(error));
      workers.push(this);
    }
    postMessage(data) {
      posts++;
      this.thread.postMessage(data);
    }
    terminate() {
      this.terminated = true;
      void this.thread.terminate();
    }
  };
  const waitFor = async (condition) => {
    const end = performance.now() + 5000;
    while (!(await condition())) {
      assert.ok(performance.now() < end, "browser engine condition timed out");
      assert.deepEqual(errors, []);
      await delay(20);
    }
  };
  const transport = await createTransport(
    (value) => updates.push(value),
    (error) => errors.push(error),
  );
  const settings = {
    mode: "engines",
    human: "white",
    white_ms: 100,
    black_ms: 100,
  };
  try {
    await transport.action("new", settings);
    await transport.action("step");
    await waitFor(async () => (await transport.state()).moves.length === 1);
    await transport.action("step");
    await waitFor(async () => (await transport.state()).moves.length === 2);
    assert.equal(
      workers.length,
      1,
      "completed searches should reuse the worker",
    );
    assert.equal(downloads, 1, "the engine binary should only be fetched once");

    await transport.action("new", { ...settings, white_ms: 1000 });
    const previousPosts = posts;
    await transport.action("step");
    await waitFor(() => posts > previousPosts);
    await assert.rejects(transport.action("load", { fen: "invalid" }));
    assert.equal((await transport.state()).thinking, true);
    const obsolete = workers[0].onmessage;
    await transport.action("pause");
    assert.equal(workers[0].terminated, true);
    obsolete({ data: { id: -1, result: { move: "e2e4" } } });
    assert.equal((await transport.state()).moves.length, 0);
    assert.equal((await transport.state()).thinking, false);

    await transport.action("new", settings);
    await transport.action("step");
    await waitFor(async () => (await transport.state()).moves.length === 1);
    assert.equal(workers.length, 2);
    assert.equal(downloads, 1);
    await transport.action("resume");
    window.dispatchEvent(new Event("pagehide"));
    const restored = new Event("pageshow");
    Object.defineProperty(restored, "persisted", { value: true });
    window.dispatchEvent(restored);
    assert.equal(updates.at(-1).running, false);
    assert.equal(updates.at(-1).thinking, false);
    assert.equal(workers.at(-1).terminated, true);
    assert.deepEqual(errors, []);
  } finally {
    window.dispatchEvent(new Event("pagehide"));
    for (const worker of workers) worker.terminate();
  }
});

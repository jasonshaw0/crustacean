import { readFile } from "node:fs/promises";
import { test } from "node:test";
import assert from "node:assert/strict";

const binary = await readFile(
  new URL("../dist/crustacean.wasm", import.meta.url),
);
async function engine() {
  const { instance } = await WebAssembly.instantiate(binary, {
    env: { now_ms: () => performance.now() },
  });
  const api = instance.exports;
  return (op, input = "", json = true) => {
    const bytes = new TextEncoder().encode(input);
    const ptr = api.input_buffer(bytes.length);
    new Uint8Array(api.memory.buffer, ptr, bytes.length).set(bytes);
    const out = api.request(op);
    const text = new TextDecoder().decode(
      new Uint8Array(api.memory.buffer, out, api.output_len()),
    );
    return json ? JSON.parse(text) : text;
  };
}
const fields = (action, data = {}) =>
  new URLSearchParams({ action, ...data }).toString();

test("browser engine plays legal replies and rejects stale results after pause/reset", async () => {
  const game = await engine();
  const worker = await engine();
  const start = game(0);
  assert.equal(start.legal_moves.length, 20);
  assert.ok(game(1, fields("move", { move: "e2e5", version: 0 })).error);
  assert.deepEqual(game(0), start);
  const moved = game(1, fields("move", { move: "e2e4", version: 0 }));
  const reply = worker(3, game(2, "", false));
  assert.ok(moved.legal_moves.includes(reply.move));
  assert.ok(reply.depth > 0);
  assert.ok(reply.elapsed_ms >= 0 && reply.elapsed_ms < 2000);
  const result = new URLSearchParams({
    ...reply,
    version: moved.version,
  }).toString();
  const played = game(4, result);
  assert.equal(played.moves.length, 2);
  assert.ok(played.analysis);
  const undone = game(1, fields("undo"));
  assert.equal(undone.fen, start.fen);
  assert.equal(undone.running, false);
  assert.ok(game(4, result).error);
  game(1, fields("resume"));
  game(1, fields("move", { move: "d2d4", version: game(0).version }));
  const stale = new URLSearchParams({
    ...reply,
    version: game(0).version,
  }).toString();
  game(1, fields("pause"));
  assert.ok(game(4, stale).error);
  game(
    1,
    fields("new", {
      mode: "player",
      human: "white",
      white_ms: 100,
      black_ms: 100,
    }),
  );
  assert.ok(game(4, stale).error);
  assert.equal(game(0).fen, start.fen);
});

test("browser promotions, castling, en passant, terminal states and bad FEN", async () => {
  const game = await engine();
  for (const [fen, move, san] of [
    ["7k/P7/8/8/8/8/8/7K w - - 0 1", "a7a8n", "a8=N"],
    ["r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1g1", "O-O"],
    ["4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2", "e5d6", "exd6"],
    ["7k/8/5KQ1/8/8/8/8/8 w - - 0 1", "g6g7", "Qg7#"],
  ]) {
    const loaded = game(1, fields("load", { fen }));
    assert.equal(loaded.fen, fen);
    const played = game(1, fields("move", { move, version: loaded.version }));
    assert.equal(played.moves[0].san, san);
  }
  const before = game(0);
  assert.equal(before.finished, true);
  assert.equal(before.result, "1-0");
  assert.ok(game(1, fields("load", { fen: "bad" })).error);
  assert.deepEqual(game(0), before);
});

test("search snapshot retains repetition history", async () => {
  const game = await engine();
  const start = game(0).fen;
  const result = game(3, `100\n${start}\n${start}\n${start}`);
  assert.equal(result.move, "");
  assert.equal(result.score, 0);
});

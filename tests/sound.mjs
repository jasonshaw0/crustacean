import { test } from "node:test";
import assert from "node:assert/strict";
import { moveSounds } from "../web/sound.js";

const game = (moves = [], fields = {}) => ({
  session: "one",
  starting_fen: "start",
  finished: false,
  mode: "player",
  human: "white",
  moves,
  ...fields,
});
const move = { uci: "e2e4", san: "e4" };

test("sound only follows forward play, never polls, imports, undo or reset", () => {
  const before = game();
  const after = game([move]);
  assert.deepEqual(moveSounds(before, after), ["move"]);
  assert.deepEqual(moveSounds(null, after), []);
  assert.deepEqual(moveSounds(after, { ...after, thinking: true }), []);
  assert.deepEqual(moveSounds(after, before), []);
  assert.deepEqual(moveSounds(before, { ...after, session: "new" }), []);
  assert.deepEqual(
    moveSounds(before, { ...after, starting_fen: "loaded" }),
    [],
  );
  assert.deepEqual(
    moveSounds(after, game([{ uci: "d2d4", san: "d4" }, move])),
    [],
  );
});

test("captures include en passant and capture promotions; castling is a move", () => {
  for (const san of ["Bxf7+", "exd6", "gxh8=Q+"])
    assert.deepEqual(moveSounds(game(), game([{ uci: "test", san }])), [
      "capture",
    ]);
  assert.deepEqual(moveSounds(game(), game([{ uci: "e1g1", san: "O-O" }])), [
    "move",
  ]);
});

test("results use the player's color and play once with the final move", () => {
  const before = game();
  const whiteWins = game([move], { finished: true, result: "1-0" });
  assert.deepEqual(moveSounds(before, whiteWins), ["move", "win"]);
  assert.deepEqual(moveSounds(before, { ...whiteWins, human: "black" }), [
    "move",
    "loss",
  ]);
  assert.deepEqual(moveSounds(before, { ...whiteWins, result: "0-1" }), [
    "move",
    "loss",
  ]);
  assert.deepEqual(
    moveSounds(before, { ...whiteWins, result: "0-1", human: "black" }),
    ["move", "win"],
  );
  assert.deepEqual(moveSounds(before, { ...whiteWins, result: "1/2-1/2" }), [
    "move",
    "draw",
  ]);
  assert.deepEqual(
    moveSounds(before, { ...whiteWins, mode: "engines", result: "0-1" }),
    ["move", "win"],
  );
  assert.deepEqual(moveSounds(whiteWins, whiteWins), []);
  assert.deepEqual(
    moveSounds(game(), game([], { finished: true, result: "1-0" })),
    [],
  );
  const captureMate = game([{ uci: "g6g7", san: "Qxg7#" }], {
    finished: true,
    result: "1-0",
  });
  assert.deepEqual(moveSounds(before, captureMate), ["capture", "win"]);
});

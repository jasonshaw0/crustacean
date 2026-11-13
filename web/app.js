import { createTransport, browserEngine } from "./transport.js";
const $ = (id) => document.getElementById(id);
const pieceNames = {
  p: "pawn",
  n: "knight",
  b: "bishop",
  r: "rook",
  q: "queen",
  k: "king",
};
const shapes = {
  p: '<circle cx="32" cy="17" r="8"/><path d="M27 25h10l4 19H23z"/><path d="M21 44h22l4 8H17z"/><path class="detail" d="M23 46h18" fill="none"/>',
  n: '<path d="M18 44c1-9 5-15 13-20l-9 3-6-6L29 9l1-6 9 6c10 7 11 20 7 35z"/><path d="M19 44h26l3 8H16z"/><path class="detail" d="m22 21 4 1m9-9 1 1m-5 24c5-4 8-8 9-14" fill="none"/>',
  b: '<path d="M32 7c-4 5-11 9-11 16 0 6 5 10 11 10s11-4 11-10c0-7-7-11-11-16z"/><path d="m27 32-5 13h20l-5-13z"/><path d="M20 44h24l4 8H16z"/><path class="detail" d="m35 16-7 10" fill="none"/><circle cx="32" cy="6" r="3"/>',
  r: '<path d="M17 10h7v7h5v-7h6v7h5v-7h7v16l-7 5 2 14H22l2-14-7-5z"/><path d="M20 44h24l4 8H16z"/><path class="detail" d="M23 27h18M26 32h12" fill="none"/>',
  q: '<path d="m17 17 9 8 6-14 6 14 9-8-7 26H24z"/><path d="M23 40h18v5H23zM20 45h24l4 7H16z"/><circle cx="17" cy="15" r="3"/><circle cx="32" cy="9" r="3"/><circle cx="47" cy="15" r="3"/><path class="detail" d="M26 35h12" fill="none"/>',
  k: '<path d="M29 5h6v6h6v5h-6v7h-6v-7h-6v-5h6z"/><path d="M32 24c-6-9-18-5-17 4 0 5 7 10 10 15h14c3-5 10-10 10-15 1-9-11-13-17-4z"/><path d="M23 40h18v6H23zM20 45h24l4 7H16z"/><path class="detail" d="M32 25v11" fill="none"/>',
};
function pieceSvg(piece) {
  return `<svg class="piece ${piece === piece.toUpperCase() ? "white" : "black"}" viewBox="0 0 64 64" aria-hidden="true">${shapes[piece.toLowerCase()]}</svg>`;
}
function colorOf(piece) {
  return piece === piece.toUpperCase() ? "white" : "black";
}
function title(s) {
  return s[0].toUpperCase() + s.slice(1);
}
function placement(fen) {
  const pieces = {};
  fen.split("/").forEach((rank, row) => {
    let file = 0;
    for (const c of rank) {
      if (/\d/.test(c)) file += Number(c);
      else {
        pieces["abcdefgh"[file] + (8 - row)] = c;
        file++;
      }
    }
  });
  return pieces;
}

let game = null;
let connected = false;
let busy = false;
let polling = false;
let serial = 0;
let flipped = false;
let selected = null;
let focusSquare = "e2";
let displayedMoves = "";
let pendingPromotion = null;
let toastTimer;
let transport;
let drag = null;
let suppressClick = false;
let droppedMove = null;
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
const squares = new Map();

function message(text, error = false) {
  clearTimeout(toastTimer);
  $("message").textContent = text;
  $("message").classList.toggle("error", error);
  $("message").hidden = false;
  toastTimer = setTimeout(
    () => {
      $("message").hidden = true;
    },
    error ? 6500 : 3000,
  );
}
function connection(value) {
  connected = value;
  $("connection").textContent = value
    ? browserEngine
      ? "Ready to play"
      : "Connected"
    : "Disconnected";
  $("connection").className = `connection ${value ? "online" : "offline"}`;
}
function canMove() {
  return (
    game &&
    connected &&
    !busy &&
    game.mode === "player" &&
    game.side === game.human &&
    game.running &&
    !game.thinking &&
    !game.finished
  );
}
function buildBoard() {
  cancelDrag();
  const fragment = document.createDocumentFragment();
  const coordinates = document.createDocumentFragment();
  squares.clear();
  for (let row = 0; row < 8; row++) {
    for (let col = 0; col < 8; col++) {
      const file = flipped ? 7 - col : col;
      const rank = flipped ? row + 1 : 8 - row;
      const coordinate = "abcdefgh"[file] + rank;
      const button = document.createElement("button");
      button.type = "button";
      button.className = "square";
      button.dataset.square = coordinate;
      button.dataset.row = String(row);
      button.dataset.col = String(col);
      button.addEventListener("click", () => {
        if (!suppressClick) void chooseSquare(coordinate);
      });
      button.addEventListener("pointerdown", (event) =>
        startDrag(event, coordinate),
      );
      button.addEventListener("focus", () => {
        focusSquare = coordinate;
        for (const [sq, el] of squares)
          el.tabIndex = sq === focusSquare ? 0 : -1;
      });
      squares.set(coordinate, button);
      fragment.append(button);
      const cell = document.createElement("div");
      cell.className =
        "coordinate-cell" + ((file + rank) % 2 === 1 ? " dark" : "");
      cell.innerHTML =
        (col === 0 ? `<span class="coord rank">${rank}</span>` : "") +
        (row === 7
          ? `<span class="coord file">${"abcdefgh"[file]}</span>`
          : "");
      coordinates.append(cell);
    }
  }
  $("board").replaceChildren(fragment);
  $("board-coordinates").replaceChildren(coordinates);
  $("evaluation-white").parentElement.classList.toggle("flipped", flipped);
  renderBoard();
}
function renderBoard() {
  if (!game) return;
  const pieces = placement(game.placement);
  const legal = selected
    ? game.legal_moves.filter((m) => m.startsWith(selected))
    : [];
  const destinations = new Set(legal.map((m) => m.slice(2, 4)));
  const last = game.moves.at(-1)?.uci;
  for (const [sq, button] of squares) {
    const piece = pieces[sq];
    const dark = ("abcdefgh".indexOf(sq[0]) + Number(sq[1])) % 2 === 1;
    button.className = [
      "square",
      dark && "dark",
      last && (last.startsWith(sq) || last.slice(2, 4) === sq) && "last",
      selected === sq && "selected",
      destinations.has(sq) && "legal",
      destinations.has(sq) && piece && "capture",
      game.check_square === sq && "check",
      drag?.active && drag.from === sq && "drag-source",
    ]
      .filter(Boolean)
      .join(" ");
    const label = `${sq}, ${piece ? `${title(colorOf(piece))} ${pieceNames[piece.toLowerCase()]}` : "empty"}${destinations.has(sq) ? ", legal move" : ""}`;
    button.setAttribute("aria-label", label);
    button.setAttribute("aria-pressed", String(selected === sq));
    button.setAttribute("aria-disabled", String(!canMove()));
    button.tabIndex = sq === focusSquare ? 0 : -1;
    const stamp = `${piece || ""}:${flipped}`;
    if (button.dataset.stamp !== stamp) {
      button.dataset.stamp = stamp;
      button.innerHTML = piece ? pieceSvg(piece) : "";
    }
  }
  $("board-hint").textContent = selected
    ? `Choose a legal square for ${selected}.`
    : game.mode === "engines"
      ? "Pause to explore one move at a time."
      : "Drag a piece, or click a piece and its destination.";
}

function render(incoming) {
  const sameSession = game?.session === incoming.session;
  if (sameSession && incoming.version < game.version) return;
  if (!sameSession) {
    flipped = incoming.mode === "player" && incoming.human === "black";
    displayedMoves = "";
  }
  if (!sameSession || game.version !== incoming.version) {
    selected = null;
    if ($("promotion-dialog").open) $("promotion-dialog").close();
    pendingPromotion = null;
  }
  const previous = game;
  if (drag && (!sameSession || incoming.version !== game.version)) cancelDrag();
  game = incoming;
  if (!squares.size || !sameSession) buildBoard();
  renderBoard();
  animateMove(previous, incoming, sameSession);
  renderFacts();
  if ($("status").textContent !== game.status)
    $("status").textContent = game.status;
  $("game-caption").textContent =
    game.mode === "engines" ? "ENGINE MATCH" : "YOUR GAME";
  $("status-detail").textContent = game.finished
    ? "Start a new game or undo to keep exploring."
    : !game.running && !game.thinking
      ? game.mode === "engines" && !game.moves.length
        ? "Start the engines, or step through a single move."
        : "Take your time. Resume whenever you’re ready."
      : game.mode === "engines"
        ? "Crustacean is playing both sides."
        : `You’re playing ${game.human}. ${game.side === game.human ? "The board is yours." : "Your opponent is finding a move."}`;
  $("mode-player").setAttribute("aria-pressed", String(game.mode === "player"));
  $("mode-engines").setAttribute(
    "aria-pressed",
    String(game.mode === "engines"),
  );
  const active = game.running || game.thinking;
  $("run").textContent = game.finished
    ? "Game over"
    : active
      ? game.mode === "engines"
        ? "Pause engines"
        : "Pause game"
      : game.mode === "engines" && !game.moves.length
        ? "Start engines"
        : "Resume game";
  $("run").disabled = !connected || busy || game.finished;
  $("step").hidden = game.mode !== "engines";
  $("step").disabled = !connected || busy || active || game.finished;
  $("undo").disabled = !connected || busy || !game.moves.length;
  $("new-game").disabled = busy || !connected;
  $("new-form").querySelector('button[type="submit"]').disabled = busy;
  $("position-form").querySelector('button[type="submit"]').disabled = busy;
  $("export").disabled = !game.moves.length;
  document.body.classList.toggle("thinking", game.thinking);
  for (const [position, color] of [
    ["top", flipped ? "white" : "black"],
    ["bottom", flipped ? "black" : "white"],
  ]) {
    const human = game.mode === "player" && game.human === color;
    $(position + "-name").textContent = human ? "You" : "Crustacean";
    $(position + "-meta").textContent = human
      ? `${title(color)} pieces`
      : `${title(color)} · ${game[color + "_ms"] / 1000} s per move`;
    const avatar = $(position + "-seat").querySelector(".avatar");
    avatar.textContent = human ? "Y" : "C";
    avatar.classList.toggle("human", human);
    $(position + "-turn").textContent =
      game.thinking && game.side === color ? "Thinking" : "To move";
    $(position + "-turn").classList.toggle(
      "active",
      !game.finished && game.side === color,
    );
  }
  const movesKey =
    game.starting_fen + ":" + game.moves.map((m) => m.uci).join(" ");
  if (movesKey !== displayedMoves) {
    displayedMoves = movesKey;
    renderMoves();
  }
  $("result-caption").textContent = game.finished
    ? game.result
    : "Standard chess";
  $("current-fen").textContent = game.fen;
  $("move-count").textContent =
    `${game.moves.length} ${game.moves.length === 1 ? "ply" : "plies"}`;
  $("analysis-label").textContent = game.thinking ? "Searching" : "Last search";
  const a = game.analysis;
  const score = a
    ? a.mate
      ? `${a.score_white < 0 ? "−" : "+"}M${Math.ceil((30000 - Math.abs(a.score_white)) / 2)}`
      : `${a.score_white >= 0 ? "+" : ""}${(a.score_white / 100).toFixed(2)}`
    : "—";
  $("score").textContent = score;
  $("evaluation-label").textContent = a ? score : "";
  $("evaluation-white").style.height =
    `${a ? 100 / (1 + Math.exp(-a.score_white / 250)) : 50}%`;
  $("evaluation-white").parentElement.setAttribute(
    "aria-label",
    `Last engine evaluation, White perspective: ${score}`,
  );
  $("depth").textContent = a ? String(a.depth) : "—";
  $("nodes").textContent = a
    ? new Intl.NumberFormat("en", {
        notation: "compact",
        maximumFractionDigits: 1,
      }).format(a.nodes)
    : "—";
  $("pv").textContent = a?.pv || "The engine’s line will appear here.";
}
function renderMoves() {
  const container = $("move-list");
  if (!game.moves.length) {
    container.innerHTML =
      '<div class="empty-history"><span class="empty-board" aria-hidden="true">◇</span><p>A fresh board. Your first move.</p></div>';
    return;
  }
  const rows = new Map();
  const fragment = document.createDocumentFragment();
  game.moves.forEach((move, index) => {
    if (!rows.has(move.number)) {
      const row = document.createElement("div");
      row.className = "move-row";
      const number = document.createElement("span");
      number.textContent = `${move.number}.`;
      row.append(
        number,
        document.createElement("span"),
        document.createElement("span"),
      );
      rows.set(move.number, row);
      fragment.append(row);
    }
    const cell = rows.get(move.number).children[move.color === "white" ? 1 : 2];
    cell.textContent = move.san;
    cell.setAttribute(
      "aria-label",
      `${move.number}. ${title(move.color)} ${move.san}`,
    );
    if (index === game.moves.length - 1) cell.className = "latest";
  });
  container.replaceChildren(fragment);
  container.scrollTop = container.scrollHeight;
}

function renderFacts() {
  const pieces = placement(game.placement);
  const values = { p: 1, n: 3, b: 3, r: 5, q: 9, k: 0 };
  const material = Object.values(pieces).reduce(
    (sum, p) =>
      sum + values[p.toLowerCase()] * (colorOf(p) === "white" ? 1 : -1),
    0,
  );
  $("material-fact").textContent =
    material === 0
      ? "Equal"
      : `${material > 0 ? "White" : "Black"} +${Math.abs(material)}`;
  $("turn-fact").textContent = game.finished ? game.result : title(game.side);
  $("last-fact").textContent = game.moves.at(-1)?.san || "—";
  $("board-status").textContent = !connected
    ? "Engine disconnected"
    : game.finished
      ? game.status
      : !game.running && !game.thinking
        ? "Game paused"
        : game.thinking
          ? `${title(game.side)} is thinking…`
          : game.check_square
            ? "You are in check"
            : `${title(game.side)} to move`;
  $("board-state").textContent = game.finished
    ? game.result
    : game.check_square
      ? "CHECK"
      : `Move ${game.fen.split(" ")[5]}`;
  $("board-state").classList.toggle(
    "in-check",
    !!game.check_square && !game.finished,
  );
}

function animateMove(before, after, sameSession) {
  if (
    !sameSession ||
    !before ||
    reducedMotion.matches ||
    after.moves.length !== before.moves.length + 1
  )
    return;
  const move = after.moves.at(-1).uci;
  if (move === droppedMove) {
    droppedMove = null;
    return;
  }
  const from = move.slice(0, 2),
    to = move.slice(2, 4);
  const pairs = [[from, to]];
  const piece = placement(before.placement)[from];
  if (
    piece?.toLowerCase() === "k" &&
    Math.abs(from.charCodeAt(0) - to.charCodeAt(0)) === 2
  ) {
    pairs.push([
      `${to[0] === "g" ? "h" : "a"}${from[1]}`,
      `${to[0] === "g" ? "f" : "d"}${from[1]}`,
    ]);
  }
  for (const [source, destination] of pairs) {
    const origin = squares.get(source).getBoundingClientRect();
    const target = squares.get(destination);
    const end = target.getBoundingClientRect();
    const svg = target.querySelector(".piece");
    if (!svg) continue;
    target.classList.add("moving");
    const animation = svg.animate(
      [
        {
          transform: `translate(${origin.x - end.x}px, ${origin.y - end.y}px)`,
        },
        { transform: "translate(0, 0)" },
      ],
      { duration: 190, easing: "cubic-bezier(.2,.7,.3,1)" },
    );
    const finish = () => target.classList.remove("moving");
    animation.finished.then(finish, finish);
  }
}

function startDrag(event, from) {
  if (event.button !== 0 || !event.isPrimary || !canMove()) return;
  const piece = placement(game.placement)[from];
  if (!piece || colorOf(piece) !== game.human) return;
  cancelDrag();
  drag = {
    from,
    piece,
    x: event.clientX,
    y: event.clientY,
    pointer: event.pointerId,
    version: game.version,
    active: false,
    button: event.currentTarget,
  };
  drag.button.setPointerCapture(event.pointerId);
}
function cancelDrag() {
  if (!drag) return;
  const old = drag;
  drag = null;
  old.ghost?.remove();
  old.button.classList.remove("drag-source");
  for (const button of squares.values()) button.classList.remove("drop-target");
  if (old.button.hasPointerCapture(old.pointer))
    old.button.releasePointerCapture(old.pointer);
}
$("board").addEventListener("pointermove", (event) => {
  if (!drag || event.pointerId !== drag.pointer) return;
  if (
    !drag.active &&
    Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 5
  )
    return;
  event.preventDefault();
  if (!drag.active) {
    drag.active = true;
    selected = drag.from;
    renderBoard();
    const size = drag.button.getBoundingClientRect().width;
    drag.ghost = document.createElement("div");
    drag.ghost.className = "drag-ghost";
    drag.ghost.setAttribute("aria-hidden", "true");
    drag.ghost.innerHTML = pieceSvg(drag.piece);
    drag.ghost.style.width = drag.ghost.style.height = `${size}px`;
    document.body.append(drag.ghost);
  }
  drag.ghost.style.left = `${event.clientX}px`;
  drag.ghost.style.top = `${event.clientY}px`;
  const target = document
    .elementFromPoint(event.clientX, event.clientY)
    ?.closest("[data-square]");
  for (const button of squares.values())
    button.classList.toggle(
      "drop-target",
      button === target && button.classList.contains("legal"),
    );
});
$("board").addEventListener("pointerup", (event) => {
  if (!drag || event.pointerId !== drag.pointer) return;
  const { active, from, version } = drag;
  const to = document
    .elementFromPoint(event.clientX, event.clientY)
    ?.closest("[data-square]")?.dataset.square;
  cancelDrag();
  if (!active) return;
  // The browser emits a click after pointerup; consume that click, not the next real tap.
  suppressClick = true;
  setTimeout(() => {
    suppressClick = false;
  }, 0);
  const moves = game.legal_moves.filter((m) => m.startsWith(from + to));
  if (canMove() && game.version === version && moves.length) {
    selected = from;
    if (moves.length > 1) showPromotion(moves);
    else {
      droppedMove = moves[0];
      void playMove(moves[0], version);
    }
  } else {
    selected = from;
    renderBoard();
  }
});
for (const event of ["pointercancel", "lostpointercapture"]) {
  $("board").addEventListener(event, () => {
    cancelDrag();
    renderBoard();
  });
}
window.addEventListener("blur", () => {
  cancelDrag();
  renderBoard();
});

async function poll() {
  if (!transport || polling || busy) return;
  polling = true;
  const ticket = serial;
  try {
    const value = await transport.state();
    if (ticket === serial && !busy) {
      connection(true);
      render(value);
    }
  } catch {
    if (ticket === serial && !busy) {
      connection(false);
      if (game) render(game);
      else {
        $("status").textContent = "Engine disconnected";
        $("status-detail").textContent =
          "Check that the Crustacean GUI server is running.";
      }
    }
  } finally {
    polling = false;
  }
}
async function action(name, fields = {}) {
  if (busy || !transport) return false;
  cancelDrag();
  busy = true;
  serial++;
  if (game) render(game);
  try {
    const data = await transport.action(name, fields);
    connection(true);
    busy = false;
    render(data);
    return true;
  } catch (error) {
    message(
      error.name === "TimeoutError" || error.name === "TypeError"
        ? "The engine is not responding. Your last confirmed position is still on the board."
        : error.message,
      true,
    );
    return false;
  } finally {
    busy = false;
    if (game) render(game);
    void poll();
  }
}
async function chooseSquare(sq) {
  if (!canMove()) return;
  const pieces = placement(game.placement);
  if (selected === sq) {
    selected = null;
    renderBoard();
    return;
  }
  if (selected) {
    const candidates = game.legal_moves.filter((m) =>
      m.startsWith(selected + sq),
    );
    if (candidates.length > 1) {
      showPromotion(candidates);
      return;
    }
    if (candidates.length === 1) {
      await playMove(candidates[0], game.version);
      return;
    }
  }
  selected = pieces[sq] && colorOf(pieces[sq]) === game.human ? sq : null;
  renderBoard();
}
async function playMove(move, version) {
  if (await action("move", { move, version })) {
    selected = null;
    renderBoard();
  }
}
function showPromotion(candidates) {
  pendingPromotion = { candidates, version: game.version };
  const container = $("promotion-choices");
  container.replaceChildren();
  for (const move of candidates) {
    const kind = move[4];
    const button = document.createElement("button");
    button.type = "button";
    button.setAttribute("aria-label", `Promote to ${title(pieceNames[kind])}`);
    button.innerHTML =
      pieceSvg(game.human === "white" ? kind.toUpperCase() : kind) +
      `<span>${title(pieceNames[kind])}</span>`;
    button.addEventListener("click", async () => {
      const pending = pendingPromotion;
      $("promotion-dialog").close();
      pendingPromotion = null;
      if (pending) await playMove(move, pending.version);
    });
    container.append(button);
  }
  $("promotion-dialog").showModal();
}
function closePromotion() {
  $("promotion-dialog").close();
  pendingPromotion = null;
  selected = null;
  renderBoard();
}
$("cancel-promotion").addEventListener("click", closePromotion);
$("promotion-dialog").addEventListener("cancel", () => {
  pendingPromotion = null;
  selected = null;
  renderBoard();
});

function setupMode() {
  const mode = $("new-form").elements.mode.value;
  const human = $("new-form").elements.human.value;
  $("color-settings").hidden = mode === "engines";
  $("white-time").disabled = mode === "player" && human === "white";
  $("black-time").disabled = mode === "player" && human === "black";
}
function openSetup(mode) {
  if (!game || busy) return;
  $("new-form").elements.mode.value = mode || game.mode;
  $("new-form").elements.human.value = game.human;
  $("white-time").value = String(game.white_ms);
  $("black-time").value = String(game.black_ms);
  $("replace-note").hidden = !game.moves.length;
  setupMode();
  $("new-dialog").showModal();
}
$("new-game").addEventListener("click", () => openSetup());
$("mode-player").addEventListener("click", () => openSetup("player"));
$("mode-engines").addEventListener("click", () => openSetup("engines"));
$("new-form").addEventListener("change", setupMode);
for (const id of ["close-new", "cancel-new"])
  $(id).addEventListener("click", () => $("new-dialog").close());
$("new-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const mode = $("new-form").elements.mode.value;
  const human = $("new-form").elements.human.value;
  if (
    await action("new", {
      mode,
      human,
      white_ms: $("white-time").value,
      black_ms: $("black-time").value,
    })
  ) {
    $("new-dialog").close();
    flipped = mode === "player" && human === "black";
    buildBoard();
    render(game);
  }
});
$("run").addEventListener("click", () =>
  action(game.running || game.thinking ? "pause" : "resume"),
);
$("step").addEventListener("click", () => action("step"));
$("undo").addEventListener("click", () => action("undo"));
$("flip").addEventListener("click", () => {
  flipped = !flipped;
  buildBoard();
  if (game) render(game);
});
$("position-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  if (await action("load", { fen: $("fen-input").value.trim() }))
    message("Position loaded.");
});
$("copy-fen").addEventListener("click", async () => {
  if (!game) return;
  try {
    await navigator.clipboard.writeText(game.fen);
    message("FEN copied.");
  } catch {
    message("Select the FEN text to copy it from this browser.");
  }
});
$("board").addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    cancelDrag();
    selected = null;
    renderBoard();
    return;
  }
  const button = event.target.closest("[data-square]");
  if (!button) return;
  const row = Number(button.dataset.row);
  const col = Number(button.dataset.col);
  let nextRow = row;
  let nextCol = col;
  if (event.key === "ArrowLeft") nextCol--;
  else if (event.key === "ArrowRight") nextCol++;
  else if (event.key === "ArrowUp") nextRow--;
  else if (event.key === "ArrowDown") nextRow++;
  else if (event.key === "Home") nextCol = 0;
  else if (event.key === "End") nextCol = 7;
  else return;
  event.preventDefault();
  if (nextRow >= 0 && nextRow < 8 && nextCol >= 0 && nextCol < 8)
    $("board").children[nextRow * 8 + nextCol].focus();
});

function exportPgn() {
  if (!game) return;
  const date = new Date();
  const day = [
    date.getFullYear(),
    String(date.getMonth() + 1).padStart(2, "0"),
    String(date.getDate()).padStart(2, "0"),
  ].join(".");
  const player = (color) =>
    game.mode === "player" && game.human === color ? "Player" : "Crustacean";
  const tags = [
    `[Event "Crustacean game"]`,
    `[Site "Crustacean"]`,
    `[Date "${day}"]`,
    `[White "${player("white")}"]`,
    `[Black "${player("black")}"]`,
    `[Result "${game.result}"]`,
  ];
  const start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
  if (game.starting_fen !== start)
    tags.push('[SetUp "1"]', `[FEN "${game.starting_fen}"]`);
  const moves = [];
  game.moves.forEach((m, index) => {
    if (m.color === "white") moves.push(`${m.number}.`);
    else if (index === 0) moves.push(`${m.number}...`);
    moves.push(m.san);
  });
  moves.push(game.result);
  const url = URL.createObjectURL(
    new Blob([tags.join("\n") + "\n\n" + moves.join(" ") + "\n"], {
      type: "application/x-chess-pgn",
    }),
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = `crustacean-${day.replaceAll(".", "-")}.pgn`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
$("export").addEventListener("click", exportPgn);
try {
  transport = await createTransport(
    (value) => {
      connection(true);
      render(value);
    },
    (error) => message(error.message, true),
  );
  $("session-note").textContent = browserEngine
    ? "No account. No install. This tab holds your game; export PGN before closing or reloading."
    : "Your game stays here while the server is running.";
  await poll();
  if (!browserEngine) setInterval(() => void poll(), 150);
} catch (error) {
  connection(false);
  $("status").textContent = "Engine could not start";
  $("status-detail").textContent = error.message;
  $("new-game").disabled = true;
  message(error.message, true);
}

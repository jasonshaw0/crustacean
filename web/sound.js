const preferenceKey = "crustacean.sound";

// Only appended moves count: polls, undo, reset and imported positions stay quiet.
export function moveSounds(before, after) {
  if (
    !before ||
    before.finished ||
    before.session !== after.session ||
    before.starting_fen !== after.starting_fen ||
    after.moves.length <= before.moves.length ||
    !before.moves.every((move, index) => move.uci === after.moves[index].uci)
  )
    return [];
  const sounds = [after.moves.at(-1).san.includes("x") ? "capture" : "move"];
  if (after.finished) {
    const winner = after.result === "1-0" ? "white" : "black";
    sounds.push(
      after.result === "1/2-1/2"
        ? "draw"
        : after.mode === "engines" || winner === after.human
          ? "win"
          : "loss",
    );
  }
  return sounds;
}

// Soft taps and short, rounded chimes; no samples, requests or audio dependency.
const notes = {
  move: [[0, 320, 0.075, 0.028]],
  capture: [
    [0, 190, 0.09, 0.034],
    [0.035, 285, 0.065, 0.016],
  ],
  win: [
    [0, 440, 0.15, 0.022],
    [0.12, 554.37, 0.17, 0.022],
    [0.24, 659.25, 0.23, 0.02],
  ],
  loss: [
    [0, 329.63, 0.18, 0.022],
    [0.16, 261.63, 0.26, 0.02],
  ],
  draw: [
    [0, 392, 0.16, 0.02],
    [0.14, 392, 0.2, 0.018],
  ],
};

export function createSoundPlayer(button) {
  let enabled = true;
  try {
    enabled = localStorage.getItem(preferenceKey) !== "off";
  } catch {}
  let context;
  const voices = new Set();
  function updateButton() {
    button.setAttribute("aria-pressed", String(enabled));
    button.title = enabled
      ? "Sound on · click to mute"
      : "Sound off · click to enable";
  }
  function unlock() {
    if (!enabled) return;
    try {
      const Audio = globalThis.AudioContext || globalThis.webkitAudioContext;
      if (!Audio) return;
      context ||= new Audio({ latencyHint: "interactive" });
      if (context.state !== "running") void context.resume().catch(() => {});
    } catch {
      /* Audio availability must never interrupt a game. */
    }
  }
  function silence() {
    for (const voice of voices) {
      voice.oscillator.stop();
      voice.oscillator.disconnect();
      voice.gain.disconnect();
    }
    voices.clear();
  }
  function play(sounds) {
    if (!enabled || document.hidden || context?.state !== "running") return;
    try {
      const start = context.currentTime + 0.008;
      sounds.forEach((sound, index) => {
        for (const [offset, frequency, duration, volume] of notes[sound] ||
          []) {
          const time = start + index * 0.14 + offset;
          const oscillator = context.createOscillator();
          const gain = context.createGain();
          oscillator.type = "sine";
          oscillator.frequency.setValueAtTime(frequency, time);
          if (sound === "move" || sound === "capture")
            oscillator.frequency.exponentialRampToValueAtTime(
              frequency * 0.6,
              time + duration,
            );
          gain.gain.setValueAtTime(0, time);
          gain.gain.linearRampToValueAtTime(volume, time + 0.006);
          gain.gain.exponentialRampToValueAtTime(0.0001, time + duration);
          gain.gain.linearRampToValueAtTime(0, time + duration + 0.01);
          oscillator.connect(gain).connect(context.destination);
          const voice = { oscillator, gain };
          voices.add(voice);
          oscillator.onended = () => {
            voices.delete(voice);
            oscillator.disconnect();
            gain.disconnect();
          };
          oscillator.start(time);
          oscillator.stop(time + duration + 0.015);
        }
      });
    } catch {
      silence();
    }
  }
  updateButton();
  button.addEventListener("click", () => {
    enabled = !enabled;
    try {
      localStorage.setItem(preferenceKey, enabled ? "on" : "off");
    } catch {}
    updateButton();
    if (enabled) unlock();
    else silence();
  });
  // Touch browsers may only unlock on release; include it for drag gestures too.
  for (const event of ["pointerdown", "pointerup", "keydown", "click"])
    document.addEventListener(event, unlock, { capture: true, passive: true });
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) silence();
  });
  window.addEventListener("pagehide", silence);
  return { play };
}

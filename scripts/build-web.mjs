import { spawnSync } from "node:child_process";
import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";
import { homedir } from "node:os";
import { existsSync } from "node:fs";

const root = fileURLToPath(new URL("../", import.meta.url));
const localCargo = join(
  homedir(),
  ".cargo",
  "bin",
  process.platform === "win32" ? "cargo.exe" : "cargo",
);
const result = spawnSync(
  existsSync(localCargo) ? localCargo : "cargo",
  [
    "build",
    "--release",
    "--locked",
    "--lib",
    "--target",
    "wasm32-unknown-unknown",
  ],
  { cwd: root, stdio: "inherit" },
);
if (result.status !== 0) process.exit(result.status || 1);
const target = resolve(root, process.env.CARGO_TARGET_DIR || "target");
const assets = new Map();
for (const name of (await readdir(join(root, "web"))).sort()) {
  if (/\.(html|css|js|svg)$/.test(name))
    assets.set(name, await readFile(join(root, "web", name)));
}
assets.set(
  "crustacean.wasm",
  await readFile(
    join(target, "wasm32-unknown-unknown", "release", "crustacean.wasm"),
  ),
);
const hash = createHash("sha256");
for (const [name, bytes] of assets) hash.update(name).update(bytes);
const version = hash.digest("hex").slice(0, 12);
await mkdir(join(root, "dist"), { recursive: true });
for (const [name, bytes] of assets) {
  // Version the full module graph, including worker imports and the wasm URL.
  const content = /\.(html|js)$/.test(name)
    ? bytes
        .toString("utf8")
        .replace(
          /(["'])(\.\/([\w.-]+\.(?:js|css|svg|wasm)))\1/g,
          (match, quote, url, file) =>
            assets.has(file) ? `${quote}${url}?v=${version}${quote}` : match,
        )
    : bytes;
  await writeFile(join(root, "dist", name), content);
}
console.log("Crustacean is ready in dist/.");

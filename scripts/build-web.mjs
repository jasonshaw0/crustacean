import { spawnSync } from "node:child_process";
import { mkdir, readdir, copyFile } from "node:fs/promises";
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
await mkdir(join(root, "dist"), { recursive: true });
for (const name of await readdir(join(root, "web"))) {
  if (/\.(html|css|js|svg)$/.test(name))
    await copyFile(join(root, "web", name), join(root, "dist", name));
}
const target = resolve(root, process.env.CARGO_TARGET_DIR || "target");
await copyFile(
  join(target, "wasm32-unknown-unknown", "release", "crustacean.wasm"),
  join(root, "dist", "crustacean.wasm"),
);
console.log("Crustacean is ready in dist/.");

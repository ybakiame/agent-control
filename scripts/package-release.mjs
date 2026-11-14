// Assemble a portable Windows release zip from `cargo build --release` output.
// Usage: node scripts/package-release.mjs   (run after `mise run release` builds, or standalone)
import { copyFileSync, mkdirSync, rmSync, writeFileSync, readFileSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const root = resolve(import.meta.dirname, "..");
const cargo = readFileSync(join(root, "Cargo.toml"), "utf8");
const ver = /version = "([^"]+)"/.exec(cargo)[1];
const name = `agent-control-v${ver}-windows-x64`;
const dist = join(root, "dist");
const stage = join(dist, name);
const rel = join(root, "target", "release");

rmSync(stage, { recursive: true, force: true });
mkdirSync(stage, { recursive: true });

for (const exe of ["agent-control-app.exe", "agent-control-mcp.exe"]) {
  const src = join(rel, exe);
  if (!existsSync(src)) {
    console.error(`missing ${src}; run \`cargo build --release --workspace\` first`);
    process.exit(1);
  }
  copyFileSync(src, join(stage, exe));
}
copyFileSync(join(root, "README.md"), join(stage, "README.md"));
copyFileSync(join(root, "README.zh-CN.md"), join(stage, "README.zh-CN.md"));
copyFileSync(join(root, "crates", "app", "assets", "icon.png"), join(stage, "icon.png"));
// Ship the hook script beside the exes so register_hooks can resolve it without a repo checkout.
copyFileSync(join(root, "scripts", "agent-control-hook.mjs"), join(stage, "agent-control-hook.mjs"));
// Ship portable by default: the store lives beside the exe, never in the global home dir.
writeFileSync(join(stage, "portable.marker"), "");

const zip = join(dist, `${name}.zip`);
rmSync(zip, { force: true });
const ps = spawnSync(
  "powershell.exe",
  ["-NoProfile", "-Command", `Compress-Archive -Path '${stage}\\*' -DestinationPath '${zip}' -Force`],
  { stdio: "inherit" },
);
if (ps.status !== 0) {
  console.error("Compress-Archive failed");
  process.exit(1);
}
console.log(`RELEASE=${zip}`);
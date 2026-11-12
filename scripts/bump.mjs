#!/usr/bin/env node
// Bumps the version in package.json, src-tauri/tauri.conf.json and
// src-tauri/Cargo.toml together, then commits and tags v<version>
// (and pushes with --push). The release workflow runs on the tag.
//
// Usage:
//   npm run bump                    # patch (0.1.0 -> 0.1.1)
//   npm run bump -- minor           # 0.1.0 -> 0.2.0
//   npm run bump -- major           # 0.1.0 -> 1.0.0
//   npm run bump -- 0.4.2           # explicit version
//   npm run bump -- minor --push    # also push the branch and the tag
//   npm run bump -- --dry-run       # show what would change; touches nothing

import { readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const dryRun = args.includes("--dry-run");
const push = args.includes("--push");
const kind = args.find((a) => !a.startsWith("-")) ?? "patch";

const SEMVER = /^\d+\.\d+\.\d+$/;

function die(msg) {
  console.error(`x ${msg}`);
  process.exit(1);
}

function git(...a) {
  return execFileSync("git", a, { cwd: root, encoding: "utf8" }).trim();
}

// The current version; tauri.conf.json is the source of truth.
const confPath = join(root, "src-tauri", "tauri.conf.json");
const conf = JSON.parse(readFileSync(confPath, "utf8"));
const current = conf.version;
if (!SEMVER.test(current)) die(`invalid version in tauri.conf.json: ${current}`);

let next;
if (SEMVER.test(kind)) {
  next = kind;
} else {
  const [maj, min, pat] = current.split(".").map(Number);
  if (kind === "major") next = `${maj + 1}.0.0`;
  else if (kind === "minor") next = `${maj}.${min + 1}.0`;
  else if (kind === "patch") next = `${maj}.${min}.${pat + 1}`;
  else die(`unknown bump "${kind}" (patch|minor|major|x.y.z)`);
}
if (next === current) die(`already at ${next}`);

console.log(`bump: ${current} -> ${next}`);

if (!dryRun) {
  const dirty = git("status", "--porcelain");
  if (dirty) die(`the working tree has uncommitted changes; commit or stash them first:\n${dirty}`);
  const tag = `v${next}`;
  if (git("tag", "--list", tag)) die(`tag ${tag} already exists`);
}

// The release runner has a newer npm than many machines, and it rejects a
// lockfile that an older npm wrote without some optional dependencies
// (v0.1.0 and v0.1.2 failed this way). Check with the latest npm first.
console.log("  checking package-lock.json with the latest npm...");
try {
  execFileSync(
    "npx",
    ["-y", "npm@latest", "ci", "--dry-run", "--ignore-scripts", "--no-audit", "--no-fund"],
    { cwd: root, stdio: "pipe", shell: process.platform === "win32" },
  );
} catch (e) {
  const out = `${e.stdout ?? ""}${e.stderr ?? ""}`
    .split("\n")
    .filter((l) => /npm error/.test(l))
    .slice(0, 4)
    .join("\n");
  die(
    `the latest npm rejects package-lock.json; regenerate it with\n` +
      `  npx -y npm@latest install --package-lock-only\n${out}`,
  );
}

function bumpJson(path, label) {
  const raw = readFileSync(path, "utf8");
  const replaced = raw.replace(/("version"\s*:\s*")\d+\.\d+\.\d+(")/, `$1${next}$2`);
  if (replaced === raw) die(`no version field found in ${label}`);
  if (!dryRun) writeFileSync(path, replaced);
  console.log(`  updated ${label}`);
}

function bumpCargo(path, label) {
  const raw = readFileSync(path, "utf8");
  // Only the first version line, which belongs to [package].
  const replaced = raw.replace(/^version = "\d+\.\d+\.\d+"/m, `version = "${next}"`);
  if (replaced === raw) die(`no version line found in ${label}`);
  if (!dryRun) writeFileSync(path, replaced);
  console.log(`  updated ${label}`);
}

bumpJson(join(root, "package.json"), "package.json");
bumpJson(confPath, "src-tauri/tauri.conf.json");
bumpCargo(join(root, "src-tauri", "Cargo.toml"), "src-tauri/Cargo.toml");

if (dryRun) {
  console.log("\n(dry run: no files or git state changed)");
  process.exit(0);
}

// Keep Cargo.lock in step so the release commit doesn't leave it behind.
// --workspace updates only the workspace members, not dependencies.
try {
  execFileSync("cargo", ["update", "--workspace"], { cwd: root, stdio: "pipe" });
  console.log("  updated Cargo.lock");
} catch (e) {
  die(`cargo update --workspace failed (Cargo.lock would keep the old version): ${e.message}`);
}

const tag = `v${next}`;
git("add", "package.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "Cargo.lock");
git("commit", "-m", `chore(release): ${tag}`);
git("tag", tag);
console.log(`\nok: committed and tagged ${tag}`);

if (push) {
  const branch = git("rev-parse", "--abbrev-ref", "HEAD");
  git("push", "origin", branch);
  git("push", "origin", tag);
  console.log(`ok: pushed ${branch} and ${tag}; the release workflow is starting`);
} else {
  console.log(`\nnext: git push origin HEAD && git push origin ${tag}`);
}

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { spawnSync } = require("node:child_process");
const { reject, HEX, verifyManifest, parseStrictJson, compareVersions } = require("./manifest.js");
const APP_NAME = "Dev Cockpit.app";
const JOURNAL_DIR = ".dev-cockpit-update";
const PHASES = ["preparing", "prepared", "applying", "applied", "committed", "failed", "rollingBack", "rolledBack"];
const exists = (p) => { try { fs.lstatSync(p); return true; } catch (e) { if (e.code === "ENOENT") return false; throw e; } };

function regularFile(p) {
  const st = fs.lstatSync(p);
  if (!st.isFile() || st.nlink !== 1) reject("state_invalid");
}
function directory(p) {
  if (!fs.lstatSync(p).isDirectory()) reject("state_invalid");
}
function syncDir(p) {
  const fd = fs.openSync(p, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW);
  try { fs.fsyncSync(fd); } finally { fs.closeSync(fd); }
}
function atomicWrite(file, value) {
  const temp = `${file}.${crypto.randomUUID()}.tmp`;
  const fd = fs.openSync(temp, "wx", 0o600);
  try { fs.writeFileSync(fd, JSON.stringify(value) + "\n"); fs.fsyncSync(fd); }
  finally { fs.closeSync(fd); }
  // An interrupted temp file is deliberately retained; never treated as state.
  fs.renameSync(temp, file);
  syncDir(path.dirname(file));
}
function rename(from, to) {
  if (exists(to)) reject("state_invalid");
  fs.renameSync(from, to);
  syncDir(path.dirname(from));
  syncDir(path.dirname(to));
}
function hashFile(file) {
  const hash = crypto.createHash("sha256");
  const fd = fs.openSync(file, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW);
  const buffer = Buffer.alloc(65536);
  try {
    let count;
    while ((count = fs.readSync(fd, buffer, 0, buffer.length, null))) hash.update(buffer.subarray(0, count));
  } finally { fs.closeSync(fd); }
  return hash.digest("hex");
}

// Manifest v1 tree hash: pre-order UTF-8 byte-sorted entries, including root '.'.
// kind NUL relative-path NUL decimal(mode & 0777) NUL content-hash/link-target LF.
// No traversal through links. Every link must resolve inside the bundle.
function bundleHash(app, durable = false) {
  directory(app);
  const root = fs.realpathSync(app);
  const hash = crypto.createHash("sha256");
  let entries = 0, total = 0;
  function walk(relative) {
    if (++entries > 100000 || /[\x00-\x1f\x7f]/.test(relative)) reject("bundle_invalid");
    const file = relative === "." ? root : path.join(root, relative);
    const st = fs.lstatSync(file);
    let kind, content = "";
    if (st.isSymbolicLink()) {
      kind = "l"; content = fs.readlinkSync(file, "utf8");
      const resolved = fs.realpathSync(file);
      if (path.isAbsolute(content) || !resolved.startsWith(root + path.sep) || /[\x00-\x1f\x7f]/.test(content)) reject("bundle_invalid");
    } else if (st.isDirectory()) kind = "d";
    else if (st.isFile() && st.nlink === 1) {
      kind = "f"; total += st.size;
      if (total > 2 * 1024 ** 3) reject("bundle_invalid");
      content = hashFile(file);
      if (durable) {
        const fd = fs.openSync(file, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW);
        try { fs.fsyncSync(fd); } finally { fs.closeSync(fd); }
      }
    } else reject("bundle_invalid");
    hash.update(`${kind}\0${relative}\0${st.mode & 0o777}\0${content}\n`, "utf8");
    if (kind === "d") {
      const names = fs.readdirSync(file, { encoding: "buffer" }).sort(Buffer.compare);
      for (const bytes of names) {
        const name = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
        walk(relative === "." ? name : `${relative}/${name}`);
      }
      if (durable) syncDir(file);
    }
  }
  walk(".");
  return hash.digest("hex");
}
function layout(installDir, id) {
  const root = path.join(installDir, JOURNAL_DIR);
  const tx = id ? path.join(root, id) : null;
  return { root, state: path.join(root, "state.json"), app: path.join(installDir, APP_NAME),
    tx, stage: tx && path.join(tx, "stage", APP_NAME),
    previous: tx && path.join(tx, "previous.app"), rejected: tx && path.join(tx, "rejected.app") };
}
function ensureRoot(installDir) {
  directory(installDir);
  const p = layout(installDir);
  if (!exists(p.root)) { fs.mkdirSync(p.root, { mode: 0o700 }); syncDir(installDir); }
  directory(p.root);
  const st = fs.statSync(p.root);
  if (st.uid !== process.getuid() || (st.mode & 0o077)) reject("state_invalid");
  return p;
}
async function withLock(installDir, action) {
  const p = ensureRoot(installDir);
  const fd = fs.openSync(path.join(p.root, "lock"), fs.constants.O_CREAT | fs.constants.O_RDWR | fs.constants.O_NOFOLLOW, 0o600);
  try {
    const lock = fs.fstatSync(fd);
    if (!lock.isFile() || lock.nlink !== 1) reject("state_invalid");
    // lockf uses BSD flock. The inherited descriptor shares its open-file
    // description with this process, so the lock survives lockf's exit and
    // is released by close/SIGKILL/reboot. Rust startup uses the same flock.
    const result = spawnSync("/usr/bin/lockf", ["-s", "-t", "0", "3"], { stdio: ["ignore", "ignore", "ignore", fd] });
    if (result.status !== 0) reject("update_busy");
    return await action();
  } finally { fs.closeSync(fd); }
}
function readState(installDir, keys) {
  const p = layout(installDir);
  if (!exists(p.state)) {
    // Orphan transactions are ambiguous; preserve them and the current app.
    if (fs.readdirSync(p.root).some((name) => name !== "lock")) reject("state_invalid");
    return null;
  }
  regularFile(p.state);
  if (fs.statSync(p.state).size > 131072) reject("state_invalid");
  let s;
  try { s = parseStrictJson(fs.readFileSync(p.state, "utf8")); } catch { reject("state_invalid"); }
  if (!s || Object.keys(s).sort().join() !== ["schemaVersion", "id", "phase", "manifest", "previousHash"].sort().join() ||
      s.schemaVersion !== 1 || typeof s.id !== "string" || !/^[a-f0-9]{32}$/.test(s.id) ||
      !PHASES.includes(s.phase) || !(s.previousHash === null || (typeof s.previousHash === "string" && HEX.test(s.previousHash)))) reject("state_invalid");
  verifyManifest(JSON.stringify(s.manifest), keys);
  const txPaths = layout(installDir, s.id);
  directory(txPaths.tx);
  if (exists(path.dirname(txPaths.stage))) directory(path.dirname(txPaths.stage));
  return s;
}
function save(installDir, s, phase) {
  s.phase = phase;
  atomicWrite(layout(installDir).state, s);
}
function rollback(installDir, s) {
  const p = layout(installDir, s.id);
  save(installDir, s, "rollingBack");
  const current = exists(p.app) ? bundleHash(p.app) : null;
  if (exists(p.previous)) {
    if (!s.previousHash || bundleHash(p.previous) !== s.previousHash) reject("state_invalid");
    if (current !== null) {
      if (current !== s.manifest.payload.bundleSha256 || exists(p.rejected)) reject("state_invalid");
      rename(p.app, p.rejected);
    }
    rename(p.previous, p.app);
  } else if (current !== s.previousHash) {
    if (s.previousHash === null && current === s.manifest.payload.bundleSha256 && !exists(p.rejected)) rename(p.app, p.rejected);
    else reject("state_invalid");
  }
  save(installDir, s, "rolledBack");
  return "rolledBack";
}
function recover(installDir, keys, onStatus = () => {}) {
  const s = readState(installDir, keys);
  if (!s) return "idle";
  const p = layout(installDir, s.id);
  if (s.phase === "committed") {
    if (!exists(p.app) || bundleHash(p.app) !== s.manifest.payload.bundleSha256) reject("bundle_invalid");
    return "committed";
  }
  if (s.phase === "rolledBack") {
    if ((exists(p.app) ? bundleHash(p.app) : null) !== s.previousHash) reject("state_invalid");
    return "rolledBack";
  }
  onStatus("recovering");
  // A rename may have completed just before the phase write. Re-verify the
  // signed tree before committing; stage presence distinguishes pre-apply.
  if (["applying", "applied"].includes(s.phase) && !exists(p.stage) && exists(p.app) &&
      bundleHash(p.app) === s.manifest.payload.bundleSha256) {
    save(installDir, s, "committed");
    onStatus("committed");
    return "committed";
  }
  const status = rollback(installDir, s);
  onStatus(status);
  return status;
}

// download/extract/checkpoint are dependency-injection seams for deterministic
// crash tests. The CLI never accepts keys or hooks from arguments/environment.
async function install({ installDir, manifestText, keys, channel, download, extract,
  onStatus = () => {}, checkpoint = () => {}, currentVersion }) {
  const manifest = verifyManifest(manifestText, keys, channel);
  return withLock(installDir, async () => {
    recover(installDir, keys, onStatus);
    const prior = readState(installDir, keys);
    const app = layout(installDir).app;
    if (prior?.phase === "committed") {
      if (compareVersions(manifest.payload.version, prior.manifest.payload.version) < 0) reject("downgrade_rejected");
      if (manifest.payload.version === prior.manifest.payload.version) {
        if (manifest.payload.bundleSha256 !== prior.manifest.payload.bundleSha256 ||
            manifest.payload.sha256 !== prior.manifest.payload.sha256) reject("version_conflict");
        return "committed";
      }
    }
    if (exists(app) && currentVersion && compareVersions(manifest.payload.version, currentVersion(app)) < 0) reject("downgrade_rejected");
    const id = crypto.randomBytes(16).toString("hex");
    const p = layout(installDir, id);
    const s = { schemaVersion: 1, id, phase: "preparing", manifest,
      previousHash: exists(app) ? bundleHash(app) : null };
    fs.mkdirSync(p.tx, { mode: 0o700 });
    save(installDir, s, "preparing");
    checkpoint("preparing");
    try {
      const zip = path.join(p.tx, "artifact.zip");
      await download(manifest.payload.downloadUrl, zip);
      regularFile(zip);
      if (hashFile(zip) !== manifest.payload.sha256) reject("artifact_invalid");
      fs.mkdirSync(path.dirname(p.stage), { mode: 0o700 });
      await extract(zip, path.dirname(p.stage), manifest.payload);
      if (fs.readdirSync(path.dirname(p.stage)).join() !== APP_NAME ||
          bundleHash(p.stage, true) !== manifest.payload.bundleSha256) reject("bundle_invalid");
      save(installDir, s, "prepared"); checkpoint("prepared");
      // Recheck immediately before touching the working app, including hooks.
      if (bundleHash(p.stage) !== manifest.payload.bundleSha256 ||
          (exists(app) ? bundleHash(app) : null) !== s.previousHash) reject("bundle_invalid");
      save(installDir, s, "applying"); checkpoint("applying");
      if (s.previousHash) rename(app, p.previous);
      checkpoint("afterBackup");
      rename(p.stage, app); checkpoint("afterReplace");
      save(installDir, s, "applied"); checkpoint("applied");
      if (bundleHash(app) !== manifest.payload.bundleSha256) reject("bundle_invalid");
      save(installDir, s, "committed"); checkpoint("committed");
      onStatus("committed");
      return "committed";
    } catch (error) {
      if (s.phase === "committed") throw error;
      // Tests use actual process death to bypass this catch, just like a reboot.
      save(installDir, s, "failed");
      rollback(installDir, s);
      onStatus("rolledBack");
      throw error;
    }
  });
}

module.exports = { APP_NAME, JOURNAL_DIR, exists, bundleHash, hashFile, layout,
  atomicWrite, withLock, readState, recover, install };

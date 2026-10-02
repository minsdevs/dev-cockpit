#!/usr/bin/env node
"use strict";
// Signed manifest v1 installer. Published unsigned 1.0.1 is a separate legacy path.
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { UpdateError, reject, verifyManifest, TRUSTED_KEYS, MAX_MANIFEST_BYTES } = require("./manifest.js");
const { APP_NAME, withLock, readState, recover, install } = require("./transaction.js");
const APP_VERSION = require("./package.json").version;

const MESSAGES = {
  manifest_invalid: "업데이트 안내 파일 형식이 잘못되어 설치를 중단했습니다.",
  signature_invalid: "서명 검증에 실패했습니다. 기존 앱과 사용자 데이터는 보존됩니다.",
  key_unknown: "신뢰할 수 있는 배포 공개키가 없습니다. 설치를 중단했습니다.",
  algorithm_unsupported: "지원하지 않는 서명 방식입니다. 설치를 중단했습니다.",
  channel_mismatch: "선택한 업데이트 채널과 배포 정보가 일치하지 않습니다.",
  artifact_invalid: "다운로드 파일이 변조되었거나 손상되었습니다. 설치를 중단했습니다.",
  bundle_invalid: "앱 파일 검증에 실패했습니다. 기존 앱과 복구 파일을 보존했습니다.",
  state_invalid: "설치 기록이 손상되었거나 파일 상태가 불명확합니다. 앱과 복구 파일을 보존했습니다.",
  update_busy: "다른 설치 또는 복구가 진행 중입니다. 완료 후 다시 실행하세요.",
  downgrade_rejected: "이전 버전으로의 업데이트를 차단했습니다.",
  version_conflict: "같은 버전의 파일 내용이 달라 설치를 차단했습니다.",
  network_failed: "업데이트를 다운로드하지 못했습니다. 네트워크 연결을 확인하세요.",
  apply_failed: "새 앱 적용에 실패했습니다. 복구 상태를 확인하세요.",
  recovering: "중단된 설치를 복구하고 있습니다…",
  rolledBack: "이전 설치 상태로 복원했습니다. 사용자 데이터는 보존됩니다.",
  committed: "서명과 앱 파일 검증을 완료했습니다. 설치가 완료되었습니다.",
};
function safeMessage(error) { return MESSAGES[error?.code] || MESSAGES.apply_failed; }
function fail(message) { console.error(`error: ${message}`); process.exit(1); }
function onStatus(status) { if (MESSAGES[status]) console.log(MESSAGES[status]); }

function parseArgs(argv) {
  const args = { dir: null, open: true, channel: "stable", preserveSettings: true };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--dir") {
      args.dir = argv[++i] || fail("--dir requires a path");
    } else if (a === "--no-open") {
      args.open = false;
    } else if (a === "--channel") {
      const ch = argv[++i];
      if (ch !== "stable" && ch !== "beta") fail("--channel must be stable or beta");
      args.channel = ch;
    } else if (a === "--no-preserve") {
      fail("--no-preserve is no longer supported; user settings are always preserved.");
    } else if (a === "--help" || a === "-h") {
      console.log(
        `Dev Cockpit installer v${APP_VERSION}\n\n` +
          `Usage: npx @min0504/dev-cockpit [options]\n\n` +
          `Options:\n` +
          `  --dir <path>     Install directory (default: /Applications, falls back to ~/Applications)\n` +
          `  --no-open        Do not launch the app after installing\n` +
          `  --channel <ch>   Update channel: stable (default) or beta\n` +
          `  -h, --help       Show this help\n`
      );
      process.exit(0);
    } else {
      fail(`unknown option: ${a} (see --help)`);
    }
  }
  return args;
}

function pickInstallDir(preferred) {
  if (preferred) {
    fs.mkdirSync(preferred, { recursive: true });
    return preferred;
  }
  try {
    fs.accessSync("/Applications", fs.constants.W_OK);
    return "/Applications";
  } catch {
    const home = path.join(os.homedir(), "Applications");
    fs.mkdirSync(home, { recursive: true });
    console.log("note: /Applications is not writable, installing to ~/Applications");
    return home;
  }
}

// HTTPS-only, bounded streaming, explicit redirect validation (including GitHub CDN).
async function download(url, destination, limit = 512 * 1024 * 1024) {
  let response;
  const signal = AbortSignal.timeout(120000);
  for (let count = 0; count < 6; count++) {
    const parsed = new URL(url);
    const allowed = ["github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"];
    if (parsed.protocol !== "https:" || parsed.username || parsed.password || parsed.port || !allowed.includes(parsed.hostname)) reject("network_failed");
    try { response = await fetch(url, { redirect: "manual", signal }); }
    catch { reject("network_failed"); }
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      const location = response.headers.get("location");
      await response.body?.cancel();
      if (!location) reject("network_failed");
      url = new URL(location, url).href;
      continue;
    }
    break;
  }
  if (!response?.ok || !response.body || Number(response.headers.get("content-length")) > limit) reject("network_failed");
  const chunks = [], fd = destination ? fs.openSync(destination, "wx", 0o600) : null;
  let total = 0;
  try {
    for await (const chunk of response.body) {
      total += chunk.length;
      if (total > limit) reject("network_failed");
      if (fd !== null) fs.writeFileSync(fd, chunk);
      else chunks.push(Buffer.from(chunk));
    }
    if (fd !== null) fs.fsyncSync(fd);
  } finally { if (fd !== null) fs.closeSync(fd); }
  if (fd === null) return new TextDecoder("utf-8", { fatal: true }).decode(Buffer.concat(chunks));
}
function plistValue(app, field) {
  const result = spawnSync("/usr/bin/plutil", ["-extract", field, "raw", "-o", "-", path.join(app, "Contents", "Info.plist")], { encoding: "utf8", maxBuffer: 4096 });
  if (result.status !== 0) reject("bundle_invalid");
  return result.stdout.trim();
}
function currentVersion(app) {
  if (plistValue(app, "CFBundleIdentifier") !== "com.min0504.devcockpit") reject("bundle_invalid");
  return plistValue(app, "CFBundleShortVersionString");
}
function validateArchive(zip) {
  // Validate both central and local names before ditto sees any path. V1 accepts
  // ordinary files/directories only (no ZIP64, encryption, links or device nodes).
  const fd = fs.openSync(zip, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW);
  const size = fs.fstatSync(fd).size;
  const read = (offset, length) => {
    if (offset < 0 || length < 0 || offset + length > size) reject("bundle_invalid");
    const data = Buffer.alloc(length);
    if (fs.readSync(fd, data, 0, length, offset) !== length) reject("bundle_invalid");
    return data;
  };
  try {
    const tail = read(Math.max(0, size - 65557), Math.min(size, 65557));
    let end = tail.length - 22;
    while (end >= 0 && (tail.readUInt32LE(end) !== 0x06054b50 || end + 22 + tail.readUInt16LE(end + 20) !== tail.length)) end--;
    if (end < 0) reject("bundle_invalid");
    const count = tail.readUInt16LE(end + 10), length = tail.readUInt32LE(end + 12), offset = tail.readUInt32LE(end + 16);
    if (tail.readUInt16LE(end + 4) || tail.readUInt16LE(end + 6) ||
        tail.readUInt16LE(end + 8) !== count || !count || count === 65535 || length > 8 * 1024 * 1024 ||
        offset + length !== size - tail.length + end) reject("bundle_invalid");
    const central = read(offset, length), seen = new Set(), ranges = [];
    let cursor = 0, total = 0;
    for (let i = 0; i < count; i++) {
      if (cursor + 46 > length || central.readUInt32LE(cursor) !== 0x02014b50) reject("bundle_invalid");
      const nameLength = central.readUInt16LE(cursor + 28), extra = central.readUInt16LE(cursor + 30), comment = central.readUInt16LE(cursor + 32);
      const compressed = central.readUInt32LE(cursor + 20), uncompressed = central.readUInt32LE(cursor + 24);
      const flags = central.readUInt16LE(cursor + 8), method = central.readUInt16LE(cursor + 10);
      const type = (central.readUInt32LE(cursor + 38) >>> 16) & 0xf000;
      const localOffset = central.readUInt32LE(cursor + 42);
      if (cursor + 46 + nameLength + extra + comment > length || (flags & 1) ||
          ![0, 8].includes(method) || ![0, 0x4000, 0x8000].includes(type) || central.readUInt16LE(cursor + 34)) reject("bundle_invalid");
      const nameBytes = central.subarray(cursor + 46, cursor + 46 + nameLength);
      const name = new TextDecoder("utf-8", { fatal: true }).decode(nameBytes);
      const parts = name.replace(/\/$/, "").split("/");
      const folded = name.replace(/\/$/, "").normalize("NFD").toLowerCase();
      if (parts[0] !== APP_NAME || parts.some((part) => !part || part === "." || part === "..") ||
          name.includes("\\") || /[\x00-\x1f\x7f]/.test(name) || seen.has(folded)) reject("bundle_invalid");
      seen.add(folded);
      total += uncompressed;
      if (total > 2 * 1024 ** 3) reject("bundle_invalid");
      const local = read(localOffset, 30);
      if (local.readUInt32LE(0) !== 0x04034b50 || local.readUInt16LE(6) !== flags || local.readUInt16LE(8) !== method ||
          local.readUInt16LE(26) !== nameLength || !read(localOffset + 30, nameLength).equals(nameBytes)) reject("bundle_invalid");
      const finish = localOffset + 30 + nameLength + local.readUInt16LE(28) + compressed;
      if (finish > offset) reject("bundle_invalid");
      ranges.push([localOffset, finish]);
      cursor += 46 + nameLength + extra + comment;
    }
    if (cursor !== length) reject("bundle_invalid");
    ranges.sort((a, b) => a[0] - b[0]);
    if (ranges.some((range, i) => i && range[0] < ranges[i - 1][1])) reject("bundle_invalid");
  } finally { fs.closeSync(fd); }
}
function extract(zip, stage, payload) {
  validateArchive(zip);
  // ditto extracts into a fresh private sibling staging directory, never /Applications.
  const result = spawnSync("/usr/bin/ditto", ["-x", "-k", zip, stage], { stdio: "ignore" });
  if (result.status !== 0 || currentVersion(path.join(stage, APP_NAME)) !== payload.version) reject("bundle_invalid");
}
async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (process.platform !== "darwin" || process.arch !== "arm64") fail("Dev Cockpit requires macOS Apple Silicon.");
  const installDir = fs.realpathSync(pickInstallDir(args.dir));
  // Offline recovery happens before attempting a new network operation.
  const restored = await withLock(installDir, () => {
    const before = readState(installDir, TRUSTED_KEYS);
    const status = recover(installDir, TRUSTED_KEYS, onStatus);
    return before?.phase !== "rolledBack" && status === "rolledBack";
  });
  if (restored) return;
  if (!Object.keys(TRUSTED_KEYS).length) reject("key_unknown");
  const manifestUrl = args.channel === "beta"
    ? "https://github.com/minsdevs/dev-cockpit/releases/download/beta/update-manifest.json"
    : "https://github.com/minsdevs/dev-cockpit/releases/latest/download/update-manifest.json";
  const manifestText = await download(manifestUrl, null, MAX_MANIFEST_BYTES);
  verifyManifest(manifestText, TRUSTED_KEYS, args.channel);
  await install({ installDir, manifestText, keys: TRUSTED_KEYS, channel: args.channel,
    download, extract, currentVersion, onStatus });
  // Never remove quarantine. Gatekeeper evaluates the installed app normally.
  if (args.open) {
    const opened = spawnSync("/usr/bin/open", [path.join(installDir, APP_NAME)], { stdio: "ignore" });
    if (opened.status !== 0) console.log("설치는 완료됐지만 앱을 열지 못했습니다. Finder에서 앱을 실행하세요.");
  }
}
if (require.main === module) main().catch((error) => fail(safeMessage(error)));
module.exports = { download, extract, validateArchive, currentVersion, safeMessage, UpdateError };

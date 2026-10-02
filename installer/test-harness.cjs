// Local test support only; excluded from the npm installer files allowlist.
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { canonicalPayload } = require("./manifest.js");
const { APP_NAME, bundleHash, hashFile, install, withLock, recover } = require("./transaction.js");

function makeApp(app, contents = "new application", version = "2.0.0") {
  fs.mkdirSync(path.join(app, "Contents", "MacOS"), { recursive: true, mode: 0o755 });
  fs.chmodSync(app, 0o755);
  fs.writeFileSync(path.join(app, "Contents", "MacOS", "dev-cockpit"), contents, { mode: 0o755 });
  fs.writeFileSync(path.join(app, "Contents", "Info.plist"), `<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.min0504.devcockpit</string>
<key>CFBundleShortVersionString</key><string>${version}</string></dict></plist>`);
}
function sign(payload, keyId = "ephemeral") {
  // Private key is never serialized, returned, written or logged.
  const { privateKey, publicKey } = crypto.generateKeyPairSync("ed25519");
  const m = { schemaVersion: 1, algorithm: "Ed25519", keyId, payload, signature: "" };
  m.signature = crypto.sign(null, canonicalPayload(m), privateKey).toString("base64");
  const keys = { [keyId]: publicKey.export({ format: "der", type: "spki" }).subarray(-32).toString("base64") };
  return { manifest: m, keys, manifestText: JSON.stringify(m) };
}
function fixture(root) {
  const source = path.join(root, "source", APP_NAME);
  makeApp(source);
  const zip = path.join(root, "input.zip");
  fs.writeFileSync(zip, "test archive content");
  const payload = { payloadVersion: 1, version: "2.0.0", channel: "stable", platform: "darwin-aarch64",
    sha256: hashFile(zip), bundleSha256: bundleHash(source),
    downloadUrl: "https://github.com/minsdevs/dev-cockpit/releases/download/v2.0.0/Dev-Cockpit-2.0.0-macos-aarch64.zip",
    releaseNotes: "한글 \"safe\"\n🛠\t\u2028", publishedAt: "2026-09-23T00:00:00Z" };
  const signed = sign(payload);
  const installDir = path.join(root, "Applications");
  fs.mkdirSync(installDir);
  makeApp(path.join(installDir, APP_NAME), "old application", "1.0.1");
  fs.writeFileSync(path.join(installDir, "user-data.txt"), "keep user data");
  return { ...signed, root, source, zip, installDir, channel: "stable",
    download: async (_url, destination) => fs.copyFileSync(zip, destination),
    extract: async (_zip, destination) => fs.cpSync(source, path.join(destination, APP_NAME), { recursive: true }),
  };
}
async function main() {
  const [operation, root, phase] = process.argv.slice(2);
  if (operation === "crash") {
    const data = fixture(root);
    // Cross-process/Rust tests need only the ephemeral PUBLIC key.
    fs.writeFileSync(path.join(root, "public.json"), JSON.stringify(data.keys), { mode: 0o600 });
    await install({ ...data, checkpoint: (at) => {
      if (at === phase) process.kill(process.pid, "SIGKILL");
    } });
  } else if (operation === "recover") {
    const keys = JSON.parse(fs.readFileSync(path.join(root, "public.json"), "utf8"));
    const installDir = path.join(root, "Applications");
    const status = await withLock(installDir, () => recover(installDir, keys));
    process.stdout.write(status);
  } else if (operation === "vector") {
    const data = fixture(root);
    process.stdout.write(JSON.stringify({ manifest: data.manifest, keys: data.keys,
      canonical: canonicalPayload(data.manifest).toString("base64"), bundleHash: bundleHash(data.source) }));
  } else if (operation === "lock") {
    await withLock(root, () => process.stdout.write("acquired"));
  } else if (operation === "hold") {
    await withLock(root, () => new Promise((resolve) => {
      process.stdout.write("locked\n");
      process.stdin.resume();
      process.stdin.on("end", resolve);
    }));
  }
}
if (require.main === module) main().catch(() => { process.exitCode = 1; });
module.exports = { makeApp, sign, fixture };

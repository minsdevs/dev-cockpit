#!/usr/bin/env node
// No key generation or key files. CI supplies a private key in memory via a
// secret environment variable; only signed public metadata is written to stdout.
const fs = require("node:fs");
const crypto = require("node:crypto");
const { canonicalPayload, verifyManifest, TRUSTED_KEYS, reject } = require("../installer/manifest.js");
const { hashFile, bundleHash } = require("../installer/transaction.js");
const { validateArchive, currentVersion } = require("../installer/install.js");

function createManifest({ app, archive, version, channel, keyId, privateKey, keys = TRUSTED_KEYS, publishedAt }) {
  if (!Object.hasOwn(keys, keyId)) reject("key_unknown");
  validateArchive(archive);
  if (currentVersion(app) !== version) reject("manifest_invalid");
  const manifest = { schemaVersion: 1, algorithm: "Ed25519", keyId, signature: "", payload: {
    payloadVersion: 1, version, channel, platform: "darwin-aarch64", sha256: hashFile(archive),
    bundleSha256: bundleHash(app),
    downloadUrl: `https://github.com/minsdevs/dev-cockpit/releases/download/v${version}/Dev-Cockpit-${version}-macos-aarch64.zip`,
    releaseNotes: `Release ${version} (${channel})`, publishedAt,
  } };
  if (privateKey.asymmetricKeyType !== "ed25519") reject("algorithm_unsupported");
  manifest.signature = crypto.sign(null, canonicalPayload(manifest), privateKey).toString("base64");
  verifyManifest(JSON.stringify(manifest), keys, channel);
  return manifest;
}
function main() {
  const [mode, first, archive, app] = process.argv.slice(2);
  if (mode === "create") {
    const privateKey = crypto.createPrivateKey(process.env.UPDATE_SIGNING_KEY_PEM || "");
    const manifest = createManifest({ app: first, archive,
      version: process.env.RELEASE_VERSION, channel: process.env.RELEASE_CHANNEL,
      keyId: process.env.UPDATE_KEY_ID, privateKey,
      publishedAt: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
    });
    process.stdout.write(JSON.stringify(manifest) + "\n");
  } else if (mode === "verify") {
    const manifest = verifyManifest(fs.readFileSync(first, "utf8"), TRUSTED_KEYS);
    validateArchive(archive);
    if (hashFile(archive) !== manifest.payload.sha256 || bundleHash(app) !== manifest.payload.bundleSha256 ||
        currentVersion(app) !== manifest.payload.version) reject("artifact_invalid");
  } else reject("manifest_invalid");
}
if (require.main === module) {
  try { main(); } catch { console.error("Manifest generation/verification failed; release publishing is blocked."); process.exitCode = 1; }
}
module.exports = { createManifest };

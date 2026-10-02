"use strict";

const crypto = require("node:crypto");
const TRUSTED_KEYS = require("./trusted-keys.json").keys;
const MAX_MANIFEST_BYTES = 65536;
const DOMAIN = "dev-cockpit:update-manifest:v1\n";
const PAYLOAD_FIELDS = ["payloadVersion", "version", "channel", "platform", "sha256",
  "bundleSha256", "downloadUrl", "releaseNotes", "publishedAt"];
const ENVELOPE_FIELDS = ["schemaVersion", "algorithm", "keyId", "payload", "signature"];
const HEX = /^[a-f0-9]{64}$/;

class UpdateError extends Error {
  constructor(code) { super(code); this.code = code; }
}
function reject(code = "manifest_invalid") { throw new UpdateError(code); }
function exactFields(object, fields) {
  if (!object || typeof object !== "object" || Array.isArray(object) ||
      Object.keys(object).length !== fields.length ||
      fields.some((key) => !Object.hasOwn(object, key))) reject();
}

// JSON.parse alone silently accepts duplicate members. Reject them before using
// the parsed value; the first parse already validates the JSON token grammar.
function parseStrictJson(text) {
  let value;
  try { value = JSON.parse(text); } catch { reject(); }
  const stack = [];
  for (let i = 0; i < text.length; i++) {
    if (text[i] === "{") stack.push(new Set());
    else if (text[i] === "[") stack.push(null);
    else if (text[i] === "}" || text[i] === "]") stack.pop();
    else if (text[i] === '"') {
      const start = i++;
      while (i < text.length && text[i] !== '"') {
        if (text[i] === "\\") i++;
        i++;
      }
      const token = JSON.parse(text.slice(start, i + 1));
      let next = i + 1;
      while (/\s/.test(text[next] || "x")) next++;
      if (text[next] === ":") {
        const members = stack.at(-1);
        if (!members || members.has(token)) reject();
        members.add(token);
      }
    }
  }
  return value;
}
function versionParts(value) {
  if (typeof value !== "string" || !/^(0|[1-9]\d{0,8})\.(0|[1-9]\d{0,8})\.(0|[1-9]\d{0,8})$/.test(value)) reject();
  return value.split(".").map(Number);
}
function compareVersions(a, b) {
  const left = versionParts(a), right = versionParts(b);
  for (let i = 0; i < 3; i++) if (left[i] !== right[i]) return Math.sign(left[i] - right[i]);
  return 0;
}
function validateEnvelope(manifest) {
  exactFields(manifest, ENVELOPE_FIELDS);
  if (manifest.schemaVersion !== 1) reject();
  if (manifest.algorithm !== "Ed25519") reject("algorithm_unsupported");
  if (typeof manifest.keyId !== "string" || !/^[A-Za-z0-9_-]{1,64}$/.test(manifest.keyId)) reject();
  if (typeof manifest.signature !== "string") reject();
  const p = manifest.payload;
  exactFields(p, PAYLOAD_FIELDS);
  if (p.payloadVersion !== 1) reject();
  for (const field of PAYLOAD_FIELDS.slice(1)) {
    if (typeof p[field] !== "string") reject();
    // UTF-8 encoders otherwise replace lone UTF-16 surrogates differently.
    for (const char of p[field]) {
      const point = char.codePointAt(0);
      if (point >= 0xd800 && point <= 0xdfff) reject();
    }
  }
  versionParts(p.version);
  if (!["stable", "beta"].includes(p.channel) || p.platform !== "darwin-aarch64" ||
      !HEX.test(p.sha256) || !HEX.test(p.bundleSha256) ||
      Buffer.byteLength(p.releaseNotes) > 16384 ||
      !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(p.publishedAt)) reject();
  const expected = `https://github.com/minsdevs/dev-cockpit/releases/download/v${p.version}/Dev-Cockpit-${p.version}-macos-aarch64.zip`;
  if (p.downloadUrl !== expected) reject();
}
function canonicalPayload(manifest) {
  validateEnvelope(manifest);
  return Buffer.from(DOMAIN + JSON.stringify([
    manifest.schemaVersion, manifest.algorithm, manifest.keyId,
    ...PAYLOAD_FIELDS.map((key) => manifest.payload[key]),
  ]), "utf8");
}
function decodeBase64(value, length) {
  if (typeof value !== "string") reject();
  const decoded = Buffer.from(value, "base64");
  if (decoded.length !== length || decoded.toString("base64") !== value) reject();
  return decoded;
}
function verifyManifest(text, keys = TRUSTED_KEYS, channel) {
  if (typeof text !== "string" || Buffer.byteLength(text) > MAX_MANIFEST_BYTES) reject();
  const manifest = parseStrictJson(text);
  const bytes = canonicalPayload(manifest);
  if (!Object.hasOwn(keys, manifest.keyId)) reject("key_unknown");
  const raw = decodeBase64(keys[manifest.keyId], 32);
  const signature = decodeBase64(manifest.signature, 64);
  let valid = false;
  try {
    const key = crypto.createPublicKey({ format: "der", type: "spki",
      key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), raw]) });
    valid = crypto.verify(null, bytes, key, signature);
  } catch { reject("signature_invalid"); }
  if (!valid) reject("signature_invalid");
  if (channel && manifest.payload.channel !== channel) reject("channel_mismatch");
  return manifest;
}

module.exports = { UpdateError, reject, HEX, MAX_MANIFEST_BYTES, TRUSTED_KEYS,
  parseStrictJson, canonicalPayload, verifyManifest, compareVersions };

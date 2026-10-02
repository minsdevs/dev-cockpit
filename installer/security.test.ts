import { afterEach, describe, expect, it, vi } from "vitest";
import { createRequire } from "node:module";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { generateKeyPairSync } from "node:crypto";
const require = createRequire(import.meta.url);
const { fixture, sign } = require("./test-harness.cjs");
const { verifyManifest, canonicalPayload, parseStrictJson } = require("./manifest.js");
const { APP_NAME, install, withLock, recover, layout, bundleHash, hashFile } = require("./transaction.js");
const { extract, validateArchive, currentVersion, safeMessage, download } = require("./install.js");
const { createManifest } = require("../scripts/update-manifest.cjs");
const roots: string[] = [];
function temp() { const root = fs.mkdtempSync(path.join(os.tmpdir(), "dev-cockpit-test-")); roots.push(root); return root; }
afterEach(() => { for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true }); vi.restoreAllMocks(); });
const serialize = (value: unknown) => JSON.stringify(value);
const installedText = (dir: string) => fs.readFileSync(path.join(dir, APP_NAME, "Contents", "MacOS", "dev-cockpit"), "utf8");

describe("manifest v1 Ed25519", () => {
  it("accepts a valid signature, reordered fields, UTF-8 and escaped newlines", () => {
    const f = fixture(temp());
    expect(verifyManifest(f.manifestText, f.keys, "stable").payload.version).toBe("2.0.0");
    const reordered = Object.fromEntries(Object.entries(f.manifest).reverse());
    expect(canonicalPayload(reordered)).toEqual(canonicalPayload(f.manifest));
    expect(verifyManifest(JSON.stringify(reordered, null, 2), f.keys)).toBeTruthy();
  });
  it.each(["version", "channel", "sha256", "bundleSha256", "releaseNotes", "downloadUrl", "publishedAt"])("rejects payload tampering: %s", (field) => {
    const f = fixture(temp()); f.manifest.payload[field] += "x";
    expect(() => verifyManifest(serialize(f.manifest), f.keys)).toThrow();
  });
  it("rejects wrong key and wrong signature", () => {
    const f = fixture(temp());
    expect(() => verifyManifest(f.manifestText, sign(f.manifest.payload).keys)).toThrow("signature_invalid");
    f.manifest.signature = Buffer.alloc(64).toString("base64");
    expect(() => verifyManifest(serialize(f.manifest), f.keys)).toThrow("signature_invalid");
  });
  it.each(["unknown key", "unsigned", "unsupported algorithm", "schema", "payload version", "extra field", "bad base64", "lone surrogate"])("rejects %s", (kind) => {
    const f = fixture(temp());
    if (kind === "unknown key") f.manifest.keyId = "unknown";
    if (kind === "unsigned") delete f.manifest.signature;
    if (kind === "unsupported algorithm") f.manifest.algorithm = "none";
    if (kind === "schema") f.manifest.schemaVersion = 2;
    if (kind === "payload version") f.manifest.payload.payloadVersion = 2;
    if (kind === "extra field") f.manifest.payload.extra = true;
    if (kind === "bad base64") f.manifest.signature += "\n";
    if (kind === "lone surrogate") f.manifest.payload.releaseNotes = "\ud800";
    expect(() => verifyManifest(serialize(f.manifest), f.keys)).toThrow();
  });
  it("rejects malformed, legacy, duplicate members, channel mismatch and oversized input", () => {
    const f = fixture(temp());
    for (const input of ["{", "null", "[]", '{"version":"1.0.1"}', " ".repeat(65537)]) {
      expect(() => verifyManifest(input, f.keys)).toThrow();
    }
    expect(() => verifyManifest(f.manifestText.replace("{", '{"schemaVersion":1,'), f.keys)).toThrow();
    expect(() => parseStrictJson('{"a":{"x":1,"\\u0078":2}}')).toThrow();
    expect(() => verifyManifest(f.manifestText, f.keys, "beta")).toThrow("channel_mismatch");
  });
});

describe("transaction apply and recovery", () => {
  it("verification failure does not download, extract, create state or alter user files", async () => {
    const f = fixture(temp()); const before = bundleHash(path.join(f.installDir, APP_NAME));
    f.manifest.signature = "";
    const download = vi.fn(); const extract = vi.fn();
    await expect(install({ ...f, manifestText: serialize(f.manifest), download, extract })).rejects.toThrow();
    expect(download).not.toHaveBeenCalled(); expect(extract).not.toHaveBeenCalled();
    expect(fs.existsSync(layout(f.installDir).root)).toBe(false);
    expect(bundleHash(path.join(f.installDir, APP_NAME))).toBe(before);
    expect(fs.readFileSync(path.join(f.installDir, "user-data.txt"), "utf8")).toBe("keep user data");
  });
  it("repeated execution is idempotent and does not rerun download/extraction", async () => {
    const f = fixture(temp()); const download = vi.fn(f.download), extract = vi.fn(f.extract);
    await expect(install({ ...f, download, extract })).resolves.toBe("committed");
    const state = fs.readFileSync(layout(f.installDir).state, "utf8");
    await expect(install({ ...f, download, extract })).resolves.toBe("committed");
    expect(download).toHaveBeenCalledTimes(1); expect(extract).toHaveBeenCalledTimes(1);
    expect(fs.readFileSync(layout(f.installDir).state, "utf8")).toBe(state);
    expect(installedText(f.installDir)).toBe("new application");
    expect(fs.existsSync(path.join(f.installDir, "user-data.txt"))).toBe(true);
  });
  it.each(["extract", "afterBackup", "afterReplace", "applied"])("rolls back after failure at %s", async (point) => {
    const f = fixture(temp()); const old = bundleHash(path.join(f.installDir, APP_NAME));
    await expect(install({ ...f,
      extract: point === "extract" ? () => { throw new Error("secret-token"); } : f.extract,
      checkpoint: (at: string) => { if (at === point) throw new Error("secret-token"); },
    })).rejects.toThrow();
    expect(bundleHash(path.join(f.installDir, APP_NAME))).toBe(old);
    expect(JSON.parse(fs.readFileSync(layout(f.installDir).state, "utf8")).phase).toBe("rolledBack");
    expect(safeMessage(new Error("secret-token stack trace"))).not.toContain("secret-token");
  });
  it("rejects corrupt archive and tampered staging without replacing the old app", async () => {
    for (const point of ["archive", "stage"]) {
      const f = fixture(temp());
      await expect(install({ ...f,
        download: point === "archive" ? (_url: string, p: string) => fs.writeFileSync(p, "tampered") : f.download,
        checkpoint: (at: string) => {
          if (point === "stage" && at === "prepared") {
            const s = JSON.parse(fs.readFileSync(layout(f.installDir).state, "utf8"));
            fs.writeFileSync(path.join(layout(f.installDir, s.id).stage, "injected"), "tamper");
          }
        },
      })).rejects.toThrow();
      expect(installedText(f.installDir)).toBe("old application");
    }
  });
  it.each(["preparing", "prepared", "applying", "afterBackup", "afterReplace", "applied", "committed"])("SIGKILL at %s recovers across a new process (relaunch/reboot model)", (phase) => {
    const root = temp();
    const child = spawnSync(process.execPath, [path.join(__dirname, "test-harness.cjs"), "crash", root, phase]);
    expect(child.signal).toBe("SIGKILL");
    const installed = path.join(root, "Applications");
    const expected = ["afterReplace", "applied", "committed"].includes(phase) ? "committed" : "rolledBack";
    for (let attempt = 0; attempt < 2; attempt++) {
      const next = spawnSync(process.execPath, [path.join(__dirname, "test-harness.cjs"), "recover", root], { encoding: "utf8" });
      expect(next.status).toBe(0); expect(next.stdout).toBe(expected);
    }
    expect(installedText(installed)).toBe(expected === "committed" ? "new application" : "old application");
    expect(fs.readFileSync(path.join(installed, "user-data.txt"), "utf8")).toBe("keep user data");
  });
  it.each(["corrupt", "missing", "symlink", "path traversal", "unknown phase", "manifest tamper"])("fails safe for %s journal", async (kind) => {
    const f = fixture(temp()); await install(f);
    const p = layout(f.installDir); const s = JSON.parse(fs.readFileSync(p.state, "utf8"));
    const previous = layout(f.installDir, s.id).previous;
    if (kind === "corrupt") fs.writeFileSync(p.state, "{");
    if (kind === "missing") fs.renameSync(p.state, p.state + ".preserved");
    if (kind === "symlink") { fs.renameSync(p.state, p.state + ".preserved"); fs.symlinkSync(p.state + ".preserved", p.state); }
    if (kind === "path traversal") { s.id = "../user-data"; fs.writeFileSync(p.state, serialize(s)); }
    if (kind === "unknown phase") { s.phase = "anything"; fs.writeFileSync(p.state, serialize(s)); }
    if (kind === "manifest tamper") { s.manifest.payload.releaseNotes += "x"; fs.writeFileSync(p.state, serialize(s)); }
    await expect(withLock(f.installDir, () => recover(f.installDir, f.keys))).rejects.toThrow();
    expect(installedText(f.installDir)).toBe("new application");
    expect(fs.existsSync(previous)).toBe(true);
  });
  it("blocks another process while the OS lock is held", async () => {
    const f = fixture(temp());
    await withLock(f.installDir, () => {
      const child = spawnSync(process.execPath, [path.join(__dirname, "test-harness.cjs"), "lock", f.installDir]);
      expect(child.status).toBe(1);
    });
    await expect(install(f)).resolves.toBe("committed");
  });
  it("preserves unknown user files in app position during rollback", async () => {
    const f = fixture(temp());
    await expect(install({ ...f, checkpoint: (at: string) => {
      if (at === "afterBackup") { fs.mkdirSync(path.join(f.installDir, APP_NAME)); fs.writeFileSync(path.join(f.installDir, APP_NAME, "user.txt"), "keep"); throw new Error("fail"); }
    } })).rejects.toThrow();
    expect(fs.readFileSync(path.join(f.installDir, APP_NAME, "user.txt"), "utf8")).toBe("keep");
    const s = JSON.parse(fs.readFileSync(layout(f.installDir).state, "utf8"));
    expect(fs.existsSync(layout(f.installDir, s.id).previous)).toBe(true);
  });
  it("rejects a signed downgrade and same-version replacement", async () => {
    const f = fixture(temp()); await install(f);
    for (const version of ["1.0.0", "2.0.0"]) {
      const payload = { ...f.manifest.payload, version, sha256: "a".repeat(64),
        downloadUrl: `https://github.com/minsdevs/dev-cockpit/releases/download/v${version}/Dev-Cockpit-${version}-macos-aarch64.zip` };
      // Keep original key trusted for the installed receipt as well.
      const next = sign(payload, "next"); const old = JSON.parse(fs.readFileSync(layout(f.installDir).state, "utf8"));
      const candidate = { ...f, manifestText: next.manifestText, keys: { ...f.keys, ...next.keys } };
      await expect(install(candidate)).rejects.toThrow(version === "1.0.0" ? "downgrade_rejected" : "version_conflict");
      expect(JSON.parse(fs.readFileSync(layout(f.installDir).state, "utf8")).manifest.signature).toBe(old.manifest.signature);
    }
  });
  it("real ZIP extraction, bundle validation and apply work together without launching", async () => {
    const f = fixture(temp()); fs.unlinkSync(f.zip);
    const packed = spawnSync("/usr/bin/ditto", ["-c", "-k", "--norsrc", "--keepParent", f.source, f.zip]);
    expect(packed.status).toBe(0);
    const signed = sign({ ...f.manifest.payload, sha256: hashFile(f.zip) });
    await expect(install({ ...f, ...signed, extract, currentVersion })).resolves.toBe("committed");
    expect(installedText(f.installDir)).toBe("new application");
  });
  it("fresh install and failed first install preserve unrelated data", async () => {
    for (const fail of [false, true]) {
      const f = fixture(temp());
      fs.rmSync(path.join(f.installDir, APP_NAME), { recursive: true });
      const task = install({ ...f, checkpoint: (at: string) => { if (fail && at === "afterReplace") throw new Error("failure"); } });
      if (fail) { await expect(task).rejects.toThrow(); expect(fs.existsSync(path.join(f.installDir, APP_NAME))).toBe(false); }
      else { await expect(task).resolves.toBe("committed"); expect(installedText(f.installDir)).toBe("new application"); }
      expect(fs.readFileSync(path.join(f.installDir, "user-data.txt"), "utf8")).toBe("keep user data");
    }
  });
  it.each(["traversal", "symlink"])("rejects %s ZIP before extraction", (kind) => {
    const f = fixture(temp()); fs.unlinkSync(f.zip);
    if (kind === "symlink") fs.symlinkSync("Info.plist", path.join(f.source, "Contents", "link"));
    expect(spawnSync("/usr/bin/ditto", ["-c", "-k", "--norsrc", "--keepParent", f.source, f.zip]).status).toBe(0);
    if (kind === "traversal") {
      const bytes = fs.readFileSync(f.zip), from = Buffer.from("dev-cockpit"), to = Buffer.from("../escape-x");
      for (let i = bytes.indexOf(from); i !== -1; i = bytes.indexOf(from, i + from.length)) to.copy(bytes, i);
      fs.writeFileSync(f.zip, bytes);
    }
    expect(() => validateArchive(f.zip)).toThrow();
  });
  it("release generator signs only with an explicitly trusted ephemeral key", () => {
    const f = fixture(temp()); fs.unlinkSync(f.zip);
    expect(spawnSync("/usr/bin/ditto", ["-c", "-k", "--norsrc", "--keepParent", f.source, f.zip]).status).toBe(0);
    const { privateKey, publicKey } = generateKeyPairSync("ed25519");
    const keys = { test: publicKey.export({ type: "spki", format: "der" }).subarray(-32).toString("base64") };
    const options = { app: f.source, archive: f.zip, version: "2.0.0", channel: "stable", keyId: "test", privateKey, keys, publishedAt: "2026-09-23T00:00:00Z" };
    const manifest = createManifest(options);
    expect(verifyManifest(serialize(manifest), keys).payload.bundleSha256).toBe(bundleHash(f.source));
    expect(() => createManifest({ ...options, keys: {} })).toThrow("key_unknown");
    expect(() => createManifest({ ...options, privateKey: generateKeyPairSync("ed25519").privateKey })).toThrow("signature_invalid");
  });
  it("rejects insecure download URLs without issuing a request", async () => {
    const fetch = vi.spyOn(globalThis, "fetch");
    for (const url of ["http://github.com/app", "https://attacker.example/app", "https://token:password@github.com/app"]) {
      await expect(download(url, null, 100)).rejects.toThrow("network_failed");
    }
    expect(fetch).not.toHaveBeenCalled();
  });
});

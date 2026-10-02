//! Ed25519 manifest v1. Legacy flat/unsigned manifests are intentionally rejected.
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::io::AsyncReadExt;

const MANIFEST_URL_STABLE: &str =
    "https://github.com/minsdevs/dev-cockpit/releases/latest/download/update-manifest.json";
const MANIFEST_URL_BETA: &str =
    "https://github.com/minsdevs/dev-cockpit/releases/download/beta/update-manifest.json";
pub const MAX_MANIFEST_BYTES: usize = 65536;
pub type TrustedKeys = BTreeMap<String, String>;

pub fn trusted_keys() -> Result<TrustedKeys, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Store {
        keys: TrustedKeys,
    }
    serde_json::from_str::<Store>(include_str!("../../installer/trusted-keys.json"))
        .map(|store| store.keys)
        .map_err(|_| "key_unknown".into())
}

/// Information about an available update.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub channel: String,
    pub release_notes: String,
    pub download_url: String,
    pub sha256: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestPayload {
    pub payload_version: u32,
    pub version: String,
    pub channel: String,
    pub platform: String,
    pub sha256: String,
    pub bundle_sha256: String,
    pub download_url: String,
    pub release_notes: String,
    pub published_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateManifest {
    pub schema_version: u32,
    pub algorithm: String,
    pub key_id: String,
    pub payload: ManifestPayload,
    pub signature: String,
}

pub fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn version_parts(value: &str) -> Result<[u32; 3], String> {
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 3 {
        return Err("manifest_invalid".into());
    }
    let mut result = [0; 3];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty()
            || part.len() > 9
            || !part.bytes().all(|b| b.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return Err("manifest_invalid".into());
        }
        result[i] = part.parse().map_err(|_| "manifest_invalid")?;
    }
    Ok(result)
}

impl UpdateManifest {
    /// Domain + compact ordered JSON array, UTF-8, no BOM/trailing newline.
    /// This is a fixed schema, not general-purpose JSON canonicalization.
    pub fn canonical_payload(&self) -> Result<Vec<u8>, String> {
        let p = &self.payload;
        if self.algorithm != "Ed25519" {
            return Err("algorithm_unsupported".into());
        }
        let timestamp = p.published_at.as_bytes();
        let valid_time = timestamp.len() == 20
            && timestamp.iter().enumerate().all(|(i, b)| match i {
                4 | 7 => *b == b'-',
                10 => *b == b'T',
                13 | 16 => *b == b':',
                19 => *b == b'Z',
                _ => b.is_ascii_digit(),
            });
        if self.schema_version != 1
            || p.payload_version != 1
            || self.key_id.is_empty()
            || self.key_id.len() > 64
            || !self
                .key_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || !matches!(p.channel.as_str(), "stable" | "beta")
            || p.platform != "darwin-aarch64"
            || !valid_hash(&p.sha256)
            || !valid_hash(&p.bundle_sha256)
            || p.release_notes.len() > 16384
            || !valid_time
        {
            return Err("manifest_invalid".into());
        }
        version_parts(&p.version)?;
        let expected = format!("https://github.com/minsdevs/dev-cockpit/releases/download/v{0}/Dev-Cockpit-{0}-macos-aarch64.zip", p.version);
        if p.download_url != expected {
            return Err("manifest_invalid".into());
        }
        let array = serde_json::json!([
            self.schema_version,
            self.algorithm,
            self.key_id,
            p.payload_version,
            p.version,
            p.channel,
            p.platform,
            p.sha256,
            p.bundle_sha256,
            p.download_url,
            p.release_notes,
            p.published_at
        ]);
        Ok(format!("dev-cockpit:update-manifest:v1\n{}", array).into_bytes())
    }
}

pub fn verify_manifest(
    body: &str,
    keys: &TrustedKeys,
    channel: Option<&str>,
) -> Result<UpdateManifest, String> {
    if body.len() > MAX_MANIFEST_BYTES {
        return Err("manifest_invalid".into());
    }
    // Typed deserialization rejects duplicate/unknown fields and missing signature.
    let manifest: UpdateManifest = serde_json::from_str(body).map_err(|_| "manifest_invalid")?;
    let bytes = manifest.canonical_payload()?;
    let encoded = keys.get(&manifest.key_id).ok_or("key_unknown")?;
    let raw = STANDARD.decode(encoded).map_err(|_| "key_unknown")?;
    if STANDARD.encode(&raw) != *encoded {
        return Err("key_unknown".into());
    }
    let raw: [u8; 32] = raw.try_into().map_err(|_| "key_unknown")?;
    let key = VerifyingKey::from_bytes(&raw).map_err(|_| "key_unknown")?;
    let raw_sig = STANDARD
        .decode(&manifest.signature)
        .map_err(|_| "signature_invalid")?;
    if STANDARD.encode(&raw_sig) != manifest.signature {
        return Err("signature_invalid".into());
    }
    let signature = Signature::from_slice(&raw_sig).map_err(|_| "signature_invalid")?;
    key.verify_strict(&bytes, &signature)
        .map_err(|_| "signature_invalid")?;
    if channel.is_some_and(|ch| ch != manifest.payload.channel) {
        return Err("channel_mismatch".into());
    }
    Ok(manifest)
}

pub async fn check_update(channel: &str) -> Result<Option<UpdateInfo>, String> {
    let url = match channel {
        "stable" => MANIFEST_URL_STABLE,
        "beta" => MANIFEST_URL_BETA,
        _ => return Err("channel_mismatch".into()),
    };
    let keys = trusted_keys()?;
    if keys.is_empty() {
        return Err("key_unknown".into());
    }
    let body = fetch_manifest(url).await?;
    let manifest = verify_manifest(&body, &keys, Some(channel))?;
    let p = manifest.payload;
    if version_parts(&p.version)? <= version_parts(env!("CARGO_PKG_VERSION"))? {
        return Ok(None);
    }
    Ok(Some(UpdateInfo {
        version: p.version,
        channel: p.channel,
        release_notes: p.release_notes,
        download_url: p.download_url,
        sha256: p.sha256,
        published_at: Some(p.published_at),
    }))
}

async fn fetch_manifest(url: &str) -> Result<String, String> {
    // Bound even a chunked response; never surface curl stderr/URLs to UI/logs.
    let mut child = tokio::process::Command::new("/usr/bin/curl")
        .args([
            "--disable",
            "-sSfL",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "15",
            "--max-filesize",
            "65536",
            url,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "network_failed")?;
    let stdout = child.stdout.take().ok_or("network_failed")?;
    let mut bytes = Vec::new();
    stdout
        .take((MAX_MANIFEST_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "network_failed")?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err("manifest_invalid".into());
    }
    if !child.wait().await.map_err(|_| "network_failed")?.success() {
        return Err("network_failed".into());
    }
    String::from_utf8(bytes).map_err(|_| "manifest_invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand_core::{OsRng, RngCore};

    pub(super) fn signed() -> (UpdateManifest, TrustedKeys) {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        let key = SigningKey::from_bytes(&seed);
        seed.fill(0);
        let mut m = UpdateManifest { schema_version: 1, algorithm: "Ed25519".into(),
            key_id: "ephemeral".into(), signature: String::new(), payload: ManifestPayload {
                payload_version: 1, version: "2.0.0".into(), channel: "stable".into(),
                platform: "darwin-aarch64".into(), sha256: "a".repeat(64), bundle_sha256: "b".repeat(64),
                download_url: "https://github.com/minsdevs/dev-cockpit/releases/download/v2.0.0/Dev-Cockpit-2.0.0-macos-aarch64.zip".into(),
                release_notes: "한글 \"safe\"\n🛠".into(), published_at: "2026-09-23T00:00:00Z".into(),
            }};
        m.signature = STANDARD.encode(key.sign(&m.canonical_payload().unwrap()).to_bytes());
        (
            m,
            BTreeMap::from([(
                "ephemeral".into(),
                STANDARD.encode(key.verifying_key().as_bytes()),
            )]),
        )
    }
    fn verify(m: &UpdateManifest, keys: &TrustedKeys) -> Result<UpdateManifest, String> {
        verify_manifest(&serde_json::to_string(m).unwrap(), keys, Some("stable"))
    }
    #[test]
    fn valid_manifest_verification_and_field_order() {
        let (m, keys) = signed();
        assert!(verify(&m, &keys).is_ok());
        let reordered = serde_json::to_string_pretty(&serde_json::to_value(&m).unwrap()).unwrap();
        assert!(verify_manifest(&reordered, &keys, None).is_ok());
    }
    #[test]
    fn tampering_wrong_signature_wrong_key_and_unknown_key() {
        let (m, keys) = signed();
        let mut tampered = m.clone();
        tampered.payload.release_notes.push('!');
        assert_eq!(verify(&tampered, &keys).unwrap_err(), "signature_invalid");
        tampered = m.clone();
        tampered.signature = STANDARD.encode([0u8; 64]);
        assert!(verify(&tampered, &keys).is_err());
        let (_, wrong) = signed();
        assert!(verify(&m, &wrong).is_err());
        assert_eq!(verify(&m, &BTreeMap::new()).unwrap_err(), "key_unknown");
    }
    #[test]
    fn malformed_unsigned_algorithm_schema_and_channel_rejected() {
        let (m, keys) = signed();
        for input in ["{}", "null", "{", "{\"version\":\"1.0.1\"}"] {
            assert!(verify_manifest(input, &keys, None).is_err());
        }
        let mut value = serde_json::to_value(&m).unwrap();
        value.as_object_mut().unwrap().remove("signature");
        assert!(verify_manifest(&value.to_string(), &keys, None).is_err());
        let mut bad = m.clone();
        bad.algorithm = "none".into();
        assert_eq!(verify(&bad, &keys).unwrap_err(), "algorithm_unsupported");
        bad = m.clone();
        bad.schema_version = 2;
        assert!(verify(&bad, &keys).is_err());
        bad = m.clone();
        bad.payload.payload_version = 2;
        assert!(verify(&bad, &keys).is_err());
        assert!(verify_manifest(&serde_json::to_string(&m).unwrap(), &keys, Some("beta")).is_err());
        let duplicate =
            serde_json::to_string(&m)
                .unwrap()
                .replacen("{", "{\"schemaVersion\":1,", 1);
        assert!(verify_manifest(&duplicate, &keys, None).is_err());
    }
    #[test]
    fn strict_versions() {
        assert!(version_parts("1.10.0").unwrap() > version_parts("1.9.9").unwrap());
        for v in ["v1.0.1", "1.0", "1.0.1-beta", "01.0.0", "1.0.x"] {
            assert!(version_parts(v).is_err());
        }
    }
}

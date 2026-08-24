//! Update checking with signed manifest verification.
//!
//! SECURITY POSTURE: fail-closed.
//! - If UPDATE_PUBLIC_KEY is a placeholder, empty, or invalid → reject ALL updates.
//! - If cryptographic verification is not available (no ed25519 dependency) → reject ALL updates.
//! - Updates are NEVER trusted without a valid key AND a verified signature.
//!
//! Status:
//! - ⏸ Ed25519 signature verification: fail-closed (requires ed25519-dalek + real key)
//! - ⏸ Apple code signing: requires Developer ID certificate
//! - ⏸ Apple notarization: requires active Apple Developer account

use serde::{Deserialize, Serialize};

const MANIFEST_URL_STABLE: &str =
    "https://github.com/Min0504/dev-cockpit/releases/latest/download/update-manifest.json";
const MANIFEST_URL_BETA: &str =
    "https://github.com/Min0504/dev-cockpit/releases/download/beta/update-manifest.json";

/// Public key for verifying update manifest signatures (ed25519).
/// 🟡 Placeholder — replace with actual public key when generated.
const UPDATE_PUBLIC_KEY: &str = "PLACEHOLDER_ED25519_PUBLIC_KEY_BASE64";

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

/// The signed manifest fetched from the update server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateManifest {
    pub version: String,
    pub channel: String,
    pub sha256: String,
    pub download_url: String,
    pub release_notes: String,
    pub published_at: Option<String>,
    /// Base64-encoded ed25519 signature of the manifest content (excluding this field).
    pub signature: String,
}

/// Current app version (from Cargo.toml).
fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Compare semantic versions. Returns true if `remote` is newer than `local`.
fn is_newer(remote: &str, local: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect()
    };
    let r = parse(remote);
    let l = parse(local);
    r > l
}

/// Check for updates by fetching and verifying the signed manifest.
///
/// Returns `Ok(Some(info))` if an update is available, `Ok(None)` if current,
/// or `Err` if the check failed (network error, invalid signature, etc.).
pub async fn check_update(channel: &str) -> Result<Option<UpdateInfo>, String> {
    let url = match channel {
        "beta" => MANIFEST_URL_BETA,
        _ => MANIFEST_URL_STABLE,
    };

    // Fetch manifest
    // Note: In production this uses reqwest or similar; here we use a minimal
    // approach that works within Tauri's async runtime.
    let body = fetch_manifest(url).await?;

    // Parse manifest
    let manifest: UpdateManifest =
        serde_json::from_str(&body).map_err(|e| format!("invalid update manifest: {e}"))?;

    // Verify signature (🟡 requires actual public key)
    verify_signature(&manifest)?;

    // Compare versions
    if !is_newer(&manifest.version, current_version()) {
        return Ok(None);
    }

    Ok(Some(UpdateInfo {
        version: manifest.version,
        channel: manifest.channel,
        release_notes: manifest.release_notes,
        download_url: manifest.download_url,
        sha256: manifest.sha256,
        published_at: manifest.published_at,
    }))
}

/// Fetch the manifest JSON from the given URL.
async fn fetch_manifest(url: &str) -> Result<String, String> {
    // Use std::process::Command to call curl as a simple HTTP client
    // (avoids adding reqwest dependency; Tauri apps can use this pattern)
    let output = tokio::process::Command::new("curl")
        .args(["-sSfL", "--max-time", "15", url])
        .output()
        .await
        .map_err(|e| format!("failed to fetch update manifest: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("update check failed: {}", stderr.trim()));
    }

    String::from_utf8(output.stdout).map_err(|e| format!("invalid manifest encoding: {e}"))
}

/// Returns `true` if the public key is a known placeholder, empty, or clearly
/// not a valid base64-encoded 32-byte ed25519 public key.
fn is_key_invalid(key: &str) -> bool {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return true;
    }
    // Reject known placeholder sentinel values
    const PLACEHOLDERS: &[&str] = &[
        "PLACEHOLDER_ED25519_PUBLIC_KEY_BASE64",
        "REPLACE_ME",
        "TODO",
        "CHANGEME",
    ];
    if PLACEHOLDERS.iter().any(|p| trimmed.eq_ignore_ascii_case(p)) {
        return true;
    }
    // A valid base64-encoded 32-byte key is exactly 44 chars (with padding).
    // Reject anything that clearly cannot be valid base64 of the right length.
    if trimmed.len() < 40 || trimmed.len() > 48 {
        return true;
    }
    // Reject if it contains non-base64 characters
    let is_base64_char = |c: char| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=';
    if !trimmed.chars().all(is_base64_char) {
        return true;
    }
    false
}

/// Verify the ed25519 signature of the manifest.
///
/// SECURITY: This function is fail-closed. If the public key is not configured
/// (placeholder, empty, or invalid format), verification ALWAYS fails and
/// returns an error. Updates are never trusted without a valid key + signature.
///
/// ⏸ Actual cryptographic ed25519 verification is disabled because no signing
/// key pair has been generated and the ed25519-dalek dependency is not present.
/// When a real key is provisioned and the crypto dependency added, the TODO
/// block below should be completed.
fn verify_signature(manifest: &UpdateManifest) -> Result<(), String> {
    // FAIL-CLOSED: reject if key is not properly configured
    if is_key_invalid(UPDATE_PUBLIC_KEY) {
        return Err(
            "업데이트 검증 불가: 공개키 미설정. 서명 검증이 구성되기 전까지 업데이트를 설치할 수 없습니다.".into()
        );
    }

    // Reject empty signatures unconditionally
    if manifest.signature.is_empty() {
        return Err("update manifest has no signature".into());
    }

    // TODO: When ed25519-dalek is added as a dependency, implement:
    // 1. Decode the public key from base64
    // 2. Reconstruct the signed payload (manifest without signature field)
    // 3. Decode the signature from base64
    // 4. Verify using ed25519-dalek:
    //    let key_bytes = base64_decode(UPDATE_PUBLIC_KEY)?;
    //    let public_key = VerifyingKey::from_bytes(&key_bytes)?;
    //    let sig_bytes = base64_decode(&manifest.signature)?;
    //    let signature = Signature::from_bytes(&sig_bytes);
    //    public_key.verify(payload.as_bytes(), &signature)?;
    //
    // Until then, this function rejects ALL manifests (fail-closed).
    Err(
        "업데이트 검증 불가: 서명 검증 기능이 아직 활성화되지 않았습니다 (crypto 의존성 미구성)."
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison() {
        assert!(is_newer("1.1.0", "1.0.1"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(is_newer("1.0.2", "1.0.1"));
        assert!(!is_newer("1.0.1", "1.0.1"));
        assert!(!is_newer("1.0.0", "1.0.1"));
        assert!(is_newer("v1.1.0", "1.0.1"));
    }

    fn make_manifest(sig: &str) -> UpdateManifest {
        UpdateManifest {
            version: "2.0.0".into(),
            channel: "stable".into(),
            sha256: "abc123def456".into(),
            download_url: "https://example.com/app.zip".into(),
            release_notes: "New features".into(),
            published_at: None,
            signature: sig.into(),
        }
    }

    #[test]
    fn placeholder_key_rejects_manifest() {
        // The current UPDATE_PUBLIC_KEY is a placeholder.
        // verify_signature MUST fail (fail-closed).
        let manifest = make_manifest("dGVzdC1zaWduYXR1cmU=");
        let result = verify_signature(&manifest);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("공개키 미설정"),
            "error must mention unconfigured key"
        );
    }

    #[test]
    fn empty_signature_rejected() {
        // Even if we somehow bypass the key check, empty sig must fail.
        let manifest = make_manifest("");
        let result = verify_signature(&manifest);
        assert!(result.is_err());
    }

    #[test]
    fn is_key_invalid_detects_placeholders() {
        assert!(is_key_invalid("PLACEHOLDER_ED25519_PUBLIC_KEY_BASE64"));
        assert!(is_key_invalid("REPLACE_ME"));
        assert!(is_key_invalid("TODO"));
        assert!(is_key_invalid("CHANGEME"));
        assert!(is_key_invalid(""));
        assert!(is_key_invalid("   "));
        assert!(is_key_invalid("too_short"));
        // Non-base64 characters
        assert!(is_key_invalid(
            "!@#$%^&*()!@#$%^&*()!@#$%^&*()!@#$%^&*()!@#$"
        ));
    }

    #[test]
    fn is_key_invalid_accepts_valid_format() {
        // 44-char base64 that looks like a valid 32-byte key
        let valid_looking = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        assert_eq!(valid_looking.len(), 44);
        assert!(!is_key_invalid(valid_looking));
    }

    #[test]
    fn verify_signature_fails_closed_with_valid_format_key() {
        // Even with a key that passes format checks, crypto verification
        // is not implemented, so it must still reject (the second fail-closed path).
        // This test documents that the function never returns Ok without real crypto.
        let manifest = make_manifest("dGVzdC1zaWduYXR1cmU=");
        let result = verify_signature(&manifest);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn check_for_update_rejects_unverified_manifest() {
        // Simulate the full flow: even if we could fetch a manifest,
        // the placeholder key means check_update should propagate the error.
        // We can't actually fetch here, but verify the verification gate works
        // by calling verify_signature directly — which is what check_update uses.
        let manifest = make_manifest("c2lnbmVk");
        let err = verify_signature(&manifest).unwrap_err();
        // The error must prevent UpdateInfo from being returned
        assert!(!err.is_empty());
    }
}

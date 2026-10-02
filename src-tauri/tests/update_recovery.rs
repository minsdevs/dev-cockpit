//! Node installer -> Rust app startup integration. Private keys never leave Node memory.
use base64::{engine::general_purpose::STANDARD, Engine};
use dev_cockpit_lib::{update_recovery, updater};
use serde_json::Value;
use std::os::unix::{
    fs::{symlink, PermissionsExt},
    process::ExitStatusExt,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn harness() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../installer/test-harness.cjs")
}
fn crash(root: &Path, phase: &str) -> updater::TrustedKeys {
    let out = Command::new("node")
        .arg(harness())
        .args(["crash", root.to_str().unwrap(), phase])
        .output()
        .unwrap();
    assert_eq!(
        out.status.signal(),
        Some(libc::SIGKILL),
        "installer must reach the crash point"
    );
    serde_json::from_slice(&fs::read(root.join("public.json")).unwrap()).unwrap()
}
fn journal(install: &Path) -> PathBuf {
    install
        .join(update_recovery::JOURNAL_DIR)
        .join("state.json")
}
fn app_text(install: &Path) -> String {
    fs::read_to_string(
        install
            .join(update_recovery::APP_NAME)
            .join("Contents/MacOS/dev-cockpit"),
    )
    .unwrap()
}

#[test]
fn node_and_rust_share_canonical_bytes_signature_and_bundle_hash() {
    let root = tempfile::tempdir().unwrap();
    let out = Command::new("node")
        .arg(harness())
        .arg("vector")
        .arg(root.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let vector: Value = serde_json::from_slice(&out.stdout).unwrap();
    let keys = serde_json::from_value(vector["keys"].clone()).unwrap();
    let verified =
        updater::verify_manifest(&vector["manifest"].to_string(), &keys, Some("stable")).unwrap();
    assert_eq!(
        STANDARD.encode(verified.canonical_payload().unwrap()),
        vector["canonical"]
    );
    assert_eq!(
        update_recovery::bundle_hash(&root.path().join("source").join(update_recovery::APP_NAME))
            .unwrap(),
        vector["bundleHash"]
    );
    // Both hash routines cover bytes, permissions and contained symlink targets.
    let app = root.path().join("source").join(update_recovery::APP_NAME);
    symlink("Info.plist", app.join("Contents/link")).unwrap();
    let original = update_recovery::bundle_hash(&app).unwrap();
    fs::set_permissions(
        app.join("Contents/Info.plist"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert_ne!(original, update_recovery::bundle_hash(&app).unwrap());
    symlink(
        root.path().join("public-secret-does-not-exist"),
        app.join("outside"),
    )
    .unwrap();
    assert!(update_recovery::bundle_hash(&app).is_err());
}

#[test]
fn interrupted_installation_app_relaunch_and_reboot_recovery_are_idempotent() {
    for phase in [
        "preparing",
        "prepared",
        "applying",
        "afterBackup",
        "afterReplace",
        "applied",
        "committed",
    ] {
        let root = tempfile::tempdir().unwrap();
        let keys = crash(root.path(), phase);
        let install = root.path().join("Applications");
        let expected = if ["afterReplace", "applied", "committed"].contains(&phase) {
            "committed"
        } else {
            "rolledBack"
        };
        for _ in 0..2 {
            assert_eq!(
                update_recovery::recover_at(&install, &keys).unwrap(),
                expected,
                "{phase}"
            );
        }
        // Relaunch the actual Node entrypoint after Rust updated the shared journal.
        let out = Command::new("node")
            .arg(harness())
            .arg("recover")
            .arg(root.path())
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8(out.stdout).unwrap(), expected);
        assert_eq!(
            app_text(&install),
            if expected == "committed" {
                "new application"
            } else {
                "old application"
            }
        );
        assert_eq!(
            fs::read_to_string(install.join("user-data.txt")).unwrap(),
            "keep user data"
        );
    }
}

#[test]
fn failed_apply_and_interrupted_rollback_restore_previous_and_preserve_rejected() {
    for phase in ["failed", "rollingBack"] {
        let root = tempfile::tempdir().unwrap();
        let keys = crash(root.path(), "afterReplace");
        let install = root.path().join("Applications");
        let mut s: Value = serde_json::from_slice(&fs::read(journal(&install)).unwrap()).unwrap();
        s["phase"] = phase.into();
        fs::write(journal(&install), s.to_string()).unwrap();
        assert_eq!(
            update_recovery::recover_at(&install, &keys).unwrap(),
            "rolledBack"
        );
        assert_eq!(app_text(&install), "old application");
        assert!(install
            .join(update_recovery::JOURNAL_DIR)
            .join(s["id"].as_str().unwrap())
            .join("rejected.app")
            .exists());
        assert_eq!(
            update_recovery::recover_at(&install, &keys).unwrap(),
            "rolledBack"
        );
    }
}

#[test]
fn corrupt_missing_unsigned_or_malicious_journal_is_preserved_without_app_mutation() {
    for corruption in [
        "json",
        "missing",
        "id",
        "phase",
        "signature",
        "symlink",
        "missingField",
        "duplicate",
    ] {
        let root = tempfile::tempdir().unwrap();
        let keys = crash(root.path(), "prepared");
        let install = root.path().join("Applications");
        let file = journal(&install);
        let before = fs::read_to_string(&file).unwrap();
        let mut s: Value = serde_json::from_str(&before).unwrap();
        match corruption {
            "json" => fs::write(&file, "{").unwrap(),
            "missing" => fs::rename(&file, file.with_extension("preserved")).unwrap(),
            "id" => {
                s["id"] = "../../user-data".into();
                fs::write(&file, s.to_string()).unwrap();
            }
            "phase" => {
                s["phase"] = "unknown".into();
                fs::write(&file, s.to_string()).unwrap();
            }
            "signature" => {
                s["manifest"]["signature"] = "".into();
                fs::write(&file, s.to_string()).unwrap();
            }
            "symlink" => {
                fs::rename(&file, file.with_extension("preserved")).unwrap();
                symlink(file.with_extension("preserved"), &file).unwrap();
            }
            "missingField" => {
                s.as_object_mut().unwrap().remove("previousHash");
                fs::write(&file, s.to_string()).unwrap();
            }
            "duplicate" => {
                fs::write(&file, before.replacen('{', "{\"schemaVersion\":1,", 1)).unwrap()
            }
            _ => unreachable!(),
        }
        assert!(
            update_recovery::recover_at(&install, &keys).is_err(),
            "{corruption}"
        );
        assert_eq!(app_text(&install), "old application");
        assert!(install.join("user-data.txt").exists());
    }
}

#[test]
fn source_run_does_not_guess_install_path_and_absent_state_is_safe() {
    assert!(
        update_recovery::installed_directory(Path::new("/repo/target/debug/dev-cockpit")).is_none()
    );
    assert_eq!(
        update_recovery::installed_directory(Path::new(
            "/Applications/Dev Cockpit.app/Contents/MacOS/dev-cockpit"
        ))
        .unwrap(),
        PathBuf::from("/Applications")
    );
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        update_recovery::recover_at(root.path(), &updater::TrustedKeys::new()).unwrap(),
        "idle"
    );
}

#[test]
fn app_recovery_cannot_race_installer_and_sigkill_releases_cross_runtime_lock() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let root = tempfile::tempdir().unwrap();
    let keys = crash(root.path(), "prepared");
    let install = root.path().join("Applications");
    let mut child = Command::new("node")
        .arg(harness())
        .arg("hold")
        .arg(&install)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line, "locked\n");
    assert_eq!(
        update_recovery::recover_at(&install, &keys).unwrap_err(),
        "update_busy"
    );
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(
        update_recovery::recover_at(&install, &keys).unwrap(),
        "rolledBack"
    );
}

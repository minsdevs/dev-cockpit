//! Installer journal v1 recovery. Kept interoperable with installer/transaction.js.
//! Only fixed, derived paths are used. Unknown files/states are never deleted.
use crate::updater::{self, TrustedKeys, UpdateManifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const APP_NAME: &str = "Dev Cockpit.app";
pub const JOURNAL_DIR: &str = ".dev-cockpit-update";
pub struct RecoveryStatus(pub Mutex<String>);

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
enum Phase {
    Preparing,
    Prepared,
    Applying,
    Applied,
    Committed,
    Failed,
    RollingBack,
    RolledBack,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    id: String,
    phase: Phase,
    manifest: UpdateManifest,
    // A local rollback integrity check, not a publisher signature.
    #[serde(deserialize_with = "required_previous_hash")]
    previous_hash: Option<String>,
}

fn required_previous_hash<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}

fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("state_invalid".into()),
    }
}
fn directory(path: &Path) -> Result<(), String> {
    if !fs::symlink_metadata(path)
        .map_err(|_| "state_invalid")?
        .is_dir()
    {
        return Err("state_invalid".into());
    }
    Ok(())
}
fn sync_dir(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|_| "state_invalid".into())
}
fn move_absent(from: &Path, to: &Path) -> Result<(), String> {
    if exists(to)? {
        return Err("state_invalid".into());
    }
    fs::rename(from, to).map_err(|_| "state_invalid")?;
    sync_dir(from.parent().ok_or("state_invalid")?)?;
    sync_dir(to.parent().ok_or("state_invalid")?)
}
fn write_journal(root: &Path, journal: &Journal) -> Result<(), String> {
    // O_EXCL prevents following/replacing an orphan temp or user-owned file.
    let mut counter = 0;
    let (temp, mut file) = loop {
        let temp = root.join(format!("state.{}.{}.tmp", std::process::id(), counter));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
        {
            Ok(file) => break (temp, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && counter < 1000 => {
                counter += 1
            }
            Err(_) => return Err("state_invalid".into()),
        }
    };
    let bytes = serde_json::to_vec(journal).map_err(|_| "state_invalid")?;
    file.write_all(&bytes)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|_| "state_invalid")?;
    fs::rename(temp, root.join("state.json")).map_err(|_| "state_invalid")?;
    sync_dir(root)
}
fn clean_name(value: &str) -> bool {
    !value.bytes().any(|b| b < 32 || b == 127)
}
fn hash_file(path: &Path) -> Result<String, String> {
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "bundle_invalid")?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).map_err(|_| "bundle_invalid")?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
pub fn bundle_hash(app: &Path) -> Result<String, String> {
    directory(app)?;
    let root = app.canonicalize().map_err(|_| "bundle_invalid")?;
    fn walk(
        root: &Path,
        relative: &str,
        hash: &mut Sha256,
        entries: &mut u64,
        total: &mut u64,
    ) -> Result<(), String> {
        *entries += 1;
        if *entries > 100000 || !clean_name(relative) {
            return Err("bundle_invalid".into());
        }
        let path = if relative == "." {
            root.to_path_buf()
        } else {
            root.join(relative)
        };
        let meta = fs::symlink_metadata(&path).map_err(|_| "bundle_invalid")?;
        let (kind, content) = if meta.file_type().is_symlink() {
            let target = fs::read_link(&path).map_err(|_| "bundle_invalid")?;
            let text = target.to_str().ok_or("bundle_invalid")?;
            let resolved = path.canonicalize().map_err(|_| "bundle_invalid")?;
            if target.is_absolute()
                || !resolved.starts_with(root)
                || resolved == root
                || !clean_name(text)
            {
                return Err("bundle_invalid".into());
            }
            ("l", text.to_string())
        } else if meta.is_dir() {
            ("d", String::new())
        } else if meta.is_file() && meta.nlink() == 1 {
            *total += meta.len();
            if *total > 2 * 1024 * 1024 * 1024 {
                return Err("bundle_invalid".into());
            }
            ("f", hash_file(&path)?)
        } else {
            return Err("bundle_invalid".into());
        };
        hash.update(
            format!(
                "{}\0{}\0{}\0{}\n",
                kind,
                relative,
                meta.permissions().mode() & 0o777,
                content
            )
            .as_bytes(),
        );
        if kind == "d" {
            let mut names = fs::read_dir(path)
                .map_err(|_| "bundle_invalid")?
                .map(|e| {
                    e.map_err(|_| "bundle_invalid")
                        .and_then(|e| e.file_name().into_string().map_err(|_| "bundle_invalid"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            names.sort();
            for name in names {
                let child = if relative == "." {
                    name
                } else {
                    format!("{relative}/{name}")
                };
                walk(root, &child, hash, entries, total)?;
            }
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    walk(&root, ".", &mut hash, &mut 0, &mut 0)?;
    Ok(format!("{:x}", hash.finalize()))
}

/// Flock is automatically released on exit/SIGKILL/reboot. Never unlink it.
pub fn recover_at(install_dir: &Path, keys: &TrustedKeys) -> Result<String, String> {
    let root = install_dir.join(JOURNAL_DIR);
    if !exists(&root)? {
        return Ok("idle".into());
    }
    directory(install_dir)?;
    directory(&root)?;
    let meta = fs::metadata(&root).map_err(|_| "state_invalid")?;
    // SAFETY: geteuid takes no pointers and has no side effects.
    if meta.uid() != unsafe { libc::geteuid() } || meta.permissions().mode() & 0o077 != 0 {
        return Err("state_invalid".into());
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(root.join("lock"))
        .map_err(|_| "state_invalid")?;
    let lock_meta = lock.metadata().map_err(|_| "state_invalid")?;
    if !lock_meta.is_file() || lock_meta.nlink() != 1 {
        return Err("state_invalid".into());
    }
    // SAFETY: a live owned file descriptor; lock drops after all journal work.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("update_busy".into());
    }
    let state = root.join("state.json");
    if !exists(&state)? {
        let orphan = fs::read_dir(&root)
            .map_err(|_| "state_invalid")?
            .any(|e| e.map_or(true, |e| e.file_name() != "lock"));
        return if orphan {
            Err("state_invalid".into())
        } else {
            Ok("idle".into())
        };
    }
    let meta = fs::symlink_metadata(&state).map_err(|_| "state_invalid")?;
    if !meta.is_file() || meta.nlink() != 1 || meta.len() > 131072 {
        return Err("state_invalid".into());
    }
    let bytes = fs::read(&state).map_err(|_| "state_invalid")?;
    let mut s: Journal = serde_json::from_slice(&bytes).map_err(|_| "state_invalid")?;
    if s.schema_version != 1
        || s.id.len() != 32
        || !s
            .id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || s.previous_hash
            .as_deref()
            .is_some_and(|hash| !updater::valid_hash(hash))
    {
        return Err("state_invalid".into());
    }
    // The journal does not confer trust. Re-verify the embedded signed payload.
    updater::verify_manifest(
        &serde_json::to_string(&s.manifest).map_err(|_| "state_invalid")?,
        keys,
        None,
    )?;
    let tx = root.join(&s.id);
    directory(&tx)?;
    let app = install_dir.join(APP_NAME);
    let previous = tx.join("previous.app");
    let rejected = tx.join("rejected.app");
    let stage_dir = tx.join("stage");
    if exists(&stage_dir)? {
        directory(&stage_dir)?;
    }
    let stage = stage_dir.join(APP_NAME);
    if s.phase == Phase::RolledBack {
        let current = if exists(&app)? {
            Some(bundle_hash(&app)?)
        } else {
            None
        };
        return if current == s.previous_hash {
            Ok("rolledBack".into())
        } else {
            Err("state_invalid".into())
        };
    }
    if s.phase == Phase::Committed {
        return if exists(&app)? && bundle_hash(&app)? == s.manifest.payload.bundle_sha256 {
            Ok("committed".into())
        } else {
            Err("bundle_invalid".into())
        };
    }
    if matches!(s.phase, Phase::Applying | Phase::Applied)
        && !exists(&stage)?
        && exists(&app)?
        && bundle_hash(&app)? == s.manifest.payload.bundle_sha256
    {
        s.phase = Phase::Committed;
        write_journal(&root, &s)?;
        return Ok("committed".into());
    }
    s.phase = Phase::RollingBack;
    write_journal(&root, &s)?;
    let current = if exists(&app)? {
        Some(bundle_hash(&app)?)
    } else {
        None
    };
    if exists(&previous)? {
        if s.previous_hash.as_ref() != Some(&bundle_hash(&previous)?) {
            return Err("state_invalid".into());
        }
        if let Some(current) = current {
            if current != s.manifest.payload.bundle_sha256 || exists(&rejected)? {
                return Err("state_invalid".into());
            }
            move_absent(&app, &rejected)?;
        }
        move_absent(&previous, &app)?;
    } else if current != s.previous_hash {
        if s.previous_hash.is_none()
            && current.as_ref() == Some(&s.manifest.payload.bundle_sha256)
            && !exists(&rejected)?
        {
            move_absent(&app, &rejected)?;
        } else {
            return Err("state_invalid".into());
        }
    }
    s.phase = Phase::RolledBack;
    write_journal(&root, &s)?;
    Ok("rolledBack".into())
}

pub fn installed_directory(executable: &Path) -> Option<PathBuf> {
    executable
        .ancestors()
        .find(|p| p.file_name().is_some_and(|name| name == APP_NAME))
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}
pub fn recover_current() -> Result<String, String> {
    let executable = std::env::current_exe().map_err(|_| "state_invalid")?;
    match installed_directory(&executable) {
        Some(dir) => recover_at(&dir, &updater::trusted_keys()?),
        None => Ok("idle".into()), // source/debug runs must not touch installed apps
    }
}

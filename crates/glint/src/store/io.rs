//! Atomic file writes, serialized background persistence, portable paths and token protection.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

use serde::Serialize;

/// The folder name: "Glint" on Windows and macOS, "glint" on Linux.
pub(super) const APP_ID: &str = if cfg!(any(windows, target_os = "macos")) { "Glint" } else { "glint" };

/// A portable copy keeps everything in a `data` folder beside the exe: the zip ships a
/// `glint.portable` file next to it, the installer does not.
pub(super) fn portable_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join("glint.portable").is_file().then(|| dir.join("data"))
}

type WriteTask = Box<dyn FnOnce() + Send + 'static>;

/// Single serialized background writer for settings and snapshots, avoiding concurrent OS thread
/// bursts and guaranteeing that later snapshots never get overwritten by slower earlier ones.
fn writer() -> &'static mpsc::Sender<WriteTask> {
    static TX: OnceLock<mpsc::Sender<WriteTask>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<WriteTask>();
        let _ = std::thread::Builder::new().name("glint-store".into()).spawn(move || {
            for task in rx {
                task();
            }
        });
        tx
    })
}

pub(super) fn enqueue_json<T: Serialize + Send + 'static>(path: PathBuf, value: T) {
    let _ = writer().send(Box::new(move || {
        if let Some(dir) = path.parent()
            && !dir.is_dir()
        {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec(&value) {
            write_atomic(&path, &json);
        }
    }));
}

pub(super) fn enqueue_json_pretty<T: Serialize + Send + 'static>(path: PathBuf, value: T) {
    let _ = writer().send(Box::new(move || {
        if let Some(dir) = path.parent()
            && !dir.is_dir()
        {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(&value) {
            write_atomic(&path, &json);
        }
    }));
}

fn unique_tmp(path: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let stem = path.file_name().and_then(|n| n.to_str()).unwrap_or("glint");
    path.with_file_name(format!(".{stem}.{pid}.{seq}.tmp"))
}

/// `write_atomic` for the token: on Unix the file is made readable by its owner only, before a
/// byte of it is written.
#[cfg(unix)]
pub(super) fn write_private(path: &Path, bytes: &[u8]) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = unique_tmp(path);
    let written = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).and_then(|mut f| f.write_all(bytes));
    if written.is_err() || std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(not(unix))]
pub(super) fn write_private(path: &Path, bytes: &[u8]) {
    write_atomic(path, bytes);
}

pub(super) fn write_atomic(path: &Path, bytes: &[u8]) {
    let _ = write_atomic_result(path, bytes);
}

pub(super) fn write_atomic_result(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = unique_tmp(path);
    if let Err(e) = std::fs::write(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn protect(plain: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData};
    // SAFETY: `input` points to `plain` for the synchronous DPAPI call; on success, Windows
    // allocates `out.pbData` of `out.cbData` bytes which we copy out and release with `LocalFree`.
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: plain.len() as u32, pbData: plain.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out);
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        let sealed = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        Some(sealed)
    }
}

#[cfg(windows)]
pub(super) fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData};
    // SAFETY: `input` points to `sealed` for the synchronous DPAPI call; on success, Windows
    // allocates `out.pbData` of `out.cbData` bytes which we copy out and release with `LocalFree`.
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: sealed.len() as u32, pbData: sealed.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out);
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        let plain = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        Some(plain)
    }
}

// Elsewhere the file itself is the protection: `save_token` makes it readable by its owner only.
#[cfg(not(windows))]
pub(super) fn protect(plain: &[u8]) -> Option<Vec<u8>> {
    Some(plain.to_vec())
}

#[cfg(not(windows))]
pub(super) fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
    Some(sealed.to_vec())
}

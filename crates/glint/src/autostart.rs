//! Starting Glint when the user signs in to the computer, the way each system expects it: a value
//! under `HKCU\…\Run` on Windows, a desktop entry in `~/.config/autostart` on Linux, a launch
//! agent in `~/Library/LaunchAgents` on macOS. Each points at the program that runs now (the
//! AppImage, not its mount, when it is one) and passes [`BACKGROUND`], so a start at login can
//! wait in the tray instead of opening the window.
//!
//! The entry is written again at every launch while the setting is on, so it follows the program
//! when an update or a move puts it somewhere else.

use std::path::PathBuf;

/// The argument a start at login carries.
pub const BACKGROUND: &str = "--background";

/// Whether this run was started at login (and so may stay in the tray).
pub fn started_at_login() -> bool {
    std::env::args().any(|a| a == BACKGROUND)
}

/// The program to start: the AppImage itself when running from one (its mount point changes on
/// every run), otherwise this executable.
fn program() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(app_image) = std::env::var_os("APPIMAGE") {
        return Some(PathBuf::from(app_image));
    }
    std::env::current_exe().ok()
}

/// Turns the start at login on or off. Errors are only logged: the setting itself stays as the
/// user left it, and the next launch tries again.
pub fn apply(on: bool) {
    let result = if on { program().ok_or_else(|| "no program path".to_owned()).and_then(|p| imp::enable(&p)) } else { imp::disable() };
    if let Err(e) = result {
        eprintln!("[autostart] {e}");
    }
}

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW};

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "Glint";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn enable(program: &Path) -> Result<(), String> {
        let command = wide(&format!("\"{}\" {}", program.display(), super::BACKGROUND));
        let (key, name) = (wide(RUN), wide(NAME));
        // SAFETY: every pointer is to a live, NUL-terminated UTF-16 buffer, and the size is the
        // data's own length in bytes, terminator included.
        let rc = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, command.as_ptr().cast(), (command.len() * 2) as u32) };
        if rc == ERROR_SUCCESS { Ok(()) } else { Err(format!("RegSetKeyValueW failed ({rc})")) }
    }

    pub fn disable() -> Result<(), String> {
        let (key, name) = (wide(RUN), wide(NAME));
        // SAFETY: as above.
        let rc = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND { Ok(()) } else { Err(format!("RegDeleteKeyValueW failed ({rc})")) }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::{Path, PathBuf};

    fn entry() -> Option<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(config.join("autostart").join("glint.desktop"))
    }

    /// `path` quoted for an `Exec=` line of a desktop entry.
    fn quoted(path: &Path) -> String {
        let s = path.display().to_string();
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`").replace('$', "\\$");
        format!("\"{escaped}\"")
    }

    pub fn enable(program: &Path) -> Result<(), String> {
        let path = entry().ok_or("no home folder")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = format!(
            "[Desktop Entry]\nType=Application\nName=Glint\nComment=Yandex Music\nIcon=glint\nExec={} {}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            quoted(program),
            super::BACKGROUND
        );
        std::fs::write(&path, text).map_err(|e| e.to_string())
    }

    pub fn disable() -> Result<(), String> {
        match entry().map(std::fs::remove_file) {
            Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::path::{Path, PathBuf};

    const LABEL: &str = "io.github.orphytype.glint";

    fn agent() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
    }

    fn xml(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    }

    pub fn enable(program: &Path) -> Result<(), String> {
        let path = agent().ok_or("no home folder")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
            xml(&program.display().to_string()),
            super::BACKGROUND
        );
        std::fs::write(&path, text).map_err(|e| e.to_string())
    }

    pub fn disable() -> Result<(), String> {
        match agent().map(std::fs::remove_file) {
            Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod imp {
    pub fn enable(_: &std::path::Path) -> Result<(), String> {
        Err("not supported on this system".into())
    }
    pub fn disable() -> Result<(), String> {
        Ok(())
    }
}

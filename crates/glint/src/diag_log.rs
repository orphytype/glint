//! Local anonymized diagnostic logging with a strict 1 MiB cap and a one-time hardware summary.
//!
//! Writes strictly under the app's local `logs/` folder (`glint.log`, `glint.prev.log`,
//! `system.txt`). No usernames, hostnames, paths, tokens, account identifiers, track titles, or
//! search queries are ever recorded.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, mpsc};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::store::Settings;

/// Each rolling log file is capped at 512 KiB; at most two files (`glint.log` + `glint.prev.log`)
/// exist, keeping log storage strictly at or below 1 MiB.
const MAX_FILE_BYTES: u64 = 512 * 1024;
const LOG_FILE: &str = "glint.log";
const PREV_FILE: &str = "glint.prev.log";
const SYS_FILE: &str = "system.txt";

static ENABLED: AtomicBool = AtomicBool::new(false);
static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static TX: OnceLock<mpsc::SyncSender<Msg>> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();

enum Msg {
    Line(String),
    Mem { stamp: String, reason: String, art: (usize, usize, usize, usize), gpu: (usize, usize, usize) },
    System(String),
    Clear,
}

/// Starts the background log writer for `dir` when running interactively.
pub fn init(dir: Option<PathBuf>, enabled: bool) {
    let _ = START.get_or_init(Instant::now);
    ENABLED.store(enabled, Ordering::Relaxed);
    let Some(dir) = dir else { return };
    let _ = LOG_DIR.set(dir.clone());
    let (tx, rx) = mpsc::sync_channel::<Msg>(256);
    if TX.set(tx).is_err() {
        return;
    }
    let _ = std::thread::Builder::new().name("glint-log".into()).spawn(move || writer_loop(&dir, rx));
    if enabled {
        event("app", &format!("start version={} arch={} os={}", env!("CARGO_PKG_VERSION"), std::env::consts::ARCH, std::env::consts::OS));
    }
}

/// Enables or disables diagnostic log recording at runtime.
pub fn set_enabled(enabled: bool) {
    let prev = ENABLED.swap(enabled, Ordering::Relaxed);
    if !prev && enabled {
        event("app", "logging enabled");
    }
}

/// Whether local diagnostic logging is currently enabled.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Records one anonymized diagnostic event line (non-blocking; no-op when disabled).
pub fn event(category: &str, message: &str) {
    if !is_enabled() {
        return;
    }
    let Some(tx) = TX.get() else { return };
    let line = format!("{} [{category}] {message}\n", stamp());
    let _ = tx.try_send(Msg::Line(line));
}

/// Samples process memory (including Windows `PAGE_WRITECOMBINE` private pages) on the log thread
/// and appends a compact summary line.
pub fn log_memory(reason: &str, art: (usize, usize, usize, usize), gpu: (usize, usize, usize)) {
    if !is_enabled() {
        return;
    }
    let Some(tx) = TX.get() else { return };
    let _ = tx.try_send(Msg::Mem { stamp: stamp(), reason: reason.to_owned(), art, gpu });
}

/// Writes the anonymized one-time hardware and system summary (`system.txt`).
pub fn write_system(settings: &Settings, method: glint_update::Method, win_size: [f32; 2]) {
    if !is_enabled() {
        return;
    }
    let Some(tx) = TX.get() else { return };
    let text = build_system_info(settings, method, win_size);
    let _ = tx.try_send(Msg::System(text));
}

/// Removes all local log files.
pub fn clear() {
    if let Some(tx) = TX.get() {
        let _ = tx.try_send(Msg::Clear);
    } else if let Some(dir) = LOG_DIR.get() {
        clear_files(dir);
    }
}

/// Current combined size in bytes of local diagnostic files (`glint.log`, `glint.prev.log`, `system.txt`).
pub fn total_bytes() -> u64 {
    let Some(dir) = LOG_DIR.get() else { return 0 };
    [LOG_FILE, PREV_FILE, SYS_FILE].iter().filter_map(|name| fs::metadata(dir.join(name)).ok().map(|m| m.len())).sum()
}

fn stamp() -> String {
    let up = START.get_or_init(Instant::now).elapsed().as_secs_f64();
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("[{h:02}:{m:02}:{s:02} +{up:8.3}s]")
}

fn writer_loop(dir: &Path, rx: mpsc::Receiver<Msg>) {
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Line(line) => append_capped(dir, &line),
            Msg::Mem { stamp, reason, art, gpu } => {
                let mem = sample_process_memory();
                let (dec_n, dec_mb, raw_n, raw_mb) = (art.0, art.1 as f64 / 1_048_576.0, art.2, art.3 as f64 / 1_048_576.0);
                let (pyrs, pyr_mb, ubufs) = (gpu.0, gpu.1 as f64 / 1_048_576.0, gpu.2);
                let line = format!("{stamp} [mem] {reason}: {mem} art={dec_n}({dec_mb:.1}MiB) raw={raw_n}({raw_mb:.1}MiB) pyrs={pyrs}({pyr_mb:.1}MiB) ubufs={ubufs}\n");
                append_capped(dir, &line);
            }
            Msg::System(text) => {
                let _ = fs::create_dir_all(dir);
                let tmp = dir.join("system.txt.tmp");
                let dst = dir.join(SYS_FILE);
                if fs::write(&tmp, text.as_bytes()).is_ok() {
                    let _ = fs::rename(&tmp, &dst);
                }
            }
            Msg::Clear => clear_files(dir),
        }
    }
}

fn clear_files(dir: &Path) {
    for name in [LOG_FILE, PREV_FILE, SYS_FILE, "system.txt.tmp"] {
        let _ = fs::remove_file(dir.join(name));
    }
}

fn append_capped(dir: &Path, line: &str) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let active = dir.join(LOG_FILE);
    let len = fs::metadata(&active).map_or(0, |m| m.len());
    if len.saturating_add(line.len() as u64) > MAX_FILE_BYTES {
        let prev = dir.join(PREV_FILE);
        let _ = fs::remove_file(&prev);
        let _ = fs::rename(&active, &prev);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&active) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn build_system_info(settings: &Settings, method: glint_update::Method, win_size: [f32; 2]) -> String {
    let gpu = rune::marks::notes().into_iter().find(|(k, _)| *k == "adapter").map_or("pending", |(_, v)| v);
    let cores = std::thread::available_parallelism().map_or(0, usize::from);
    let (os_name, cpu_name, ram_mb, screen) = platform_system_info();
    let (w, h) = (win_size[0].round() as u32, win_size[1].round() as u32);
    format!(
        "Glint system summary (anonymized, local only)\n\
         app = Glint {}\n\
         arch = {}\n\
         install = {:?}\n\
         os = {}\n\
         cpu = {} ({} logical cores)\n\
         ram_total_mb = {}\n\
         gpu_adapter = {}\n\
         display = {}\n\
         window = {}x{}\n\
         settings = theme:{:?} shaders:{} cpu_mode:{} reduce_motion:{} quality:{:?} layout:{:?} butterflies:{}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
        method,
        os_name,
        cpu_name,
        cores,
        ram_mb,
        gpu,
        screen,
        w,
        h,
        settings.theme,
        settings.shaders,
        settings.cpu_mode,
        settings.reduce_motion,
        settings.quality,
        settings.player_layout,
        settings.butterflies,
    )
}

#[cfg(windows)]
fn platform_system_info() -> (String, String, u64, String) {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    let os = windows_os_string();
    let cpu = windows_reg_string(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString").unwrap_or_else(|| "unknown".into());
    let mut mem = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    // SAFETY: `mem` has `dwLength` initialized to its exact struct size as required by Win32.
    let ram_mb = if unsafe { GlobalMemoryStatusEx(&mut mem) } != 0 { mem.ullTotalPhys / (1024 * 1024) } else { 0 };
    // SAFETY: `GetSystemMetrics` takes plain integer metric indices and has no pointer arguments.
    let (sx, sy) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    (os, cpu.trim().to_owned(), ram_mb, format!("{sx}x{sy}"))
}

#[cfg(windows)]
fn windows_os_string() -> String {
    let key = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let build = windows_reg_string(key, "CurrentBuildNumber").unwrap_or_default();
    let build_num = build.parse::<u32>().unwrap_or(0);
    let display = windows_reg_string(key, "DisplayVersion").unwrap_or_default();
    let edition = windows_reg_string(key, "EditionID").unwrap_or_default();
    let ubr = windows_reg_dword(key, "UBR");
    let major = if build_num >= 22000 { "Windows 11" } else { "Windows 10" };
    let build_full = match ubr {
        Some(u) if !build.is_empty() => format!("{build}.{u}"),
        _ => build,
    };
    let mut out = major.to_owned();
    if !edition.is_empty() {
        out.push(' ');
        out.push_str(&edition);
    }
    if !display.is_empty() {
        out.push(' ');
        out.push_str(&display);
    }
    if !build_full.is_empty() {
        out.push_str(" (build ");
        out.push_str(&build_full);
        out.push(')');
    }
    out
}

#[cfg(windows)]
fn windows_reg_string(subkey: &str, value: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};

    let sub: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let val: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let mut bytes: u32 = 0;
    // SAFETY: NUL-terminated UTF-16 registry subkey and value name; querying required buffer size.
    let status = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, sub.as_ptr(), val.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), &mut bytes) };
    if status != 0 || !(2..=4096).contains(&bytes) {
        return None;
    }
    let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
    // SAFETY: `buf` is at least `bytes` bytes wide and writable; pointers are valid for the call.
    let status = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, sub.as_ptr(), val.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut bytes) };
    if status != 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

#[cfg(windows)]
fn windows_reg_dword(subkey: &str, value: &str) -> Option<u32> {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW};

    let sub: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let val: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let mut data: u32 = 0;
    let mut bytes: u32 = size_of::<u32>() as u32;
    // SAFETY: `data` is a writable 4-byte `u32` matching `bytes`; strings are NUL-terminated UTF-16.
    let status = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, sub.as_ptr(), val.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), (&mut data as *mut u32).cast(), &mut bytes) };
    (status == 0).then_some(data)
}

/// Releases unused freed working-set pages back to the OS on Windows when hiding or trimming
/// caches so Task Manager reflects lower RAM usage right away.
pub fn trim_working_set() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        // SAFETY: `GetCurrentProcess()` returns the current process pseudo-handle.
        unsafe {
            let _ = K32EmptyWorkingSet(GetCurrentProcess());
        }
    }
}

#[cfg(windows)]
fn sample_process_memory() -> String {
    use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEM_PRIVATE, MEMORY_BASIC_INFORMATION, PAGE_WRITECOMBINE, VirtualQuery};
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut counters: PROCESS_MEMORY_COUNTERS_EX = unsafe {
        // SAFETY: `PROCESS_MEMORY_COUNTERS_EX` is a plain C POD struct of integer fields.
        std::mem::zeroed()
    };
    counters.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    // SAFETY: `GetCurrentProcess()` returns the pseudo-handle for this process; `counters` has `cb` set.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(), counters.cb) };
    let (ws_mb, priv_mb) = if ok != 0 { (counters.WorkingSetSize as f64 / 1_048_576.0, counters.PrivateUsage as f64 / 1_048_576.0) } else { (0.0, 0.0) };

    // Walk virtual address regions to sum committed private `PAGE_WRITECOMBINE` pages (D3D12 upload/UMA heaps).
    let mut addr: usize = 0;
    let mut wc_bytes: u64 = 0;
    let mut mbi: MEMORY_BASIC_INFORMATION = unsafe {
        // SAFETY: `MEMORY_BASIC_INFORMATION` is a plain C POD struct.
        std::mem::zeroed()
    };
    let mbi_size = size_of::<MEMORY_BASIC_INFORMATION>();
    for _ in 0..65_536 {
        // SAFETY: `VirtualQuery` reads VM metadata for `addr` into our valid `mbi` buffer.
        let ret = unsafe { VirtualQuery(addr as *const core::ffi::c_void, &mut mbi, mbi_size) };
        if ret == 0 || mbi.RegionSize == 0 {
            break;
        }
        if mbi.State == MEM_COMMIT && mbi.Type == MEM_PRIVATE && (mbi.Protect & PAGE_WRITECOMBINE) != 0 {
            wc_bytes = wc_bytes.saturating_add(mbi.RegionSize as u64);
        }
        match addr.checked_add(mbi.RegionSize) {
            Some(next) if next > addr => addr = next,
            _ => break,
        }
    }
    let wc_mb = wc_bytes as f64 / 1_048_576.0;
    format!("ws={ws_mb:.1}MiB priv={priv_mb:.1}MiB wc={wc_mb:.1}MiB")
}

#[cfg(target_os = "linux")]
fn platform_system_info() -> (String, String, u64, String) {
    let pretty = fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=")).map(|v| v.trim_matches('"').to_owned()))
        .unwrap_or_else(|| "Linux".into());
    let kernel = fs::read_to_string("/proc/sys/kernel/osrelease").ok().map(|s| s.trim().to_owned()).unwrap_or_default();
    let os = if kernel.is_empty() { pretty } else { format!("{pretty} (kernel {kernel})") };
    let cpu = fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("model name")).and_then(|r| r.split_once(':')).map(|(_, v)| v.trim().to_owned()))
        .unwrap_or_else(|| "unknown".into());
    let ram_mb = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("MemTotal:")).and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok()))
        .map_or(0, |kb| kb / 1024);
    (os, cpu, ram_mb, "unknown".into())
}

#[cfg(target_os = "linux")]
fn sample_process_memory() -> String {
    let rss_kb = fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("VmRSS:")).and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok()))
        .unwrap_or(0);
    format!("rss={:.1}MiB", rss_kb as f64 / 1024.0)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn platform_system_info() -> (String, String, u64, String) {
    (std::env::consts::OS.to_owned(), "unknown".into(), 0, "unknown".into())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn sample_process_memory() -> String {
    "mem=n/a".into()
}

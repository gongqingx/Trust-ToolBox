use std::backtrace::Backtrace;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

fn local_timestamp_ms() -> String {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::SYSTEMTIME;
        use windows_sys::Win32::System::SystemInformation::GetLocalTime;
        let mut st = SYSTEMTIME::default();
        GetLocalTime(&mut st);
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
        )
    }
    #[cfg(not(windows))]
    {
        String::from("0000-00-00 00:00:00.000")
    }
}

const DEBUG_FILE_NAME: &str = "tui_debug.log";
const CRASH_FILE_NAME: &str = "tui_crash.log";
const FALLBACK_DEBUG_LOG_NAME: &str = "trust_toolbox_debug.log";
const FALLBACK_CRASH_LOG_NAME: &str = "trust_toolbox_crash.log";

struct DiagState {
    config_dir: PathBuf,
    debug_enabled: bool,
    debug_file: Option<File>,
    env_snapshot: String,
}

static DIAG_STATE: OnceLock<Mutex<DiagState>> = OnceLock::new();

fn state() -> &'static Mutex<DiagState> {
    DIAG_STATE.get_or_init(|| {
        Mutex::new(DiagState {
            config_dir: PathBuf::new(),
            debug_enabled: false,
            debug_file: None,
            env_snapshot: String::new(),
        })
    })
}

fn open_append_log(cfg_dir: &Path, name: &str, fallback_name: &str) -> Option<(File, PathBuf)> {
    let mut candidates = Vec::with_capacity(2);
    if !cfg_dir.as_os_str().is_empty() {
        candidates.push(cfg_dir.join(name));
    }
    candidates.push(std::env::temp_dir().join(fallback_name));

    for path in candidates {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) {
            return Some((file, path));
        }
    }
    None
}

/// Initializes diagnostic logging and crash report persistence.
pub fn init(config_dir: &Path, enable_debug: bool, env_snapshot: String) {
    let mut target_path = None;
    if let Ok(mut guard) = state().lock() {
        if let Some(mut old) = guard.debug_file.take() {
            let _ = old.flush();
        }
        guard.config_dir = config_dir.to_path_buf();
        guard.debug_enabled = enable_debug;
        guard.env_snapshot = env_snapshot.clone();
        if enable_debug {
            if let Some((file, path)) = open_append_log(config_dir, DEBUG_FILE_NAME, FALLBACK_DEBUG_LOG_NAME) {
                guard.debug_file = Some(file);
                target_path = Some(path);
            }
        }
    }

    if enable_debug {
        if let Some(path) = target_path {
            infof(&format!("==== Trust工具箱 Debug Session Start (log={}) ====", path.display()));
            for line in env_snapshot.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    infof(&format!("env: {trimmed}"));
                }
            }
        }
    }
}

/// Writes a formatted diagnostic line when debug logging is enabled.
pub fn infof(msg: &str) {
    let Ok(mut guard) = state().lock() else {
        return;
    };
    if !guard.debug_enabled {
        return;
    }
    if let Some(file) = guard.debug_file.as_mut() {
        let ts = local_timestamp_ms();
        let _ = writeln!(file, "[{ts}] {msg}");
    }
}

/// Flushes and closes the debug log file if open.
pub fn close() {
    if let Ok(mut guard) = state().lock() {
        if let Some(mut file) = guard.debug_file.take() {
            let _ = file.flush();
        }
    }
}

/// Appends a structured crash report with stack trace and environment snapshot.
pub fn write_crash_report(source: &str, err_msg: &str, backtrace: &str) -> Option<PathBuf> {
    let (cfg_dir, env_snapshot) = if let Ok(guard) = state().lock() {
        (guard.config_dir.clone(), guard.env_snapshot.trim().to_string())
    } else {
        (PathBuf::new(), String::new())
    };

    let now = local_timestamp_ms();
    let mut text = String::new();
    text.push_str("================================================================================\n");
    text.push_str(&format!("Time    : {now}\n"));
    text.push_str(&format!("Source  : {source}\n"));
    text.push_str(&format!(
        "Runtime : v{} {}/{}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    text.push_str(&format!("Error   : {err_msg}\n"));
    if !env_snapshot.is_empty() {
        text.push_str("--- Environment Snapshot ---\n");
        text.push_str(&env_snapshot);
        text.push('\n');
    }
    if !backtrace.is_empty() {
        text.push_str("--- Stack Trace ---\n");
        text.push_str(backtrace);
        if !backtrace.ends_with('\n') {
            text.push('\n');
        }
    }
    text.push_str("================================================================================\n\n");

    let (mut file, path) = open_append_log(&cfg_dir, CRASH_FILE_NAME, FALLBACK_CRASH_LOG_NAME)?;
    let _ = file.write_all(text.as_bytes());
    let _ = file.sync_all();
    Some(path)
}

/// Installs a panic hook that writes structured crash details to tui_crash.log.
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let bt = Backtrace::force_capture();
        let msg = info.to_string();
        let _ = write_crash_report("panic", &msg, &bt.to_string());
        prev(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_crash_report_creates_log() {
        let tmp = std::env::temp_dir().join(format!("tb_diag_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp);
        init(&tmp, true, "TestEnv=1".to_string());
        infof("test diagnostic line");
        let path = write_crash_report("unit_test", "simulated error", "frame 0")
            .expect("expected crash report path");
        assert!(path.exists());
        let content = fs::read_to_string(&path).expect("read crash report");
        assert!(content.contains("simulated error"));
        assert!(content.contains("TestEnv=1"));
        close();
        let _ = fs::remove_dir_all(&tmp);
    }
}

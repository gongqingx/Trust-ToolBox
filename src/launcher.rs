use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use windows_sys::Win32::Foundation::{
    CloseHandle, FARPROC, FILETIME, GetLastError, GlobalFree, HANDLE, HWND,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject,
};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Threading::{
    CREATE_NEW_CONSOLE, CREATE_NEW_PROCESS_GROUP, GetExitCodeProcess, GetProcessTimes,
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    TerminateProcess,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const CF_UNICODETEXT: u32 = 13;
const STILL_ACTIVE_CODE: u32 = 259;

#[derive(Debug)]
struct JobGuard(usize);

impl JobGuard {
    fn as_handle(&self) -> HANDLE {
        self.0 as HANDLE
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe {
                CloseHandle(self.0 as HANDLE);
            }
            self.0 = 0;
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TrackedProcessSnapshot {
    pub pid: u32,
    pub tool_id: String,
    pub tool_name: String,
    pub exe_name: String,
    pub started_at: SystemTime,
    pub running: bool,
    pub admin: bool,
    pub external: bool,
}

#[derive(Debug)]
struct TrackedProcessInner {
    pid: u32,
    tool_id: String,
    tool_name: String,
    exe_name: String,
    started_at: SystemTime,
    create_time_ms: i64,
    running: bool,
    admin: bool,
    external: bool,
    job: Option<JobGuard>,
}

#[allow(dead_code)]
impl TrackedProcessInner {
    fn snapshot(&self) -> TrackedProcessSnapshot {
        TrackedProcessSnapshot {
            pid: self.pid,
            tool_id: self.tool_id.clone(),
            tool_name: self.tool_name.clone(),
            exe_name: self.exe_name.clone(),
            started_at: self.started_at,
            running: self.running,
            admin: self.admin,
            external: self.external,
        }
    }
}

#[derive(Debug, Default)]
struct TrackerState {
    items: Vec<TrackedProcessInner>,
    last_launch: HashMap<String, Instant>,
}

#[derive(Debug, Clone)]
pub struct ProcessTracker {
    state: Arc<Mutex<TrackerState>>,
}

impl Default for ProcessTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl ProcessTracker {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(TrackerState::default())),
        }
    }

    pub fn allow_launch(&self, key: &str, cooldown: Duration) -> bool {
        let mut guard = self.state.lock().unwrap();
        let now = Instant::now();
        if let Some(prev) = guard.last_launch.get(key) {
            if now.duration_since(*prev) < cooldown {
                return false;
            }
        }
        guard.last_launch.insert(key.to_string(), now);
        true
    }

    fn add_inner(&self, tp: TrackedProcessInner) {
        let mut guard = self.state.lock().unwrap();
        guard.items.insert(0, tp);
        if guard.items.len() > 30 {
            guard.items.truncate(30);
        }
    }

    pub fn mark_exited(&self, pid: u32) {
        if pid == 0 {
            return;
        }
        let mut guard = self.state.lock().unwrap();
        for item in &mut guard.items {
            if item.pid == pid && item.running {
                item.running = false;
                item.job = None;
            }
        }
    }

    pub fn snapshot(&self) -> Vec<TrackedProcessSnapshot> {
        let guard = self.state.lock().unwrap();
        guard.items.iter().map(|it| it.snapshot()).collect()
    }

    pub fn has_running(&self) -> bool {
        let guard = self.state.lock().unwrap();
        guard.items.iter().any(|it| it.running && it.pid > 0)
    }

    pub fn running_summary(&self) -> (usize, HashSet<String>) {
        let guard = self.state.lock().unwrap();
        let mut count = 0usize;
        let mut ids = HashSet::new();
        for it in &guard.items {
            if it.running && it.pid > 0 {
                count += 1;
                ids.insert(it.tool_id.clone());
            }
        }
        (count, ids)
    }

    pub fn refresh(&self) {
        let candidates: Vec<(u32, i64)> = {
            let guard = self.state.lock().unwrap();
            guard
                .items
                .iter()
                .filter(|it| it.pid > 0 && it.running)
                .map(|it| (it.pid, it.create_time_ms))
                .collect()
        };

        for (pid, expected_ctime) in candidates {
            let (alive, ctime) = query_win32_process_state(pid);
            let reused = expected_ctime > 0 && ctime > 0 && (ctime - expected_ctime).abs() > 2000;
            if !alive || reused {
                self.mark_exited(pid);
            }
        }
    }

    pub fn kill(&self, pid: u32) -> Result<(), String> {
        if pid == 0 {
            return Err("该启动项由系统外壳托管，无独立 PID".to_string());
        }

        let (job_opt, expected_ctime) = {
            let mut guard = self.state.lock().unwrap();
            let mut found_job = None;
            let mut ctime = 0i64;
            for item in &mut guard.items {
                if item.pid == pid {
                    ctime = item.create_time_ms;
                    if let Some(j) = item.job.take() {
                        item.running = false;
                        found_job = Some(j);
                    }
                    break;
                }
            }
            (found_job, ctime)
        };

        if let Some(job) = job_opt {
            let ok = unsafe { TerminateJobObject(job.as_handle(), 1) };
            if ok != 0 {
                return Ok(());
            }
            let err = unsafe { GetLastError() };
            return Err(format!("TerminateJobObject failed (err={err})"));
        }

        let (alive, ctime) = query_win32_process_state(pid);
        if !alive {
            self.mark_exited(pid);
            return Err(format!("进程 PID {pid} 已退出"));
        }
        if expected_ctime > 0 && ctime > 0 && (ctime - expected_ctime).abs() > 2000 {
            self.mark_exited(pid);
            return Err(format!("PID {pid} 已被系统回收"));
        }

        unsafe {
            let h_proc = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if h_proc.is_null() {
                return Err(format!("OpenProcess failed (err={})", GetLastError()));
            }
            let ok = TerminateProcess(h_proc, 1);
            let err = GetLastError();
            CloseHandle(h_proc);
            if ok == 0 {
                return Err(format!("TerminateProcess failed (err={err})"));
            }
        }

        self.mark_exited(pid);
        Ok(())
    }

    pub fn kill_all(&self) -> usize {
        let snap = self.snapshot();
        let mut killed = 0usize;
        for tp in snap {
            if tp.running && tp.pid > 0 && self.kill(tp.pid).is_ok() {
                killed += 1;
            }
        }
        killed
    }

    pub fn clear_exited(&self) -> usize {
        let mut guard = self.state.lock().unwrap();
        let before = guard.items.len();
        guard.items.retain(|it| it.running);
        before - guard.items.len()
    }
}

fn to_wide_null(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn filetime_to_ms(ft: &FILETIME) -> i64 {
    let ticks = ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64);
    (ticks / 10_000) as i64
}

pub fn query_win32_process_state(pid: u32) -> (bool, i64) {
    if pid == 0 {
        return (false, 0);
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return (false, 0);
        }
        let mut exit_code: u32 = 0;
        let ok = GetExitCodeProcess(h, &mut exit_code);
        if ok == 0 || exit_code != STILL_ACTIVE_CODE {
            CloseHandle(h);
            return (false, 0);
        }
        let mut creation = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let mut exit = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let mut kernel = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let mut user = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let ctime = if GetProcessTimes(h, &mut creation, &mut exit, &mut kernel, &mut user) != 0 {
            filetime_to_ms(&creation)
        } else {
            0
        };
        CloseHandle(h);
        (true, ctime)
    }
}

fn attach_job_object(pid: u32) -> (Option<JobGuard>, i64) {
    if pid == 0 {
        return (None, 0);
    }
    let (_, ctime) = query_win32_process_state(pid);
    unsafe {
        let h_job = CreateJobObjectW(null(), null());
        if h_job.is_null() {
            return (None, ctime);
        }
        let h_proc = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
        if h_proc.is_null() {
            CloseHandle(h_job);
            return (None, ctime);
        }
        let ok = AssignProcessToJobObject(h_job, h_proc);
        CloseHandle(h_proc);
        if ok == 0 {
            CloseHandle(h_job);
            return (None, ctime);
        }
        (Some(JobGuard(h_job as usize)), ctime)
    }
}

type ShellExecuteWFn = unsafe extern "system" fn(
    hwnd: HWND,
    lp_operation: *const u16,
    lp_file: *const u16,
    lp_parameters: *const u16,
    lp_directory: *const u16,
    n_show_cmd: i32,
) -> isize;

static SHELL_EXECUTE_W: OnceLock<Option<ShellExecuteWFn>> = OnceLock::new();

/// Lazily loads `shell32.dll!ShellExecuteW` only when elevated/shell launch or dir open runs.
fn load_shell_execute_w() -> Option<ShellExecuteWFn> {
    *SHELL_EXECUTE_W.get_or_init(|| unsafe {
        let dll = to_wide_null("shell32.dll");
        let hmod = LoadLibraryW(dll.as_ptr());
        if hmod.is_null() {
            return None;
        }
        let proc_ptr: FARPROC = GetProcAddress(hmod, c"ShellExecuteW".as_ptr() as *const u8);
        proc_ptr.map(|f| core::mem::transmute_copy(&f))
    })
}

fn shell_execute(verb: &str, file: &Path, args: &str, dir: &Path) -> Result<(), String> {
    let shell_exec =
        load_shell_execute_w().ok_or_else(|| "无法加载 shell32.dll!ShellExecuteW".to_string())?;
    let v_w = to_wide_null(verb);
    let f_w = to_wide_null(&file.to_string_lossy());
    let a_w = if args.is_empty() {
        None
    } else {
        Some(to_wide_null(args))
    };
    let d_w = if dir.as_os_str().is_empty() {
        None
    } else {
        Some(to_wide_null(&dir.to_string_lossy()))
    };

    let res = unsafe {
        shell_exec(
            null_mut(),
            v_w.as_ptr(),
            f_w.as_ptr(),
            a_w.as_ref().map_or(null(), |v| v.as_ptr()),
            d_w.as_ref().map_or(null(), |v| v.as_ptr()),
            SW_SHOWNORMAL,
        )
    };
    if res <= 32 {
        return Err(format!("ShellExecuteW({verb}) failed with code {res}"));
    }
    Ok(())
}

pub fn launch_normal(
    tracker: &ProcessTracker,
    tool_id: &str,
    tool_name: &str,
    target_path: &Path,
    work_dir: Option<&Path>,
    kind: &str,
) -> Result<TrackedProcessSnapshot, String> {
    if target_path.as_os_str().is_empty() {
        return Err("未找到可启动的目标文件".to_string());
    }
    if !target_path.exists() {
        let fname = target_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        return Err(format!("文件不存在或被杀毒软件拦截: {fname}"));
    }
    let dir = match work_dir {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => target_path.parent().unwrap_or_else(|| Path::new(".")),
    };
    let base_name = target_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let ext = target_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| format!(".{}", s.to_lowercase()))
        .unwrap_or_default();

    if kind == "bat" || ext == ".bat" || ext == ".cmd" {
        let mut child = Command::new("cmd.exe")
            .arg("/c")
            .arg(target_path)
            .current_dir(dir)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .map_err(|e| e.to_string())?;
        let pid = child.id();
        let (job, ctime) = attach_job_object(pid);
        let tp = TrackedProcessInner {
            pid,
            tool_id: tool_id.to_string(),
            tool_name: tool_name.to_string(),
            exe_name: base_name,
            started_at: SystemTime::now(),
            create_time_ms: ctime,
            running: true,
            admin: false,
            external: false,
            job,
        };
        let snap = tp.snapshot();
        tracker.add_inner(tp);
        let tracker_clone = tracker.clone();
        thread::spawn(move || {
            let _ = child.wait();
            tracker_clone.mark_exited(pid);
        });
        return Ok(snap);
    }

    if matches!(kind, "img" | "archive" | "html")
        || matches!(
            ext.as_str(),
            ".jpg" | ".jpeg" | ".png" | ".gif" | ".bmp" | ".html" | ".htm" | ".zip" | ".rar" | ".7z"
        )
    {
        shell_execute("open", target_path, "", dir)?;
        let tp = TrackedProcessInner {
            pid: 0,
            tool_id: tool_id.to_string(),
            tool_name: tool_name.to_string(),
            exe_name: base_name,
            started_at: SystemTime::now(),
            create_time_ms: 0,
            running: false,
            admin: false,
            external: true,
            job: None,
        };
        let snap = tp.snapshot();
        tracker.add_inner(tp);
        return Ok(snap);
    }

    match Command::new(target_path)
        .current_dir(dir)
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .spawn()
    {
        Ok(mut child) => {
            let pid = child.id();
            let (job, ctime) = attach_job_object(pid);
            let tp = TrackedProcessInner {
                pid,
                tool_id: tool_id.to_string(),
                tool_name: tool_name.to_string(),
                exe_name: base_name,
                started_at: SystemTime::now(),
                create_time_ms: ctime,
                running: true,
                admin: false,
                external: false,
                job,
            };
            let snap = tp.snapshot();
            tracker.add_inner(tp);
            let tracker_clone = tracker.clone();
            thread::spawn(move || {
                let _ = child.wait();
                tracker_clone.mark_exited(pid);
            });
            Ok(snap)
        }
        Err(spawn_err) => {
            if let Err(se_err) = shell_execute("open", target_path, "", dir) {
                return Err(format!("{spawn_err} (ShellExecute: {se_err})"));
            }
            let tp = TrackedProcessInner {
                pid: 0,
                tool_id: tool_id.to_string(),
                tool_name: tool_name.to_string(),
                exe_name: base_name,
                started_at: SystemTime::now(),
                create_time_ms: 0,
                running: false,
                admin: false,
                external: true,
                job: None,
            };
            let snap = tp.snapshot();
            tracker.add_inner(tp);
            Ok(snap)
        }
    }
}

pub fn launch_admin(
    tracker: &ProcessTracker,
    tool_id: &str,
    tool_name: &str,
    target_path: &Path,
    work_dir: Option<&Path>,
) -> Result<TrackedProcessSnapshot, String> {
    if target_path.as_os_str().is_empty() {
        return Err("未找到可启动的目标文件".to_string());
    }
    if !target_path.exists() {
        let fname = target_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        return Err(format!("文件不存在或被杀毒软件拦截: {fname}"));
    }
    let dir = match work_dir {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => target_path.parent().unwrap_or_else(|| Path::new(".")),
    };
    shell_execute("runas", target_path, "", dir)?;
    let tp = TrackedProcessInner {
        pid: 0,
        tool_id: tool_id.to_string(),
        tool_name: tool_name.to_string(),
        exe_name: target_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string(),
        started_at: SystemTime::now(),
        create_time_ms: 0,
        running: false,
        admin: true,
        external: true,
        job: None,
    };
    let snap = tp.snapshot();
    tracker.add_inner(tp);
    Ok(snap)
}

pub fn open_in_explorer(target_path: &Path) -> Result<(), String> {
    if target_path.is_file() {
        Command::new("explorer.exe")
            .arg(format!("/select,{}", target_path.display()))
            .spawn()
            .map_err(|e| e.to_string())?;
    } else {
        Command::new("explorer.exe")
            .arg(target_path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let utf16 = to_wide_null(text);
    let byte_len = utf16.len() * 2;

    unsafe {
        let mut opened = false;
        for _ in 0..5 {
            if OpenClipboard(null_mut()) != 0 {
                opened = true;
                break;
            }
            thread::sleep(Duration::from_millis(15));
        }
        if !opened {
            return Err(format!("OpenClipboard failed (err={})", GetLastError()));
        }

        EmptyClipboard();
        let h_mem = GlobalAlloc(GMEM_MOVEABLE, byte_len);
        if h_mem.is_null() {
            let err = GetLastError();
            CloseClipboard();
            return Err(format!("GlobalAlloc failed (err={err})"));
        }

        let ptr = GlobalLock(h_mem);
        if ptr.is_null() {
            let err = GetLastError();
            GlobalFree(h_mem);
            CloseClipboard();
            return Err(format!("GlobalLock failed (err={err})"));
        }

        std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr as *mut u16, utf16.len());
        GlobalUnlock(h_mem);

        let set_res = SetClipboardData(CF_UNICODETEXT, h_mem);
        if set_res.is_null() {
            let err = GetLastError();
            GlobalFree(h_mem);
            CloseClipboard();
            return Err(format!("SetClipboardData failed (err={err})"));
        }
        CloseClipboard();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn allow_launch_debounce() {
        let tr = ProcessTracker::new();
        let cooldown = Duration::from_millis(120);
        assert!(tr.allow_launch("cpuz|cpuz.exe", cooldown));
        assert!(!tr.allow_launch("cpuz|cpuz.exe", cooldown));
        thread::sleep(Duration::from_millis(150));
        assert!(tr.allow_launch("cpuz|cpuz.exe", cooldown));
    }

    #[test]
    fn copy_clipboard_works() {
        copy_to_clipboard("Trust工具箱-剪贴板测试").expect("copy_to_clipboard");
    }

    #[test]
    fn missing_executable_message() {
        let tr = ProcessTracker::new();
        let missing = std::env::temp_dir().join("tb_absent_tool_test.exe");
        let err = launch_normal(&tr, "absent", "缺失工具", &missing, None, "exe").unwrap_err();
        assert!(err.contains("文件不存在或被杀毒软件拦截"), "got {err}");
    }

    #[test]
    fn job_object_kills_child_process_tree() {
        let tmp_dir = std::env::temp_dir().join(format!("tb_job_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp_dir);
        fs::create_dir_all(&tmp_dir).unwrap();
        let bat_path = tmp_dir.join("spawn_ping.bat");
        fs::write(&bat_path, "@echo off\r\nping.exe -n 30 127.0.0.1 >nul\r\n").unwrap();

        let tracker = ProcessTracker::new();
        let tp = launch_normal(&tracker, "test_bat", "测试批处理", &bat_path, Some(&tmp_dir), "bat")
            .expect("launch bat");
        assert!(tp.pid > 0);

        thread::sleep(Duration::from_millis(450));
        tracker.kill(tp.pid).expect("tracker.kill");
        thread::sleep(Duration::from_millis(300));

        let (alive, _) = query_win32_process_state(tp.pid);
        assert!(!alive, "root cmd.exe PID {} should be terminated", tp.pid);

        let _ = fs::remove_dir_all(&tmp_dir);
    }
}

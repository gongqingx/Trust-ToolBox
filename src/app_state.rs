use crate::catalog::{Catalog, Tool, extract_password};
use crate::config::Config;
use crate::gdi::{Rect, THEME, ThemePalette};
use crate::hwinfo::{HardwareReport, format_plain_text, parse_hwinfo, shorten_model_name};
use crate::icons::{
    IconCache, IconJob, PreviewImageCache, decode_image_preview_gdi, extract_icon_job_raw,
};
use crate::launcher::{self, ProcessTracker};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const WM_APP_WORKER: u32 = 0x8001; // WM_APP + 1

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveView {
    Hardware,
    Pinned,
    AllTools,
    Category,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPanel {
    Sidebar = 0,
    Main = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveModal {
    None,
    Help,
    Variants,
    ScreenTest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollTarget {
    Sidebar,
    MainList,
    Hardware,
    Detail,
    Help,
    Variants,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HitAction {
    FocusSearch,
    ClearSearch,
    TogglePreview,
    OpenHelp,
    FocusPanel(FocusPanel),
    SelectSidebar(usize),
    SelectTool(usize),
    CycleSort,
    ReloadHwInfo,
    LaunchTulading,
    ExportHwInfo,
    CopyHwInfo,
    LaunchSelected(bool),
    OpenVariants,
    OpenToolDir,
    TogglePin,
    CopyPassword,
    CloseModal,
    SelectVariant(usize),
    LaunchVariant(usize, bool),
    ScreenStep(i32),
    ToggleScreenHud,
    ScrollbarDrag(ScrollTarget),
    ModalBackdrop,
    ModalInner,
}

#[derive(Debug, Clone)]
pub struct HitRegion {
    pub rect: Rect,
    pub action: HitAction,
    pub pointer_cursor: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ScrollTrack {
    pub target: ScrollTarget,
    pub viewport: Rect,
    pub track: Rect,
    pub thumb: Rect,
    pub content_h: i32,
}

#[derive(Debug, Clone, Default)]
pub struct FrameLayout {
    pub hits: Vec<HitRegion>,
    pub scrolls: Vec<ScrollTrack>,
    pub list_viewport_rect: Option<Rect>,
}

#[derive(Default)]
pub struct UiEffects {
    pub repaint: bool,
    pub activity: bool,
    pub toast_timer: bool,
    pub set_fullscreen: Option<bool>,
    pub trim_now: bool,
    pub quit: bool,
}

pub struct SidebarEntry {
    pub is_header: bool,
    pub id: String,
    pub view_kind: &'static str,
    pub label: String,
    pub icon_key: &'static str,
    pub count: usize,
    pub show_count: bool,
    pub selected: bool,
}

pub struct HwSummaryItem {
    pub icon_key: &'static str,
    pub title: &'static str,
    pub primary: String,
    pub secondary: String,
}

pub struct HwSpecRow {
    pub icon_key: Option<&'static str>,
    pub label: String,
    pub value: String,
    pub group_start: bool,
}

pub fn category_icon_key(id: &str) -> &'static str {
    match id {
        "overview" => "overview",
        "cpu" => "cpu",
        "mobo" => "mobo",
        "ram" => "ram",
        "gpu" => "gpu",
        "disk" => "disk",
        "screen" => "screen",
        "burn" => "burn",
        "periph" => "periph",
        "other" => "other",
        _ => "folder",
    }
}

pub fn tool_kind_label(tool: &Tool) -> &'static str {
    match tool.kind.as_str() {
        "bat" => "BAT",
        "archive" => "ZIP",
        "img" => "IMG",
        "html" => "WEB",
        "tui" => "TUI",
        _ => {
            let lower = tool.entry_path.to_string_lossy().to_lowercase();
            if lower.contains("64") {
                "x64"
            } else if lower.contains("86") || lower.contains("32") {
                "x86"
            } else {
                "EXE"
            }
        }
    }
}

pub fn sort_label(mode: &str) -> &'static str {
    match mode {
        "name" => "名称",
        "type" => "类型",
        _ => "默认",
    }
}

pub fn non_empty<'a>(s: &'a str, fallback: &'a str) -> &'a str {
    if s.trim().is_empty() { fallback } else { s }
}

fn paren(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        String::new()
    } else {
        format!("({t})")
    }
}

pub enum WorkerReq {
    ExtractIcons(Vec<IconJob>),
    DecodePreview {
        gen: u64,
        path: PathBuf,
        max_dim: u32,
    },
}

pub enum WorkerRes {
    IconsBatch(Vec<(String, usize)>),
    PreviewDecoded {
        gen: u64,
        path: PathBuf,
        max_dim: u32,
        result: Result<(crate::gdi::GdiBitmap, u32, u32), String>,
    },
}

pub struct AppState {
    pub data_root: PathBuf,
    pub config_dir: PathBuf,
    pub cat: Catalog,
    pub cfg: Config,
    pub hw: HardwareReport,
    pub tracker: ProcessTracker,
    pub icon_cache: IconCache,
    pub preview_cache: PreviewImageCache,

    worker_tx: Option<mpsc::Sender<WorkerReq>>,
    res_tx: mpsc::Sender<WorkerRes>,
    res_rx: mpsc::Receiver<WorkerRes>,
    pub hwnd_raw: Arc<AtomicIsize>,
    pub latest_preview_gen: Arc<AtomicU64>,
    pub preview_gen: u64,

    pub active_view: ActiveView,
    pub active_category: String,
    pub focus_panel: FocusPanel,
    pub search_focused: bool,
    pub search_query: String,
    pub selected_tool_idx: usize,
    pub selected_variant_idx: usize,

    pub active_modal: ActiveModal,
    pub screen_pattern_idx: i32,
    pub show_screen_hud: bool,
    pub was_fullscreen_before_test: bool,
    pub fullscreen_active: bool,

    pub toast_msg: String,
    pub cached_tools: Vec<(usize, usize)>,
    pub initial_trim_pending: bool,

    pub scroll_sidebar: f32,
    pub scroll_list: f32,
    pub scroll_hw: f32,
    pub scroll_detail: f32,
    pub scroll_help: f32,
    pub scroll_variants: f32,

    pub hovered_action: Option<HitAction>,
    pub last_frame: FrameLayout,
}

impl AppState {
    pub fn new(
        data_root: PathBuf,
        config_dir: PathBuf,
        cat: Catalog,
        mut cfg: Config,
        hw: HardwareReport,
    ) -> Self {
        cfg.pinned.retain(|id| cat.get(id).is_some());
        cfg.recent.retain(|id| cat.get(id).is_some());

        let (active_view, active_category) = if let Some(first) = cat.categories.first() {
            (ActiveView::Category, first.id.clone())
        } else {
            (ActiveView::Hardware, String::new())
        };
        let (res_tx, res_rx) = mpsc::channel();

        let mut st = Self {
            data_root,
            config_dir,
            cat,
            cfg,
            hw,
            tracker: ProcessTracker::new(),
            icon_cache: IconCache::default(),
            preview_cache: PreviewImageCache::default(),
            worker_tx: None,
            res_tx,
            res_rx,
            hwnd_raw: Arc::new(AtomicIsize::new(0)),
            latest_preview_gen: Arc::new(AtomicU64::new(0)),
            preview_gen: 0,
            active_view,
            active_category,
            focus_panel: FocusPanel::Main,
            search_focused: false,
            search_query: String::new(),
            selected_tool_idx: 0,
            selected_variant_idx: 0,
            active_modal: ActiveModal::None,
            screen_pattern_idx: 0,
            show_screen_hud: true,
            was_fullscreen_before_test: false,
            fullscreen_active: false,
            toast_msg: String::new(),
            cached_tools: Vec::new(),
            initial_trim_pending: true,
            scroll_sidebar: 0.0,
            scroll_list: 0.0,
            scroll_hw: 0.0,
            scroll_detail: 0.0,
            scroll_help: 0.0,
            scroll_variants: 0.0,
            hovered_action: None,
            last_frame: FrameLayout::default(),
        };
        st.recompute_tools();
        st.prefetch_visible_icons();
        st.request_preview_for_selected();
        st
    }

    pub fn scroll_offset_mut(&mut self, target: ScrollTarget) -> &mut f32 {
        match target {
            ScrollTarget::Sidebar => &mut self.scroll_sidebar,
            ScrollTarget::MainList => &mut self.scroll_list,
            ScrollTarget::Hardware => &mut self.scroll_hw,
            ScrollTarget::Detail => &mut self.scroll_detail,
            ScrollTarget::Help => &mut self.scroll_help,
            ScrollTarget::Variants => &mut self.scroll_variants,
        }
    }

    pub fn send_worker(&mut self, mut req: WorkerReq) {
        if let Some(tx) = self.worker_tx.as_ref() {
            match tx.send(req) {
                Ok(()) => return,
                Err(mpsc::SendError(returned)) => {
                    req = returned;
                    self.worker_tx = None;
                }
            }
        }
        let (tx, rx) = mpsc::channel::<WorkerReq>();
        self.worker_tx = Some(tx.clone());
        let res_tx = self.res_tx.clone();
        let hwnd_raw = Arc::clone(&self.hwnd_raw);
        let worker_preview_gen = Arc::clone(&self.latest_preview_gen);
        let _ = std::thread::Builder::new()
            .name("trust-bg-worker".to_string())
            .stack_size(512 * 1024)
            .spawn(move || {
                let notify = |msg: WorkerRes| -> Result<(), ()> {
                    res_tx.send(msg).map_err(|_| ())?;
                    let raw = hwnd_raw.load(Ordering::Acquire);
                    if raw != 0 {
                        unsafe {
                            PostMessageW(raw as HWND, WM_APP_WORKER, 0, 0);
                        }
                    }
                    Ok(())
                };
                let mut batch = Vec::with_capacity(6);
                loop {
                    let req = match rx.recv_timeout(Duration::from_secs(3)) {
                        Ok(r) => r,
                        Err(mpsc::RecvTimeoutError::Timeout)
                        | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    match req {
                        WorkerReq::ExtractIcons(jobs) => {
                            for job in jobs {
                                let raw = extract_icon_job_raw(&job, 48);
                                batch.push((job.tool_id, raw));
                                if batch.len() >= 6 {
                                    let send_batch =
                                        std::mem::replace(&mut batch, Vec::with_capacity(6));
                                    if notify(WorkerRes::IconsBatch(send_batch)).is_err() {
                                        return;
                                    }
                                }
                            }
                            if !batch.is_empty() {
                                let send_batch = std::mem::take(&mut batch);
                                if notify(WorkerRes::IconsBatch(send_batch)).is_err() {
                                    return;
                                }
                            }
                        }
                        WorkerReq::DecodePreview {
                            gen,
                            path,
                            max_dim,
                        } => {
                            if gen != worker_preview_gen.load(Ordering::Relaxed) {
                                continue;
                            }
                            let result = decode_image_preview_gdi(&path, max_dim);
                            if gen != worker_preview_gen.load(Ordering::Relaxed) {
                                continue;
                            }
                            if notify(WorkerRes::PreviewDecoded {
                                gen,
                                path,
                                max_dim,
                                result,
                            })
                            .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
            });
        let _ = tx.send(req);
    }

    pub fn drain_worker(&mut self, fx: &mut UiEffects) {
        while let Ok(res) = self.res_rx.try_recv() {
            match res {
                WorkerRes::IconsBatch(items) => {
                    for (tool_id, raw_hicon) in items {
                        self.icon_cache.insert_raw_hicon(tool_id, raw_hicon);
                    }
                    fx.repaint = true;
                    fx.activity = true;
                }
                WorkerRes::PreviewDecoded {
                    gen,
                    path,
                    max_dim,
                    result,
                } => {
                    if gen != self.preview_gen {
                        continue;
                    }
                    if let Ok((bmp, ow, oh)) = result {
                        self.preview_cache.insert_bitmap(&path, max_dim, bmp, ow, oh);
                        fx.repaint = true;
                    }
                    fx.activity = true;
                }
            }
        }
    }

    pub fn prefetch_visible_icons(&mut self) {
        let mut jobs = Vec::new();
        for &idx in &self.cached_tools {
            if let Some(t) = self.cat.get_by_idx(idx) {
                let (_, job) = self.icon_cache.get_or_queue(t);
                if let Some(j) = job {
                    jobs.push(j);
                }
            }
        }
        if !jobs.is_empty() {
            self.send_worker(WorkerReq::ExtractIcons(jobs));
        }
    }

    pub fn request_preview_for_selected(&mut self) {
        self.preview_gen += 1;
        self.latest_preview_gen.store(self.preview_gen, Ordering::Relaxed);
        self.scroll_detail = 0.0;

        let Some(tool) = self.selected_tool().cloned() else {
            self.preview_cache.clear();
            return;
        };
        let (_, job) = self.icon_cache.get_or_queue(&tool);
        if let Some(j) = job {
            self.send_worker(WorkerReq::ExtractIcons(vec![j]));
        }
        if tool.kind == "img" && !tool.entry_path.as_os_str().is_empty() {
            if self.preview_cache.get_cached(&tool.entry_path, 480).is_none() {
                self.send_worker(WorkerReq::DecodePreview {
                    gen: self.preview_gen,
                    path: tool.entry_path,
                    max_dim: 480,
                });
            }
        } else {
            self.preview_cache.clear();
        }
    }

    pub fn selected_tool(&self) -> Option<&Tool> {
        if self.active_view == ActiveView::Hardware {
            return None;
        }
        let &idx = self.cached_tools.get(self.selected_tool_idx)?;
        self.cat.get_by_idx(idx)
    }

    pub fn current_view_title(&self) -> String {
        match self.active_view {
            ActiveView::Hardware => "硬件概览".to_string(),
            ActiveView::Pinned => "收藏夹".to_string(),
            ActiveView::AllTools => "全部工具".to_string(),
            ActiveView::Category => self
                .cat
                .categories
                .iter()
                .find(|c| c.id == self.active_category)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "工具列表".to_string()),
        }
    }

    pub fn build_sidebar_entries(&self) -> Vec<SidebarEntry> {
        let mut out = vec![
            SidebarEntry {
                is_header: true,
                id: String::new(),
                view_kind: "",
                label: "工作台".to_string(),
                icon_key: "",
                count: 0,
                show_count: false,
                selected: false,
            },
            SidebarEntry {
                is_header: false,
                id: "hardware".to_string(),
                view_kind: "hardware",
                label: "硬件概览".to_string(),
                icon_key: "dashboard",
                count: 0,
                show_count: false,
                selected: self.active_view == ActiveView::Hardware,
            },
        ];

        out.push(SidebarEntry {
            is_header: false,
            id: "all".to_string(),
            view_kind: "all",
            label: "全部工具".to_string(),
            icon_key: "grid",
            count: self.cat.total_tools(),
            show_count: true,
            selected: self.active_view == ActiveView::AllTools,
        });
        out.push(SidebarEntry {
            is_header: false,
            id: "pinned".to_string(),
            view_kind: "pinned",
            label: "收藏夹".to_string(),
            icon_key: "star",
            count: self.cfg.pinned.len(),
            show_count: true,
            selected: self.active_view == ActiveView::Pinned,
        });
        out.push(SidebarEntry {
            is_header: true,
            id: String::new(),
            view_kind: "",
            label: "工具分类".to_string(),
            icon_key: "",
            count: 0,
            show_count: false,
            selected: false,
        });

        for c in &self.cat.categories {
            out.push(SidebarEntry {
                is_header: false,
                id: c.id.clone(),
                view_kind: "category",
                label: c.name.clone(),
                icon_key: category_icon_key(&c.id),
                count: c.tools.len(),
                show_count: true,
                selected: self.active_view == ActiveView::Category && self.active_category == c.id,
            });
        }
        out
    }

    pub fn build_hw_summary(&self) -> [HwSummaryItem; 4] {
        let rep = &self.hw;

        let cpu_primary = if rep.cpu_model.trim().is_empty() {
            "未检测到".to_string()
        } else {
            shorten_model_name(&rep.cpu_model, 24)
        };
        let mut cpu_sec_parts = Vec::new();
        if !rep.cpu_cores.trim().is_empty() || !rep.cpu_threads.trim().is_empty() {
            cpu_sec_parts.push(format!(
                "{}核 {}线程",
                non_empty(&rep.cpu_cores, "?"),
                non_empty(&rep.cpu_threads, "?")
            ));
        }
        if !rep.cpu_clock.trim().is_empty() {
            cpu_sec_parts.push(rep.cpu_clock.trim().to_string());
        }
        let cpu_secondary = if cpu_sec_parts.is_empty() {
            "规格未知".to_string()
        } else {
            cpu_sec_parts.join(" · ")
        };

        let (gpu_primary, gpu_secondary) = if let Some(g) = rep.gpus.first() {
            let p = if g.model.trim().is_empty() {
                "未检测到".to_string()
            } else {
                shorten_model_name(&g.model, 24)
            };
            let mut sec = Vec::new();
            if !g.vram.trim().is_empty() {
                sec.push(format!("显存 {}", g.vram.trim()));
            }
            if rep.gpus.len() > 1 {
                sec.push(format!("共 {} 张", rep.gpus.len()));
            }
            let s = if sec.is_empty() {
                "显示适配器".to_string()
            } else {
                sec.join(" · ")
            };
            (p, s)
        } else {
            ("未检测到".to_string(), "显示适配器".to_string())
        };

        let total_mem = rep.total_memory_gb();
        let mem_primary = if total_mem.is_empty() && rep.memory_type.trim().is_empty() {
            "未检测到".to_string()
        } else {
            format!("{} {}", total_mem, rep.memory_type.trim())
                .trim()
                .to_string()
        };
        let mut mem_sec = Vec::new();
        if !rep.memory_modules.is_empty() {
            mem_sec.push(format!("{} 条插槽", rep.memory_modules.len()));
            if let Some(spd) = rep
                .memory_modules
                .iter()
                .map(|m| m.speed.trim())
                .find(|s| !s.is_empty())
            {
                mem_sec.push(spd.to_string());
            }
        }
        let mem_secondary = if mem_sec.is_empty() {
            "物理内存".to_string()
        } else {
            mem_sec.join(" · ")
        };

        let os_primary = if rep.os_name.trim().is_empty() {
            "未检测到".to_string()
        } else {
            rep.os_name.trim().to_string()
        };
        let mut os_sec = Vec::new();
        if !rep.os_build.trim().is_empty() {
            os_sec.push(format!("Build {}", rep.os_build.trim()));
        }
        if !rep.os_arch.trim().is_empty() {
            os_sec.push(rep.os_arch.trim().to_string());
        }
        let os_secondary = if os_sec.is_empty() {
            "操作系统".to_string()
        } else {
            os_sec.join(" · ")
        };

        [
            HwSummaryItem {
                icon_key: "cpu",
                title: "处理器",
                primary: cpu_primary,
                secondary: cpu_secondary,
            },
            HwSummaryItem {
                icon_key: "gpu",
                title: "显卡",
                primary: gpu_primary,
                secondary: gpu_secondary,
            },
            HwSummaryItem {
                icon_key: "ram",
                title: "内存",
                primary: mem_primary,
                secondary: mem_secondary,
            },
            HwSummaryItem {
                icon_key: "overview",
                title: "系统",
                primary: os_primary,
                secondary: os_secondary,
            },
        ]
    }

    pub fn build_hw_rows(&self) -> Vec<HwSpecRow> {
        let rep = &self.hw;
        fn join_dots(parts: Vec<String>) -> String {
            let filtered: Vec<String> = parts
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if filtered.is_empty() {
                "未检测到".to_string()
            } else {
                filtered.join(" · ")
            }
        }
        fn push_row(
            rows: &mut Vec<HwSpecRow>,
            icon_key: Option<&'static str>,
            group_start: bool,
            label: impl Into<String>,
            value: String,
        ) {
            rows.push(HwSpecRow {
                icon_key,
                label: label.into(),
                value,
                group_start,
            });
        }

        let mut rows = Vec::new();

        // 1. 操作系统
        let os_build = if rep.os_build.trim().is_empty() {
            String::new()
        } else {
            format!("Build {}", rep.os_build.trim())
        };
        push_row(
            &mut rows,
            Some("overview"),
            false,
            "操作系统",
            join_dots(vec![rep.os_name.clone(), os_build, rep.os_arch.clone()]),
        );

        // 2. 整机 / 主板 / BIOS
        let sys_model = format!("{} {}", rep.system_vendor.trim(), rep.system_model.trim())
            .trim()
            .to_string();
        let sys_full = format!("{sys_model} {}", paren(&rep.system_type))
            .trim()
            .to_string();
        push_row(
            &mut rows,
            Some("mobo"),
            true,
            "整机型号",
            join_dots(vec![sys_full]),
        );

        let board = format!("{} {}", rep.board_brand.trim(), rep.board_model.trim())
            .trim()
            .to_string();
        let chipset_name = non_empty(&rep.chipset_desc, &rep.chipset_model).trim().to_string();
        let chipset = if chipset_name.is_empty() {
            String::new()
        } else if rep.chipset_step.trim().is_empty() {
            chipset_name
        } else {
            format!("{chipset_name} (步进 {})", rep.chipset_step.trim())
        };
        push_row(
            &mut rows,
            None,
            false,
            "主板芯片",
            join_dots(vec![board, chipset]),
        );

        let bios_ver = if rep.bios_version.trim().is_empty() {
            String::new()
        } else {
            format!("版本 {}", rep.bios_version.trim())
        };
        let bios_oem = rep.bios_oem.trim();
        let bios_oem_part = if !bios_oem.is_empty()
            && bios_oem != rep.bios_vendor.trim()
            && bios_oem != rep.bios_version.trim()
        {
            bios_oem.to_string()
        } else {
            String::new()
        };
        push_row(
            &mut rows,
            None,
            false,
            "BIOS 固件",
            join_dots(vec![rep.bios_vendor.clone(), bios_ver, bios_oem_part]),
        );

        // 3. 处理器
        let cores_threads = if rep.cpu_cores.trim().is_empty() && rep.cpu_threads.trim().is_empty() {
            String::new()
        } else {
            format!(
                "{}核心 {}线程",
                non_empty(&rep.cpu_cores, "?"),
                non_empty(&rep.cpu_threads, "?")
            )
        };
        let cpu_clock = if rep.cpu_clock.trim().is_empty() {
            String::new()
        } else {
            format!("主频 {}", rep.cpu_clock.trim())
        };
        let cpu_bclk = if rep.cpu_bclk.trim().is_empty() {
            String::new()
        } else {
            format!("外频 {}", rep.cpu_bclk.trim())
        };
        push_row(
            &mut rows,
            Some("cpu"),
            true,
            "处理器",
            join_dots(vec![
                rep.cpu_model.clone(),
                cores_threads,
                cpu_clock,
                cpu_bclk,
                rep.cpu_socket.clone(),
            ]),
        );

        // 4. 物理内存
        let format_mem = |mm: &crate::hwinfo::MemoryModule| -> String {
            let part = if mm.part_number.trim().is_empty() {
                String::new()
            } else {
                format!("型号 {}", mm.part_number.trim())
            };
            let sn = mm.serial.trim();
            let sn_part = if !sn.is_empty() && sn != "未知" {
                format!("SN {sn}")
            } else {
                String::new()
            };
            join_dots(vec![
                mm.capacity.clone(),
                rep.memory_type.clone(),
                mm.speed.clone(),
                mm.manufacturer.clone(),
                part,
                sn_part,
            ])
        };
        if rep.memory_modules.is_empty() {
            push_row(&mut rows, Some("ram"), true, "物理内存", "未检测到".to_string());
        } else if rep.memory_modules.len() == 1 {
            push_row(
                &mut rows,
                Some("ram"),
                true,
                "物理内存",
                format_mem(&rep.memory_modules[0]),
            );
        } else {
            let total = rep.total_memory_gb();
            let sum_head = format!("共 {} {}", non_empty(&total, "?"), rep.memory_type.trim())
                .trim()
                .to_string();
            let slots_txt = format!("{} 条插槽", rep.memory_modules.len());
            push_row(
                &mut rows,
                Some("ram"),
                true,
                "物理内存",
                join_dots(vec![sum_head, slots_txt]),
            );
            for mm in &rep.memory_modules {
                push_row(
                    &mut rows,
                    None,
                    false,
                    format!("内存插槽 #{}", mm.index),
                    format_mem(mm),
                );
            }
        }

        // 5. 显卡
        if rep.gpus.is_empty() {
            push_row(&mut rows, Some("gpu"), true, "显卡", "未检测到".to_string());
        } else {
            let multi = rep.gpus.len() > 1;
            for (idx, g) in rep.gpus.iter().enumerate() {
                let lbl = if multi {
                    format!("显卡 #{}", g.index)
                } else {
                    "显卡".to_string()
                };
                let vram = if g.vram.trim().is_empty() {
                    String::new()
                } else {
                    format!("显存 {}", g.vram.trim())
                };
                let drv = if g.driver_version.trim().is_empty() {
                    String::new()
                } else {
                    format!("驱动 {}", g.driver_version.trim())
                };
                let step = if g.stepping.trim().is_empty() {
                    String::new()
                } else {
                    format!("步进 {}", g.stepping.trim())
                };
                push_row(
                    &mut rows,
                    if idx == 0 { Some("gpu") } else { None },
                    idx == 0,
                    lbl,
                    join_dots(vec![g.model.clone(), vram, drv, step]),
                );
            }
        }

        // 6. 显示器
        if rep.monitors.is_empty() {
            push_row(&mut rows, Some("screen"), true, "显示器", "未检测到".to_string());
        } else {
            let multi = rep.monitors.len() > 1;
            for (idx, m) in rep.monitors.iter().enumerate() {
                let lbl = if multi {
                    format!("显示器 #{}", m.index)
                } else {
                    "显示器".to_string()
                };
                let name_part = format!("{} {}", m.model.trim(), paren(&m.id))
                    .trim()
                    .to_string();
                let sz = m.size.trim();
                let dims = m.dimensions.trim();
                let size_part = if !sz.is_empty() && !dims.is_empty() {
                    format!("{sz} ({dims})")
                } else if !sz.is_empty() {
                    sz.to_string()
                } else {
                    dims.to_string()
                };
                push_row(
                    &mut rows,
                    if idx == 0 { Some("screen") } else { None },
                    idx == 0,
                    lbl,
                    join_dots(vec![name_part, size_part, m.manufacture.clone()]),
                );
            }
        }

        // 7. 物理硬盘
        if rep.disks.is_empty() {
            push_row(&mut rows, Some("disk"), true, "物理硬盘", "未检测到".to_string());
        } else {
            let multi = rep.disks.len() > 1;
            for (idx, d) in rep.disks.iter().enumerate() {
                let lbl = if multi {
                    format!("硬盘 #{}", d.index)
                } else {
                    "物理硬盘".to_string()
                };
                let cap = d.capacity.trim();
                let act = d.actual_size.trim();
                let cap_part = if !cap.is_empty() && !act.is_empty() {
                    format!("{cap} (实际 {act})")
                } else if !cap.is_empty() {
                    cap.to_string()
                } else {
                    act.to_string()
                };
                push_row(
                    &mut rows,
                    if idx == 0 { Some("disk") } else { None },
                    idx == 0,
                    lbl,
                    join_dots(vec![d.model.clone(), cap_part, d.bus_type.clone()]),
                );
            }
        }

        // 8. 声卡音频
        let audio_parts: Vec<String> = rep
            .audio_devices
            .iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect();
        let audio_val = if audio_parts.is_empty() {
            "未检测到".to_string()
        } else {
            audio_parts.join("  /  ")
        };
        push_row(&mut rows, Some("periph"), true, "声卡音频", audio_val);

        // 9. 网络适配器
        if rep.net_adapters.is_empty() {
            push_row(
                &mut rows,
                Some("web"),
                true,
                "网络适配器",
                "未检测到".to_string(),
            );
        } else {
            let multi = rep.net_adapters.len() > 1;
            for (idx, n) in rep.net_adapters.iter().enumerate() {
                let lbl = if multi {
                    format!("网卡 #{}", n.index)
                } else {
                    "网络适配器".to_string()
                };
                let mac_part = if n.mac.trim().is_empty() {
                    String::new()
                } else {
                    format!("MAC {}", n.mac.trim())
                };
                push_row(
                    &mut rows,
                    if idx == 0 { Some("web") } else { None },
                    idx == 0,
                    lbl,
                    join_dots(vec![
                        n.model.clone(),
                        n.adapter_type.clone(),
                        n.speed.clone(),
                        mac_part,
                    ]),
                );
            }
        }

        rows
    }

    pub fn recompute_tools(&mut self) {
        let q = self.search_query.trim();
        let mut raw: Vec<(usize, usize)> = if !q.is_empty() && self.active_view != ActiveView::Pinned {
            self.cat.all_tool_indices.clone()
        } else {
            match self.active_view {
                ActiveView::Pinned => self
                    .cfg
                    .pinned
                    .iter()
                    .filter_map(|id| self.cat.by_id.get(id).copied())
                    .collect(),
                ActiveView::AllTools => self.cat.all_tool_indices.clone(),
                ActiveView::Category => self
                    .cat
                    .categories
                    .iter()
                    .enumerate()
                    .find(|(_, c)| c.id == self.active_category)
                    .map(|(ci, c)| (0..c.tools.len()).map(|ti| (ci, ti)).collect())
                    .unwrap_or_default(),
                ActiveView::Hardware => Vec::new(),
            }
        };

        raw.retain(|&idx| self.cat.get_by_idx(idx).is_some_and(|t| t.match_query(q)));

        match self.cfg.sort_mode.as_str() {
            "name" => raw.sort_by(|&a, &b| {
                let ta = self.cat.get_by_idx(a).map(|t| t.name.to_lowercase()).unwrap_or_default();
                let tb = self.cat.get_by_idx(b).map(|t| t.name.to_lowercase()).unwrap_or_default();
                ta.cmp(&tb)
            }),
            "type" => raw.sort_by(|&a, &b| {
                let ta = self.cat.get_by_idx(a);
                let tb = self.cat.get_by_idx(b);
                let ka = ta.map(|t| t.kind.as_str()).unwrap_or("");
                let kb = tb.map(|t| t.kind.as_str()).unwrap_or("");
                let na = ta.map(|t| t.name.to_lowercase()).unwrap_or_default();
                let nb = tb.map(|t| t.name.to_lowercase()).unwrap_or_default();
                ka.cmp(kb).then_with(|| na.cmp(&nb))
            }),
            _ => {}
        }

        self.cached_tools = raw;
        if self.cached_tools.is_empty() {
            self.selected_tool_idx = 0;
        } else if self.selected_tool_idx >= self.cached_tools.len() {
            self.selected_tool_idx = self.cached_tools.len() - 1;
        }
    }

    pub fn set_toast(&mut self, msg: impl Into<String>, fx: &mut UiEffects) {
        self.toast_msg = msg.into();
        fx.toast_timer = true;
        fx.repaint = true;
    }

    fn ensure_selected_tool_visible(&mut self, logical_scale: f32) {
        let Some(vp) = self.last_frame.list_viewport_rect else {
            return;
        };
        let row_h = (54.0 * logical_scale).round().max(1.0);
        let top = self.selected_tool_idx as f32 * row_h;
        let bottom = top + row_h;
        let vp_h = vp.h.max(1) as f32;
        if top < self.scroll_list {
            self.scroll_list = top;
        } else if bottom > self.scroll_list + vp_h {
            self.scroll_list = (bottom - vp_h).max(0.0);
        }
    }

    pub fn select_sidebar_entry(&mut self, entry_idx: usize, fx: &mut UiEffects) {
        let entries = self.build_sidebar_entries();
        let Some(entry) = entries.get(entry_idx) else {
            return;
        };
        if entry.is_header {
            return;
        }
        match entry.view_kind {
            "hardware" => self.active_view = ActiveView::Hardware,
            "pinned" => self.active_view = ActiveView::Pinned,
            "all" => self.active_view = ActiveView::AllTools,
            "category" => {
                self.active_view = ActiveView::Category;
                self.active_category = entry.id.clone();
            }
            _ => return,
        }
        self.search_focused = false;
        self.selected_tool_idx = 0;
        self.scroll_list = 0.0;
        self.scroll_hw = 0.0;
        self.recompute_tools();
        self.prefetch_visible_icons();
        self.request_preview_for_selected();
        fx.repaint = true;
        fx.activity = true;
    }

    pub fn move_sidebar(&mut self, delta: i32, fx: &mut UiEffects) {
        let entries = self.build_sidebar_entries();
        let selectable: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| if !e.is_header { Some(i) } else { None })
            .collect();
        if selectable.is_empty() {
            return;
        }
        let cur = selectable
            .iter()
            .position(|&i| entries[i].selected)
            .unwrap_or(0);
        let next = (cur as i32 + delta).clamp(0, selectable.len() as i32 - 1) as usize;
        self.select_sidebar_entry(selectable[next], fx);
    }

    pub fn move_tool(&mut self, delta: i32, scale: f32, fx: &mut UiEffects) {
        if self.active_view == ActiveView::Hardware {
            self.scroll_hw = (self.scroll_hw + delta as f32 * 60.0 * scale).max(0.0);
            fx.repaint = true;
            fx.activity = true;
            return;
        }
        if self.cached_tools.is_empty() {
            return;
        }
        let max_idx = (self.cached_tools.len() - 1) as i32;
        let next = (self.selected_tool_idx as i32 + delta).clamp(0, max_idx) as usize;
        if next != self.selected_tool_idx {
            self.selected_tool_idx = next;
            self.ensure_selected_tool_visible(scale);
            self.request_preview_for_selected();
            fx.repaint = true;
            fx.activity = true;
        }
    }

    pub fn launch_tool_variant(
        &mut self,
        tool: &Tool,
        target_path: &Path,
        kind: &str,
        admin: bool,
        fx: &mut UiEffects,
    ) {
        fx.activity = true;
        if kind == "tui" || tool.kind == "tui" {
            self.screen_pattern_idx = 0;
            self.active_modal = ActiveModal::ScreenTest;
            self.was_fullscreen_before_test = self.fullscreen_active;
            self.show_screen_hud = true;
            fx.set_fullscreen = Some(true);
            self.cfg.add_recent(&tool.id);
            let _ = self.cfg.save(&self.config_dir);
            fx.repaint = true;
            return;
        }

        let debounce_key = format!("{}|{}", tool.id, target_path.display());
        if !self.tracker.allow_launch(&debounce_key, Duration::from_millis(500)) {
            self.set_toast("启动太频繁，已忽略重复点击", fx);
            return;
        }

        let res = if admin {
            launcher::launch_admin(&self.tracker, &tool.id, &tool.name, target_path, Some(&tool.dir))
        } else {
            launcher::launch_normal(
                &self.tracker,
                &tool.id,
                &tool.name,
                target_path,
                Some(&tool.dir),
                kind,
            )
        };

        match res {
            Ok(tp) => {
                self.cfg.add_recent(&tool.id);
                let _ = self.cfg.save(&self.config_dir);
                if admin {
                    self.set_toast(format!("已请求管理员权限启动: {}", tool.name), fx);
                } else if tp.pid > 0 {
                    self.set_toast(format!("已启动 {} (PID {})", tool.name, tp.pid), fx);
                } else {
                    self.set_toast(format!("已打开: {}", tool.name), fx);
                }
                fx.repaint = true;
            }
            Err(e) => {
                self.set_toast(format!("启动失败: {e}"), fx);
            }
        }
    }

    pub fn launch_selected(&mut self, admin: bool, fx: &mut UiEffects) {
        if let Some(tool) = self.selected_tool().cloned() {
            let ep = tool.entry_path.clone();
            let kind = tool.kind.clone();
            self.launch_tool_variant(&tool, &ep, &kind, admin, fx);
        }
    }

    pub fn open_tool_dir(&mut self, fx: &mut UiEffects) {
        if let Some(tool) = self.selected_tool().cloned() {
            if !tool.dir.as_os_str().is_empty() {
                let target = if !tool.entry_path.as_os_str().is_empty() {
                    &tool.entry_path
                } else {
                    &tool.dir
                };
                match launcher::open_in_explorer(target) {
                    Ok(()) => self.set_toast(format!("已在资源管理器定位: {}", tool.rel_dir), fx),
                    Err(e) => self.set_toast(format!("打开目录失败: {e}"), fx),
                }
            }
        }
    }

    pub fn toggle_pin(&mut self, fx: &mut UiEffects) {
        if let Some(tool) = self.selected_tool().cloned() {
            let now_pinned = self.cfg.toggle_pin(&tool.id);
            let _ = self.cfg.save(&self.config_dir);
            if now_pinned {
                self.set_toast(format!("已收藏: {}", tool.name), fx);
            } else {
                self.set_toast(format!("已移出收藏: {}", tool.name), fx);
            }
            if self.active_view == ActiveView::Pinned {
                self.recompute_tools();
                self.prefetch_visible_icons();
                self.request_preview_for_selected();
            }
            fx.repaint = true;
            fx.activity = true;
        }
    }

    pub fn copy_password(&mut self, fx: &mut UiEffects) {
        if let Some(tool) = self.selected_tool().cloned() {
            if !tool.password_note.is_empty() {
                let pw = extract_password(&tool.password_note);
                match launcher::copy_to_clipboard(&pw) {
                    Ok(()) => self.set_toast(format!("已复制解压密码: {pw}"), fx),
                    Err(e) => self.set_toast(format!("复制密码失败: {e}"), fx),
                }
            }
        }
    }

    pub fn set_search_query(&mut self, q: String, fx: &mut UiEffects) {
        if self.active_view == ActiveView::Hardware {
            self.active_view = ActiveView::AllTools;
        }
        self.search_query = q;
        self.selected_tool_idx = 0;
        self.scroll_list = 0.0;
        self.recompute_tools();
        self.prefetch_visible_icons();
        self.request_preview_for_selected();
        fx.repaint = true;
        fx.activity = true;
    }

    pub fn prepare_search(&mut self, fx: &mut UiEffects) {
        self.search_focused = true;
        if self.active_view == ActiveView::Hardware {
            self.active_view = ActiveView::AllTools;
            self.selected_tool_idx = 0;
            self.scroll_list = 0.0;
            self.recompute_tools();
            self.prefetch_visible_icons();
            self.request_preview_for_selected();
        }
        fx.repaint = true;
        fx.activity = true;
    }

    pub fn clear_search(&mut self, toast: bool, fx: &mut UiEffects) {
        self.search_query.clear();
        self.selected_tool_idx = 0;
        self.scroll_list = 0.0;
        self.recompute_tools();
        self.prefetch_visible_icons();
        self.request_preview_for_selected();
        if toast {
            self.set_toast("已清除搜索筛选", fx);
        } else {
            fx.repaint = true;
        }
        fx.activity = true;
    }

    pub fn cycle_sort(&mut self, fx: &mut UiEffects) {
        self.cfg.sort_mode = match self.cfg.sort_mode.as_str() {
            "default" => "name".to_string(),
            "name" => "type".to_string(),
            _ => "default".to_string(),
        };
        let _ = self.cfg.save(&self.config_dir);
        self.selected_tool_idx = 0;
        self.scroll_list = 0.0;
        self.recompute_tools();
        self.prefetch_visible_icons();
        self.request_preview_for_selected();
        let lbl = sort_label(&self.cfg.sort_mode);
        self.set_toast(format!("排序方式已切换为: {lbl}"), fx);
    }

    pub fn theme(&self) -> &'static ThemePalette {
        &THEME
    }

    pub fn reload_hwinfo(&mut self, fx: &mut UiEffects) {
        match parse_hwinfo(&self.data_root) {
            Ok(rep) => {
                self.hw = rep;
                self.set_toast("已重新加载静态硬件配置 hwinfo.ini", fx);
                fx.activity = true;
            }
            Err(e) => self.set_toast(format!("读取 hwinfo.ini 失败: {e}"), fx),
        }
    }

    pub fn launch_tulading(&mut self, fx: &mut UiEffects) {
        if let Some(tool) = self.cat.get("tulading_hw").cloned() {
            let ep = tool.entry_path.clone();
            let kind = tool.kind.clone();
            self.launch_tool_variant(&tool, &ep, &kind, false, fx);
        }
    }

    pub fn export_hwinfo(&mut self, fx: &mut UiEffects) {
        let txt = format_plain_text(&self.hw);
        let out_path = self.config_dir.join("硬件配置报告.txt");
        match fs::write(&out_path, txt.as_bytes()) {
            Ok(()) => self.set_toast(format!("已导出至 {}", out_path.display()), fx),
            Err(e) => self.set_toast(format!("导出失败: {e}"), fx),
        }
    }

    pub fn copy_hwinfo(&mut self, fx: &mut UiEffects) {
        let txt = format_plain_text(&self.hw);
        match launcher::copy_to_clipboard(&txt) {
            Ok(()) => self.set_toast("已复制硬件配置报告到剪贴板", fx),
            Err(e) => self.set_toast(format!("复制剪贴板失败: {e}"), fx),
        }
    }

    pub fn close_modal(&mut self, fx: &mut UiEffects) {
        let was_screen = self.active_modal == ActiveModal::ScreenTest;
        self.active_modal = ActiveModal::None;
        if was_screen {
            fx.set_fullscreen = Some(self.was_fullscreen_before_test);
            fx.trim_now = true;
        }
        fx.repaint = true;
        fx.activity = true;
    }

    pub fn execute_hit(&mut self, action: HitAction, is_dblclk: bool, fx: &mut UiEffects) {
        match action {
            HitAction::FocusSearch => self.prepare_search(fx),
            HitAction::ClearSearch => self.clear_search(true, fx),
            HitAction::TogglePreview => {
                self.cfg.show_preview = !self.cfg.show_preview;
                let _ = self.cfg.save(&self.config_dir);
                fx.repaint = true;
                fx.activity = true;
            }
            HitAction::OpenHelp => {
                self.active_modal = ActiveModal::Help;
                self.scroll_help = 0.0;
                fx.repaint = true;
                fx.activity = true;
            }
            HitAction::FocusPanel(p) => {
                self.search_focused = false;
                self.focus_panel = p;
                fx.repaint = true;
                fx.activity = true;
            }
            HitAction::SelectSidebar(i) => {
                self.focus_panel = FocusPanel::Sidebar;
                self.select_sidebar_entry(i, fx);
            }
            HitAction::SelectTool(i) => {
                self.search_focused = false;
                self.focus_panel = FocusPanel::Main;
                if i < self.cached_tools.len() {
                    if is_dblclk && i == self.selected_tool_idx {
                        self.launch_selected(false, fx);
                    } else {
                        self.selected_tool_idx = i;
                        self.request_preview_for_selected();
                        if is_dblclk {
                            self.launch_selected(false, fx);
                        } else {
                            fx.repaint = true;
                            fx.activity = true;
                        }
                    }
                }
            }
            HitAction::CycleSort => self.cycle_sort(fx),
            HitAction::ReloadHwInfo => self.reload_hwinfo(fx),
            HitAction::LaunchTulading => self.launch_tulading(fx),
            HitAction::ExportHwInfo => self.export_hwinfo(fx),
            HitAction::CopyHwInfo => self.copy_hwinfo(fx),
            HitAction::LaunchSelected(admin) => self.launch_selected(admin, fx),
            HitAction::OpenVariants => {
                if let Some(tool) = self.selected_tool() {
                    if !tool.variants.is_empty() {
                        self.selected_variant_idx = 0;
                        self.scroll_variants = 0.0;
                        self.active_modal = ActiveModal::Variants;
                        fx.repaint = true;
                        fx.activity = true;
                    }
                }
            }
            HitAction::OpenToolDir => self.open_tool_dir(fx),
            HitAction::TogglePin => self.toggle_pin(fx),
            HitAction::CopyPassword => self.copy_password(fx),
            HitAction::CloseModal | HitAction::ModalBackdrop => self.close_modal(fx),
            HitAction::SelectVariant(idx) => {
                self.selected_variant_idx = idx;
                if is_dblclk {
                    if let Some(tool) = self.selected_tool().cloned() {
                        if let Some(v) = tool.variants.get(idx) {
                            let p = PathBuf::from(&v.path);
                            let k = v.kind.clone();
                            self.active_modal = ActiveModal::None;
                            self.launch_tool_variant(&tool, &p, &k, false, fx);
                            return;
                        }
                    }
                }
                fx.repaint = true;
                fx.activity = true;
            }
            HitAction::LaunchVariant(idx, admin) => {
                self.active_modal = ActiveModal::None;
                if let Some(tool) = self.selected_tool().cloned() {
                    if let Some(v) = tool.variants.get(idx) {
                        let p = PathBuf::from(&v.path);
                        let k = v.kind.clone();
                        self.launch_tool_variant(&tool, &p, &k, admin, fx);
                    }
                }
                fx.repaint = true;
            }
            HitAction::ScreenStep(delta) => {
                self.screen_pattern_idx = (self.screen_pattern_idx + delta).rem_euclid(8);
                fx.repaint = true;
                fx.activity = true;
            }
            HitAction::ToggleScreenHud => {
                self.show_screen_hud = !self.show_screen_hud;
                fx.repaint = true;
                fx.activity = true;
            }
            HitAction::ScrollbarDrag(_) | HitAction::ModalInner => {}
        }
    }
}

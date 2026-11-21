#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod catalog;
mod config;
mod diag;
mod gdi;
mod hwinfo;
mod icons;
mod launcher;
mod ui_render;

use app_state::{
    ActiveModal, AppState, FocusPanel, HitAction, ScrollTarget, UiEffects, WM_APP_WORKER,
};
use config::Config;
use gdi::{BackBuffer, Canvas, FontCache, to_wide_null};
use hwinfo::load_initial_hardware;
use icons::synthesize_app_window_icon_rgba;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::sync::atomic::Ordering;

use windows_sys::Win32::Foundation::{FARPROC, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, CreateBitmap, CreateDIBSection,
    DIB_RGB_COLORS, DeleteObject, EndPaint, GetDC, GetMonitorInfoW, HDC, InvalidateRect,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, PAINTSTRUCT, ReleaseDC,
    ScreenToClient,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows_sys::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VK_BACK, VK_CONTROL, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR,
    VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, CreateIconIndirect, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyWindow, DispatchMessageW, GWL_EXSTYLE, GWL_STYLE, GetClientRect,
    GetMessageW, GetWindowLongW, GetWindowPlacement, HCURSOR, HICON, HTCLIENT,
    ICON_BIG, ICON_SMALL, ICONINFO, IDC_ARROW, IDC_HAND, KillTimer, LoadCursorW, MINMAXINFO, MSG,
    PostMessageW, PostQuitMessage, RegisterClassExW, SIZE_MINIMIZED, SPI_GETWORKAREA, SW_SHOW,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW,
    SetCursor, SetTimer, SetWindowLongW, SetWindowPlacement, SetWindowPos, ShowWindow,
    SystemParametersInfoW, TranslateMessage, WINDOWPLACEMENT, WM_CLOSE, WM_CHAR, WM_DESTROY,
    WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_PRINTCLIENT, WM_SETCURSOR, WM_SETICON,
    WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
};

const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_TRIM_ID: usize = 1;
const TIMER_TOAST_ID: usize = 2;

struct DragScroll {
    target: ScrollTarget,
    start_y: i32,
    start_offset: f32,
    travel_px: i32,
    max_scroll: f32,
}

#[derive(Default)]
struct DeferredWinOps {
    result: Option<LRESULT>,
    set_focus: bool,
    set_capture: bool,
    release_capture: bool,
    dpi_rect: Option<RECT>,
    fullscreen_enter: Option<(i32, RECT)>,
    fullscreen_exit: Option<(i32, WINDOWPLACEMENT)>,
    arm_idle_trim_ms: Option<u32>,
    sync_ime: Option<bool>,
    drop_host: bool,
}

struct WinHost {
    hwnd: HWND,
    dpi: u32,
    scale: f32,
    client_w: i32,
    client_h: i32,
    backbuffer: BackBuffer,
    fonts: FontCache,
    state: AppState,
    icon_big: HICON,
    icon_small: HICON,
    cursor_arrow: HCURSOR,
    cursor_hand: HCURSOR,
    pointer_cursor: bool,
    tracking_mouse: bool,
    pending_high_surrogate: u16,
    drag_scroll: Option<DragScroll>,
    trim_timer_armed: bool,
    ime_enabled: bool,
    saved_placement: WINDOWPLACEMENT,
    saved_style: i32,
}

thread_local! {
    static WIN_HOST: RefCell<Option<WinHost>> = const { RefCell::new(None) };
}

type ImmAssociateContextExFn =
    unsafe extern "system" fn(hwnd: HWND, himc: *mut core::ffi::c_void, flags: u32) -> i32;
const IACE_DEFAULT: u32 = 0x0010;

/// Detaches or re-attaches Windows IME on the main HWND so CTF/D3D/DWrite stay unloaded unless searching.
fn set_window_ime_enabled(hwnd: HWND, enabled: bool) {
    if hwnd.is_null() {
        return;
    }
    static FN: OnceLock<Option<ImmAssociateContextExFn>> = OnceLock::new();
    let func = *FN.get_or_init(|| unsafe {
        let dll = to_wide_null("imm32.dll");
        let mut hmod = GetModuleHandleW(dll.as_ptr());
        if hmod.is_null() {
            hmod = LoadLibraryW(dll.as_ptr());
        }
        if hmod.is_null() {
            return None;
        }
        let proc_ptr: FARPROC =
            GetProcAddress(hmod, c"ImmAssociateContextEx".as_ptr() as *const u8);
        proc_ptr.map(|f| core::mem::transmute_copy(&f))
    });
    if let Some(f) = func {
        unsafe {
            let _ = f(hwnd, null_mut(), if enabled { IACE_DEFAULT } else { 0 });
        }
    }
}

fn trim_working_set() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Memory::{GetProcessHeap, HeapCompact};
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
        let heap = GetProcessHeap();
        if !heap.is_null() {
            let _ = HeapCompact(heap, 0);
        }
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

fn resolve_roots() -> (PathBuf, PathBuf) {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let cwd = std::env::current_dir().ok();

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(ed) = &exe_dir {
        candidates.push(ed.clone());
        if let Some(p1) = ed.parent() {
            candidates.push(p1.to_path_buf());
            candidates.push(p1.join("图吧工具箱202207"));
            if let Some(p2) = p1.parent() {
                candidates.push(p2.to_path_buf());
                candidates.push(p2.join("图吧工具箱202207"));
            }
        }
    }
    if let Some(cd) = &cwd {
        candidates.push(cd.clone());
        if let Some(p1) = cd.parent() {
            candidates.push(p1.to_path_buf());
            candidates.push(p1.join("图吧工具箱202207"));
        }
    }

    let mut data_root = cwd.unwrap_or_else(|| PathBuf::from("."));
    for c in candidates {
        if c.join("tools").is_dir() {
            data_root = c;
            break;
        }
    }

    let config_dir = match &exe_dir {
        Some(ed) => {
            let s = ed.to_string_lossy().to_lowercase();
            if s.contains("\\target\\") || s.ends_with("\\target") {
                data_root.clone()
            } else {
                ed.clone()
            }
        }
        None => data_root.clone(),
    };

    (data_root, config_dir)
}

fn create_hicon_from_rgba(w: u32, h: u32, rgba: &[u8]) -> HICON {
    if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
        return null_mut();
    }
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: core::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [Default::default()],
    };
    unsafe {
        let mut bits_ptr: *mut core::ffi::c_void = null_mut();
        let color_bmp =
            CreateDIBSection(null_mut(), &bmi, DIB_RGB_COLORS, &mut bits_ptr, null_mut(), 0);
        if color_bmp.is_null() || bits_ptr.is_null() {
            if !color_bmp.is_null() {
                DeleteObject(color_bmp);
            }
            return null_mut();
        }
        let dst = core::slice::from_raw_parts_mut(bits_ptr as *mut u8, (w * h * 4) as usize);
        for (d, s) in dst.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
        let mask_bmp = CreateBitmap(w as i32, h as i32, 1, 1, null());
        let ii = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bmp,
            hbmColor: color_bmp,
        };
        let hicon = CreateIconIndirect(&ii);
        if !mask_bmp.is_null() {
            DeleteObject(mask_bmp);
        }
        DeleteObject(color_bmp);
        hicon
    }
}

#[inline]
fn lparam_xy(lparam: LPARAM) -> (i32, i32) {
    let x = (lparam & 0xFFFF) as i16 as i32;
    let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
    (x, y)
}

impl WinHost {
    fn render_frame(&mut self, ref_hdc: HDC) {
        let w = self.client_w.max(1);
        let h = self.client_h.max(1);
        if !self.backbuffer.ensure(ref_hdc, w, h) {
            return;
        }
        self.fonts.ensure_dpi(self.dpi);
        let mut canvas = Canvas::new(&mut self.backbuffer, &self.fonts, self.dpi);
        ui_render::render_app(&mut canvas, &mut self.state, w, h);
    }

    fn apply_effects(&mut self, fx: UiEffects, ops: &mut DeferredWinOps) {
        if let Some(fs) = fx.set_fullscreen {
            self.prepare_fullscreen(fs, ops);
        }
        if fx.activity && !self.state.initial_trim_pending && !fx.trim_now {
            self.trim_timer_armed = true;
            ops.arm_idle_trim_ms = Some(5000);
        }
        if fx.toast_timer {
            unsafe {
                SetTimer(self.hwnd, TIMER_TOAST_ID, 4000, None);
            }
        }
        if fx.trim_now {
            self.trim_timer_armed = false;
            self.backbuffer.release();
            self.state.preview_cache.clear();
            trim_working_set();
        }
        if fx.repaint {
            unsafe {
                InvalidateRect(self.hwnd, null(), 0);
            }
        }
        if fx.quit {
            unsafe {
                PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
            }
        }
        if self.state.search_focused != self.ime_enabled {
            self.ime_enabled = self.state.search_focused;
            ops.sync_ime = Some(self.ime_enabled);
        }
    }

    fn prepare_fullscreen(&mut self, fullscreen: bool, ops: &mut DeferredWinOps) {
        if self.state.fullscreen_active == fullscreen || self.hwnd.is_null() {
            return;
        }
        self.state.fullscreen_active = fullscreen;
        unsafe {
            if fullscreen {
                let mut wp = WINDOWPLACEMENT {
                    length: core::mem::size_of::<WINDOWPLACEMENT>() as u32,
                    ..Default::default()
                };
                GetWindowPlacement(self.hwnd, &mut wp);
                self.saved_placement = wp;
                let style = GetWindowLongW(self.hwnd, GWL_STYLE);
                self.saved_style = style;
                let hmon = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO {
                    cbSize: core::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if GetMonitorInfoW(hmon, &mut mi) != 0 {
                    let new_style = (style & !(WS_OVERLAPPEDWINDOW as i32)) | (WS_POPUP as i32);
                    ops.fullscreen_enter = Some((new_style, mi.rcMonitor));
                }
            } else {
                ops.fullscreen_exit = Some((self.saved_style | (WS_VISIBLE as i32), self.saved_placement));
            }
        }
    }

    fn hit_test(&self, px: i32, py: i32) -> Option<(HitAction, bool)> {
        self.state
            .last_frame
            .hits
            .iter()
            .rev()
            .find(|h| h.rect.contains(px, py))
            .map(|h| (h.action.clone(), h.pointer_cursor))
    }

    fn handle_mouse_move(&mut self, px: i32, py: i32) {
        if let Some(ref drag) = self.drag_scroll {
            let dy = py - drag.start_y;
            let delta = if drag.travel_px > 0 {
                (dy as f32 / drag.travel_px as f32) * drag.max_scroll
            } else {
                0.0
            };
            let target = drag.target;
            let next = (drag.start_offset + delta).clamp(0.0, drag.max_scroll);
            *self.state.scroll_offset_mut(target) = next;
            unsafe {
                InvalidateRect(self.hwnd, null(), 0);
            }
            return;
        }

        let (next_hov, hand) = match self.hit_test(px, py) {
            Some((act, ptr)) => (Some(act), ptr),
            None => (None, false),
        };
        if hand != self.pointer_cursor {
            self.pointer_cursor = hand;
            unsafe {
                SetCursor(if hand {
                    self.cursor_hand
                } else {
                    self.cursor_arrow
                });
            }
        }
        if next_hov != self.state.hovered_action {
            self.state.hovered_action = next_hov;
            unsafe {
                InvalidateRect(self.hwnd, null(), 0);
            }
        }
    }

    fn handle_mouse_down(
        &mut self,
        px: i32,
        py: i32,
        is_dblclk: bool,
        ops: &mut DeferredWinOps,
    ) {
        ops.set_focus = true;
        if let Some((act, _)) = self.hit_test(px, py) {
            if let HitAction::ScrollbarDrag(target) = act {
                if let Some(st) = self
                    .state
                    .last_frame
                    .scrolls
                    .iter()
                    .find(|s| s.target == target)
                    .copied()
                {
                    let max_scroll = (st.content_h - st.viewport.h).max(1) as f32;
                    let pad = (2.0 * self.scale).round() as i32;
                    let travel = ((st.track.h - pad * 2) - st.thumb.h).max(1);
                    if st.thumb.contains(px, py) {
                        let start_offset = *self.state.scroll_offset_mut(target);
                        self.drag_scroll = Some(DragScroll {
                            target,
                            start_y: py,
                            start_offset,
                            travel_px: travel,
                            max_scroll,
                        });
                        ops.set_capture = true;
                    } else {
                        let ratio = ((py - (st.track.y + pad) - st.thumb.h / 2) as f32
                            / travel as f32)
                            .clamp(0.0, 1.0);
                        *self.state.scroll_offset_mut(target) = ratio * max_scroll;
                        unsafe {
                            InvalidateRect(self.hwnd, null(), 0);
                        }
                    }
                    return;
                }
            }

            let mut fx = UiEffects::default();
            self.state.execute_hit(act, is_dblclk, &mut fx);
            self.apply_effects(fx, ops);
        }
    }

    fn handle_mouse_wheel(&mut self, px: i32, py: i32, delta_y: f32, ops: &mut DeferredWinOps) {
        let mut target_scroll = None;
        for st in self.state.last_frame.scrolls.iter().rev() {
            if st.viewport.contains(px, py) {
                target_scroll = Some(*st);
                break;
            }
        }
        if let Some(st) = target_scroll {
            let max_s = (st.content_h - st.viewport.h).max(0) as f32;
            let off = self.state.scroll_offset_mut(st.target);
            *off = (*off - delta_y * self.scale).clamp(0.0, max_s);
            let mut fx = UiEffects {
                repaint: true,
                activity: true,
                ..Default::default()
            };
            if self.state.initial_trim_pending {
                fx.activity = false;
            }
            self.apply_effects(fx, ops);
        }
    }

    fn handle_key_down(&mut self, vk: u16, ops: &mut DeferredWinOps) {
        let mut fx = UiEffects::default();
        let ctrl_down = unsafe { GetKeyState(VK_CONTROL as i32) < 0 };

        if ctrl_down && vk == b'F' as u16 {
            self.state.active_modal = ActiveModal::None;
            self.state.prepare_search(&mut fx);
            self.apply_effects(fx, ops);
            return;
        }

        match self.state.active_modal {
            ActiveModal::Help => {
                match vk {
                    x if x == VK_ESCAPE => self.state.close_modal(&mut fx),
                    x if x == VK_DOWN => {
                        self.state.scroll_help += 36.0 * self.scale;
                        fx.repaint = true;
                    }
                    x if x == VK_UP => {
                        self.state.scroll_help = (self.state.scroll_help - 36.0 * self.scale).max(0.0);
                        fx.repaint = true;
                    }
                    x if x == VK_NEXT => {
                        self.state.scroll_help += 180.0 * self.scale;
                        fx.repaint = true;
                    }
                    x if x == VK_PRIOR => {
                        self.state.scroll_help = (self.state.scroll_help - 180.0 * self.scale).max(0.0);
                        fx.repaint = true;
                    }
                    _ => {}
                }
                self.apply_effects(fx, ops);
                return;
            }
            ActiveModal::Variants => {
                let len = self
                    .state
                    .selected_tool()
                    .map(|t| t.variants.len())
                    .unwrap_or(0);
                match vk {
                    x if x == VK_ESCAPE => self.state.close_modal(&mut fx),
                    x if x == VK_DOWN && len > 0 => {
                        self.state.selected_variant_idx =
                            (self.state.selected_variant_idx + 1).min(len - 1);
                        fx.repaint = true;
                    }
                    x if x == VK_UP && len > 0 => {
                        self.state.selected_variant_idx =
                            self.state.selected_variant_idx.saturating_sub(1);
                        fx.repaint = true;
                    }
                    x if x == VK_RETURN => {
                        let idx = self.state.selected_variant_idx;
                        self.state
                            .execute_hit(HitAction::LaunchVariant(idx, false), false, &mut fx);
                    }
                    _ => {}
                }
                self.apply_effects(fx, ops);
                return;
            }
            ActiveModal::ScreenTest => {
                match vk {
                    x if x == VK_ESCAPE => self.state.close_modal(&mut fx),
                    x if x == VK_LEFT || x == VK_UP => {
                        self.state.screen_pattern_idx =
                            (self.state.screen_pattern_idx - 1).rem_euclid(8);
                        fx.repaint = true;
                    }
                    x if x == VK_RIGHT || x == VK_DOWN || x == VK_SPACE || x == VK_RETURN => {
                        self.state.screen_pattern_idx =
                            (self.state.screen_pattern_idx + 1).rem_euclid(8);
                        fx.repaint = true;
                    }
                    _ => {}
                }
                self.apply_effects(fx, ops);
                return;
            }
            ActiveModal::None => {}
        }

        if self.state.search_focused {
            match vk {
                x if x == VK_ESCAPE => {
                    self.state.search_focused = false;
                    self.state.clear_search(false, &mut fx);
                }
                x if x == VK_RETURN => {
                    self.state.search_focused = false;
                    self.state.focus_panel = FocusPanel::Main;
                    self.state.launch_selected(false, &mut fx);
                }
                x if x == VK_TAB => {
                    self.state.search_focused = false;
                    self.state.focus_panel = FocusPanel::Main;
                    fx.repaint = true;
                }
                x if x == VK_DOWN => self.state.move_tool(1, self.scale, &mut fx),
                x if x == VK_UP => self.state.move_tool(-1, self.scale, &mut fx),
                x if x == VK_BACK => {
                    let mut q = self.state.search_query.clone();
                    if q.pop().is_some() {
                        self.state.set_search_query(q, &mut fx);
                    }
                }
                _ => {}
            }
            self.apply_effects(fx, ops);
            return;
        }

        // Normal mode virtual-key navigation
        match vk {
            x if x == VK_ESCAPE => {
                if !self.state.search_query.is_empty() {
                    self.state.clear_search(true, &mut fx);
                }
            }
            x if x == VK_TAB => {
                self.state.focus_panel = match self.state.focus_panel {
                    FocusPanel::Sidebar => FocusPanel::Main,
                    FocusPanel::Main => FocusPanel::Sidebar,
                };
                fx.repaint = true;
                fx.activity = true;
            }
            x if x == VK_DOWN => match self.state.focus_panel {
                FocusPanel::Sidebar => self.state.move_sidebar(1, &mut fx),
                FocusPanel::Main => self.state.move_tool(1, self.scale, &mut fx),
            },
            x if x == VK_UP => match self.state.focus_panel {
                FocusPanel::Sidebar => self.state.move_sidebar(-1, &mut fx),
                FocusPanel::Main => self.state.move_tool(-1, self.scale, &mut fx),
            },
            x if x == VK_NEXT => {
                if self.state.focus_panel == FocusPanel::Main {
                    self.state.move_tool(8, self.scale, &mut fx);
                }
            }
            x if x == VK_PRIOR => {
                if self.state.focus_panel == FocusPanel::Main {
                    self.state.move_tool(-8, self.scale, &mut fx);
                }
            }
            x if x == VK_HOME => {
                if self.state.focus_panel == FocusPanel::Main {
                    self.state.move_tool(-9999, self.scale, &mut fx);
                }
            }
            x if x == VK_END => {
                if self.state.focus_panel == FocusPanel::Main {
                    self.state.move_tool(9999, self.scale, &mut fx);
                }
            }
            x if x == VK_LEFT => {
                self.state.focus_panel = FocusPanel::Sidebar;
                fx.repaint = true;
            }
            x if x == VK_RIGHT => {
                self.state.focus_panel = FocusPanel::Main;
                fx.repaint = true;
            }
            x if x == VK_RETURN => match self.state.focus_panel {
                FocusPanel::Sidebar => {
                    self.state.focus_panel = FocusPanel::Main;
                    fx.repaint = true;
                }
                FocusPanel::Main => self.state.launch_selected(false, &mut fx),
            },
            _ => {}
        }
        self.apply_effects(fx, ops);
    }

    fn handle_char(&mut self, c: char, ops: &mut DeferredWinOps) {
        let mut fx = UiEffects::default();

        match self.state.active_modal {
            ActiveModal::Help => {
                match c {
                    'q' => self.state.close_modal(&mut fx),
                    'j' => {
                        self.state.scroll_help += 36.0 * self.scale;
                        fx.repaint = true;
                    }
                    'k' => {
                        self.state.scroll_help = (self.state.scroll_help - 36.0 * self.scale).max(0.0);
                        fx.repaint = true;
                    }
                    _ => {}
                }
                self.apply_effects(fx, ops);
                return;
            }
            ActiveModal::Variants => {
                let len = self
                    .state
                    .selected_tool()
                    .map(|t| t.variants.len())
                    .unwrap_or(0);
                match c {
                    'q' => self.state.close_modal(&mut fx),
                    'j' if len > 0 => {
                        self.state.selected_variant_idx =
                            (self.state.selected_variant_idx + 1).min(len - 1);
                        fx.repaint = true;
                    }
                    'k' if len > 0 => {
                        self.state.selected_variant_idx =
                            self.state.selected_variant_idx.saturating_sub(1);
                        fx.repaint = true;
                    }
                    'a' => {
                        let idx = self.state.selected_variant_idx;
                        self.state
                            .execute_hit(HitAction::LaunchVariant(idx, true), false, &mut fx);
                    }
                    _ => {}
                }
                self.apply_effects(fx, ops);
                return;
            }
            ActiveModal::ScreenTest => {
                match c {
                    'q' => self.state.close_modal(&mut fx),
                    'h' => {
                        self.state.show_screen_hud = !self.state.show_screen_hud;
                        fx.repaint = true;
                    }
                    _ => {}
                }
                self.apply_effects(fx, ops);
                return;
            }
            ActiveModal::None => {}
        }

        if self.state.search_focused {
            let mut q = self.state.search_query.clone();
            q.push(c);
            self.state.set_search_query(q, &mut fx);
            self.apply_effects(fx, ops);
            return;
        }

        match c {
            '/' => self.state.prepare_search(&mut fx),
            '?' => {
                self.state.active_modal = ActiveModal::Help;
                self.state.scroll_help = 0.0;
                fx.repaint = true;
            }
            'q' => fx.quit = true,
            'P' => self
                .state
                .execute_hit(HitAction::TogglePreview, false, &mut fx),
            's' => self.state.cycle_sort(&mut fx),
            'j' => match self.state.focus_panel {
                FocusPanel::Sidebar => self.state.move_sidebar(1, &mut fx),
                FocusPanel::Main => self.state.move_tool(1, self.scale, &mut fx),
            },
            'k' => match self.state.focus_panel {
                FocusPanel::Sidebar => self.state.move_sidebar(-1, &mut fx),
                FocusPanel::Main => self.state.move_tool(-1, self.scale, &mut fx),
            },
            'h' => {
                self.state.focus_panel = FocusPanel::Sidebar;
                fx.repaint = true;
            }
            'l' => {
                self.state.focus_panel = FocusPanel::Main;
                fx.repaint = true;
            }
            'a' => self.state.launch_selected(true, &mut fx),
            'b' => self
                .state
                .execute_hit(HitAction::OpenVariants, false, &mut fx),
            'o' => self.state.open_tool_dir(&mut fx),
            'p' => self.state.toggle_pin(&mut fx),
            'w' => self.state.copy_password(&mut fx),
            'c' if self.state.active_view == app_state::ActiveView::Hardware => {
                self.state.copy_hwinfo(&mut fx);
            }
            'r' if self.state.active_view == app_state::ActiveView::Hardware => {
                self.state.reload_hwinfo(&mut fx)
            }
            'd' if self.state.active_view == app_state::ActiveView::Hardware => {
                self.state.launch_tulading(&mut fx)
            }
            'e' if self.state.active_view == app_state::ActiveView::Hardware => {
                self.state.export_hwinfo(&mut fx)
            }
            _ => {}
        }
        self.apply_effects(fx, ops);
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Handle messages that must not hold any RefCell borrow across GDI/Window APIs.
    if msg == WM_ERASEBKGND {
        return 1;
    }
    if msg == WM_CLOSE {
        DestroyWindow(hwnd);
        return 0;
    }
    if msg == WM_PAINT {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut arm_idle_trim = false;
        WIN_HOST.with(|cell| {
            if let Ok(mut borrow) = cell.try_borrow_mut() {
                if let Some(host) = borrow.as_mut() {
                    if host.hwnd == hwnd {
                        host.render_frame(hdc);
                        host.backbuffer.blit_to(hdc);
                        if !host.trim_timer_armed && !host.state.initial_trim_pending {
                            host.trim_timer_armed = true;
                            arm_idle_trim = true;
                        }
                    }
                }
            }
        });
        EndPaint(hwnd, &mut ps);
        if arm_idle_trim {
            SetTimer(hwnd, TIMER_TRIM_ID, 1200, None);
        }
        return 0;
    }

    let mut ops = DeferredWinOps::default();
    WIN_HOST.with(|cell| {
        let Ok(mut borrow) = cell.try_borrow_mut() else {
            return;
        };
        let Some(host) = borrow.as_mut() else {
            return;
        };
        if host.hwnd != hwnd {
            return;
        }
        match msg {
            WM_PRINTCLIENT => {
                let hdc = wparam as HDC;
                if !hdc.is_null() {
                    let was_allocated = !host.backbuffer.hdc.is_null();
                    host.render_frame(hdc);
                    host.backbuffer.blit_to(hdc);
                    if !was_allocated {
                        host.backbuffer.release();
                        trim_working_set();
                    }
                }
                ops.result = Some(0);
            }
            WM_SIZE => {
                if (wparam as u32) != SIZE_MINIMIZED {
                    let (w, h) = lparam_xy(lparam);
                    host.client_w = w.max(1);
                    host.client_h = h.max(1);
                    InvalidateRect(hwnd, null(), 0);
                }
                ops.result = Some(0);
            }
            WM_DPICHANGED => {
                let new_dpi = (wparam & 0xFFFF).max(96) as u32;
                host.dpi = new_dpi;
                host.scale = new_dpi as f32 / 96.0;
                if lparam != 0 {
                    ops.dpi_rect = Some(*(lparam as *const RECT));
                }
                InvalidateRect(hwnd, null(), 0);
                ops.result = Some(0);
            }
            WM_GETMINMAXINFO => {
                if lparam != 0 && !host.state.fullscreen_active {
                    let mmi = &mut *(lparam as *mut MINMAXINFO);
                    let sf = host.scale;
                    let dpi = GetDpiForWindow(hwnd).max(96);
                    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
                    let mut rc = RECT {
                        left: 0,
                        top: 0,
                        right: (920.0 * sf).ceil() as i32,
                        bottom: (600.0 * sf).ceil() as i32,
                    };
                    AdjustWindowRectExForDpi(&mut rc, style, 0, ex_style, dpi);
                    mmi.ptMinTrackSize.x = rc.right - rc.left;
                    mmi.ptMinTrackSize.y = rc.bottom - rc.top;
                }
                ops.result = Some(0);
            }
            WM_SETCURSOR => {
                if (lparam & 0xFFFF) as u32 == HTCLIENT {
                    SetCursor(if host.pointer_cursor {
                        host.cursor_hand
                    } else {
                        host.cursor_arrow
                    });
                    ops.result = Some(1);
                }
            }
            WM_MOUSEMOVE => {
                if !host.tracking_mouse {
                    host.tracking_mouse = true;
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: core::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut tme);
                }
                let (x, y) = lparam_xy(lparam);
                host.handle_mouse_move(x, y);
                ops.result = Some(0);
            }
            WM_MOUSELEAVE => {
                host.tracking_mouse = false;
                if host.state.hovered_action.take().is_some() {
                    InvalidateRect(hwnd, null(), 0);
                }
                ops.result = Some(0);
            }
            WM_LBUTTONDOWN => {
                let (x, y) = lparam_xy(lparam);
                host.handle_mouse_down(x, y, false, &mut ops);
                ops.result = Some(0);
            }
            WM_LBUTTONDBLCLK => {
                let (x, y) = lparam_xy(lparam);
                host.handle_mouse_down(x, y, true, &mut ops);
                ops.result = Some(0);
            }
            WM_LBUTTONUP => {
                if host.drag_scroll.take().is_some() {
                    ops.release_capture = true;
                }
                ops.result = Some(0);
            }
            WM_MOUSEWHEEL => {
                let (sx, sy) = lparam_xy(lparam);
                let mut pt = POINT { x: sx, y: sy };
                ScreenToClient(hwnd, &mut pt);
                let wheel = ((wparam >> 16) & 0xFFFF) as i16 as f32;
                let delta_y = (wheel / 120.0) * 48.0;
                host.handle_mouse_wheel(pt.x, pt.y, delta_y, &mut ops);
                ops.result = Some(0);
            }
            WM_KEYDOWN => {
                host.handle_key_down((wparam & 0xFFFF) as u16, &mut ops);
                ops.result = Some(0);
            }
            WM_CHAR => {
                let code = (wparam & 0xFFFF) as u16;
                if code >= 0x20 && code != 0x7F {
                    let ch = if (0xD800..=0xDBFF).contains(&code) {
                        host.pending_high_surrogate = code;
                        None
                    } else if (0xDC00..=0xDFFF).contains(&code) {
                        let hi = host.pending_high_surrogate;
                        host.pending_high_surrogate = 0;
                        if hi != 0 {
                            let cp =
                                0x10000 + (((hi as u32 - 0xD800) << 10) | (code as u32 - 0xDC00));
                            char::from_u32(cp)
                        } else {
                            None
                        }
                    } else {
                        host.pending_high_surrogate = 0;
                        char::from_u32(code as u32)
                    };
                    if let Some(c) = ch {
                        host.handle_char(c, &mut ops);
                    }
                }
                ops.result = Some(0);
            }
            WM_APP_WORKER => {
                let mut fx = UiEffects::default();
                host.state.drain_worker(&mut fx);
                host.apply_effects(fx, &mut ops);
                ops.result = Some(0);
            }
            WM_TIMER => {
                match wparam {
                    TIMER_TRIM_ID => {
                        KillTimer(hwnd, TIMER_TRIM_ID);
                        host.trim_timer_armed = false;
                        host.state.initial_trim_pending = false;
                        host.backbuffer.release();
                        host.state.preview_cache.clear();
                        trim_working_set();
                    }
                    TIMER_TOAST_ID => {
                        KillTimer(hwnd, TIMER_TOAST_ID);
                        if !host.state.toast_msg.is_empty() {
                            host.state.toast_msg.clear();
                            InvalidateRect(hwnd, null(), 0);
                        }
                    }
                    _ => {}
                }
                ops.result = Some(0);
            }
            WM_DESTROY => {
                KillTimer(hwnd, TIMER_TRIM_ID);
                KillTimer(hwnd, TIMER_TOAST_ID);
                host.state.hwnd_raw.store(0, Ordering::Release);
                host.backbuffer.release();
                host.fonts.clear();
                if !host.icon_big.is_null() {
                    DestroyIcon(host.icon_big);
                    host.icon_big = null_mut();
                }
                if !host.icon_small.is_null() {
                    DestroyIcon(host.icon_small);
                    host.icon_small = null_mut();
                }
                PostQuitMessage(0);
                ops.drop_host = true;
                ops.result = Some(0);
            }
            _ => {}
        }
    });

    if ops.set_focus {
        SetFocus(hwnd);
    }
    if ops.set_capture {
        SetCapture(hwnd);
    }
    if ops.release_capture {
        ReleaseCapture();
    }
    if let Some(rc) = ops.dpi_rect {
        SetWindowPos(
            hwnd,
            null_mut(),
            rc.left,
            rc.top,
            rc.right - rc.left,
            rc.bottom - rc.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    if let Some((new_style, mr)) = ops.fullscreen_enter {
        SetWindowLongW(hwnd, GWL_STYLE, new_style);
        SetWindowPos(
            hwnd,
            null_mut(),
            mr.left,
            mr.top,
            mr.right - mr.left,
            mr.bottom - mr.top,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    if let Some((restore_style, wp)) = ops.fullscreen_exit {
        SetWindowLongW(hwnd, GWL_STYLE, restore_style);
        SetWindowPlacement(hwnd, &wp);
        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    if let Some(ms) = ops.arm_idle_trim_ms {
        SetTimer(hwnd, TIMER_TRIM_ID, ms, None);
    }
    if let Some(enable) = ops.sync_ime {
        set_window_ime_enabled(hwnd, enable);
    }
    if ops.drop_host {
        WIN_HOST.with(|cell| {
            if let Ok(mut b) = cell.try_borrow_mut() {
                *b = None;
            }
        });
    }

    ops.result
        .unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
}

fn attach_parent_console_if_needed() {
    #[cfg(not(debug_assertions))]
    unsafe {
        use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check_flag = args.iter().any(|a| a == "--check");
    let rebuild_static_flag = args.iter().any(|a| a == "--rebuild-static");
    let debug_flag = args.iter().any(|a| a == "--debug");
    let help_flag = args.iter().any(|a| a == "--help" || a == "-h");

    if check_flag || rebuild_static_flag || help_flag {
        attach_parent_console_if_needed();
    }

    if help_flag {
        println!(
            "Trust工具箱 v{} (Native Win32 GDI)\n\
             Usage:\n  Trust工具箱.exe [--check] [--rebuild-static] [--debug] [--help]",
            env!("CARGO_PKG_VERSION")
        );
        return;
    }

    let (data_root, config_dir) = resolve_roots();
    let env_snapshot = format!(
        "DataRoot={}\nConfigDir={}\nArch={}",
        data_root.display(),
        config_dir.display(),
        std::env::consts::ARCH
    );
    diag::init(&config_dir, debug_flag, env_snapshot);
    diag::install_panic_hook();

    let static_path = data_root.join(catalog::STATIC_CATALOG_FILE_NAME);
    let cat_res = if rebuild_static_flag || !static_path.is_file() {
        let res = catalog::scan_and_build(&data_root);
        if let Ok(ref c) = res {
            let _ = catalog::save_static_file(&data_root, c);
        }
        res
    } else {
        catalog::load(&data_root)
    };

    let cat = match cat_res {
        Ok(c) => c,
        Err(e) => {
            diag::write_crash_report("catalog::load", &e.to_string(), "");
            eprintln!("加载工具目录失败: {e}");
            diag::close();
            std::process::exit(1);
        }
    };

    let cfg = Config::load(&config_dir);
    let hw = load_initial_hardware(&data_root);

    if check_flag {
        println!("DataRoot: {}", data_root.display());
        println!("ConfigDir: {}", config_dir.display());
        println!("StaticCatalog: {}", static_path.display());
        println!("Categories: {}, Tools: {}", cat.categories.len(), cat.total_tools());
        for c in &cat.categories {
            println!("  - {:<10} : {:>2} tools", c.name, c.tools.len());
        }
        println!("OS: {} ({})", hw.os_name, hw.os_build);
        println!("CPU: {} ({}C/{}T)", hw.cpu_model, hw.cpu_cores, hw.cpu_threads);
        if let Some(gpu) = hw.gpus.first() {
            println!("GPU: {} ({})", gpu.model, gpu.vram);
        }
        diag::close();
        return;
    }

    diag::infof("starting native Win32 GDI desktop GUI");

    unsafe {
        let _ = SetProcessDpiAwarenessContext(-4isize as _);
        let hinst = GetModuleHandleW(null());
        let class_name = to_wide_null("TrustToolboxWin32Gdi");
        let cursor_arrow = LoadCursorW(null_mut(), IDC_ARROW);
        let cursor_hand = LoadCursorW(null_mut(), IDC_HAND);

        let wc = WNDCLASSEXW {
            cbSize: core::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: null_mut(),
            hCursor: cursor_arrow,
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: null_mut(),
        };
        RegisterClassExW(&wc);

        let title = to_wide_null("Trust工具箱");
        let style = WS_OVERLAPPEDWINDOW;
        let ex_style = WS_EX_APPWINDOW;
        let hwnd = CreateWindowExW(
            ex_style,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            windows_sys::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT,
            windows_sys::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT,
            1200,
            720,
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        if hwnd.is_null() {
            diag::write_crash_report("CreateWindowExW", "failed to create main window", "");
            diag::close();
            std::process::exit(1);
        }
        set_window_ime_enabled(hwnd, false);

        let dpi = GetDpiForWindow(hwnd).max(96);
        let sf = dpi as f32 / 96.0;
        let mut wa = RECT::default();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut RECT as *mut _, 0);
        let max_cw = ((wa.right - wa.left) - 24).max(640);
        let max_ch = ((wa.bottom - wa.top) - 48).max(480);
        let target_cw = (1200.0 * sf).round() as i32;
        let target_ch = (720.0 * sf).round() as i32;
        let mut wr = RECT {
            left: 0,
            top: 0,
            right: target_cw.min(max_cw),
            bottom: target_ch.min(max_ch),
        };
        AdjustWindowRectExForDpi(&mut wr, style, 0, ex_style, dpi);
        let win_w = wr.right - wr.left;
        let win_h = wr.bottom - wr.top;
        let pos_x = wa.left + ((wa.right - wa.left) - win_w).max(0) / 2;
        let pos_y = wa.top + ((wa.bottom - wa.top) - win_h).max(0) / 2;
        SetWindowPos(
            hwnd,
            null_mut(),
            pos_x,
            pos_y,
            win_w,
            win_h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );

        let mut cr = RECT::default();
        GetClientRect(hwnd, &mut cr);
        let cw = (cr.right - cr.left).max(1);
        let ch = (cr.bottom - cr.top).max(1);

        let (iw, ih, irgba) = synthesize_app_window_icon_rgba();
        let icon_big = create_hicon_from_rgba(iw, ih, &irgba);
        let icon_small = create_hicon_from_rgba(iw, ih, &irgba);
        if !icon_big.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as WPARAM, icon_big as LPARAM);
        }
        if !icon_small.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as WPARAM, icon_small as LPARAM);
        }

        let state = AppState::new(data_root, config_dir, cat, cfg, hw);
        state.hwnd_raw.store(hwnd as isize, Ordering::Release);

        let host = WinHost {
            hwnd,
            dpi,
            scale: sf,
            client_w: cw,
            client_h: ch,
            backbuffer: BackBuffer::default(),
            fonts: FontCache::default(),
            state,
            icon_big,
            icon_small,
            cursor_arrow,
            cursor_hand,
            pointer_cursor: false,
            tracking_mouse: false,
            pending_high_surrogate: 0,
            drag_scroll: None,
            trim_timer_armed: true,
            ime_enabled: false,
            saved_placement: WINDOWPLACEMENT::default(),
            saved_style: style as i32,
        };
        WIN_HOST.with(|cell| {
            *cell.borrow_mut() = Some(host);
        });

        // Pre-render cleanly before ShowWindow to eliminate first-frame flicker
        let screen_dc = GetDC(hwnd);
        if !screen_dc.is_null() {
            WIN_HOST.with(|cell| {
                if let Some(ref mut h) = *cell.borrow_mut() {
                    h.render_frame(screen_dc);
                }
            });
            ReleaseDC(hwnd, screen_dc);
        }

        ShowWindow(hwnd, SW_SHOW);
        SetTimer(hwnd, TIMER_TRIM_ID, 600, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    diag::close();
}

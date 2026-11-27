use crate::catalog::Tool;
use crate::gdi::{GdiBitmap, to_wide_null};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::{CStr, c_void};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{FARPROC, HMODULE};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, GdiFlush, HGDIOBJ, SelectObject,
};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, HICON, PrivateExtractIconsW,
};

pub struct OwnedIcon(pub HICON);

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                DestroyIcon(self.0);
            }
            self.0 = null_mut();
        }
    }
}

/// Extracts an icon from a Windows PE executable directly as an HICON (zero pixel copy).
pub fn extract_exe_hicon(exe_path: &Path, size: i32) -> Option<OwnedIcon> {
    let wide = to_wide_null(exe_path.to_str()?);
    let mut hicon: HICON = null_mut();
    let mut icon_id: u32 = 0;
    let count = unsafe {
        PrivateExtractIconsW(
            wide.as_ptr(),
            0,
            size,
            size,
            &mut hicon,
            &mut icon_id,
            1,
            0,
        )
    };
    if count == 0 || count == u32::MAX || hicon.is_null() {
        return None;
    }
    Some(OwnedIcon(hicon))
}

fn sdf_round_box(u: f32, v: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let dx = (u - cx).abs() - (hw - r);
    let dy = (v - cy).abs() - (hh - r);
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - r
}

/// Generates a square RGBA app icon (mineral sage-teal tile + symmetric 2x2 warm-ivory grid).
pub fn synthesize_app_window_icon_rgba_sized(size: u32) -> (u32, u32, Vec<u8>) {
    let s = size.max(16);
    let mut buf = vec![0u8; (s * s * 4) as usize];
    let (bg_r, bg_g, bg_b) = (45.0f32, 110.0f32, 92.0f32); // #2D6E5C
    let (fg_r, fg_g, fg_b) = (250.0f32, 248.0f32, 243.0f32); // #FAF8F3
    const SS: usize = 8;
    let inv_ss = 1.0 / SS as f32;
    let inv_s = 1.0 / s as f32;
    let px_scale = s as f32;

    for py in 0..s {
        for px in 0..s {
            let mut sum_bg = 0.0f32;
            let mut sum_fg = 0.0f32;
            for sy in 0..SS {
                let v = (py as f32 + (sy as f32 + 0.5) * inv_ss) * inv_s;
                for sx in 0..SS {
                    let u = (px as f32 + (sx as f32 + 0.5) * inv_ss) * inv_s;
                    let d_bg = sdf_round_box(u, v, 0.5, 0.5, 0.465, 0.465, 0.235);
                    let cov_bg = (0.5 - d_bg * px_scale).clamp(0.0, 1.0);
                    if cov_bg > 0.0 {
                        sum_bg += cov_bg;
                        let d_tl = sdf_round_box(u, v, 0.3515, 0.3515, 0.1165, 0.1165, 0.034);
                        let d_tr = sdf_round_box(u, v, 0.6485, 0.3515, 0.1165, 0.1165, 0.034);
                        let d_bl = sdf_round_box(u, v, 0.3515, 0.6485, 0.1165, 0.1165, 0.034);
                        let d_br = sdf_round_box(u, v, 0.6485, 0.6485, 0.1165, 0.1165, 0.034);
                        let d_fg = d_tl.min(d_tr).min(d_bl).min(d_br);
                        let cov_fg = (0.5 - d_fg * px_scale).clamp(0.0, 1.0);
                        sum_fg += cov_bg.min(cov_fg);
                    }
                }
            }
            let n = (SS * SS) as f32;
            let a_bg = sum_bg / n;
            let a_fg = sum_fg / n;
            let idx = ((py * s + px) * 4) as usize;
            if a_bg > 0.002 {
                let t = (a_fg / a_bg).clamp(0.0, 1.0);
                buf[idx] = (bg_r * (1.0 - t) + fg_r * t).round().clamp(0.0, 255.0) as u8;
                buf[idx + 1] = (bg_g * (1.0 - t) + fg_g * t).round().clamp(0.0, 255.0) as u8;
                buf[idx + 2] = (bg_b * (1.0 - t) + fg_b * t).round().clamp(0.0, 255.0) as u8;
                buf[idx + 3] = (a_bg * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    (s, s, buf)
}

/// Generates a 64x64 rounded window/taskbar icon RGBA buffer (palette-independent brand icon).
#[allow(dead_code)]
pub fn synthesize_app_window_icon_rgba(_theme: &crate::gdi::ThemePalette) -> (u32, u32, Vec<u8>) {
    synthesize_app_window_icon_rgba_sized(64)
}

fn tool_icon_candidates(tool: &Tool) -> Vec<PathBuf> {
    let mut cands = Vec::new();
    let entry_is_exe = tool
        .entry_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("exe"))
        .unwrap_or(false);
    if entry_is_exe {
        cands.push(tool.entry_path.clone());
    }
    for v in &tool.variants {
        if v.path.to_lowercase().ends_with(".exe") {
            let pb = PathBuf::from(&v.path);
            if !cands.contains(&pb) {
                cands.push(pb);
            }
        }
    }
    if !tool.dir.as_os_str().is_empty() {
        for f in &tool.files {
            if !f.is_dir && f.name.to_lowercase().ends_with(".exe") {
                let pb = tool.dir.join(&f.name);
                if !cands.contains(&pb) {
                    cands.push(pb);
                }
            }
        }
    }
    cands
}

const MAX_ICON_CACHE_ENTRIES: usize = 128;

pub struct IconJob {
    pub tool_id: String,
    pub candidates: Vec<PathBuf>,
}

/// Worker-side PE icon extraction returning raw HICON value (0 if none) for Send safety.
pub fn extract_icon_job_raw(job: &IconJob, icon_px: i32) -> usize {
    for cand in &job.candidates {
        if let Some(owned) = extract_exe_hicon(cand, icon_px) {
            let raw = owned.0 as usize;
            core::mem::forget(owned);
            return raw;
        }
    }
    0
}

#[derive(Default)]
pub struct IconCache {
    map: HashMap<String, Option<Rc<OwnedIcon>>>,
    order: VecDeque<String>,
    pending: HashSet<String>,
}

impl IconCache {
    fn insert_entry(&mut self, tool_id: String, icon: Option<Rc<OwnedIcon>>) {
        self.pending.remove(&tool_id);
        if !self.map.contains_key(&tool_id) {
            while self.map.len() >= MAX_ICON_CACHE_ENTRIES {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                } else {
                    break;
                }
            }
            self.order.push_back(tool_id.clone());
        }
        self.map.insert(tool_id, icon);
    }

    pub fn insert_raw_hicon(&mut self, tool_id: String, raw_hicon: usize) {
        if raw_hicon != 0 {
            let icon = Rc::new(OwnedIcon(raw_hicon as HICON));
            self.insert_entry(tool_id, Some(icon));
        } else {
            self.insert_entry(tool_id, None);
        }
    }

    pub fn get_or_queue(&mut self, tool: &Tool) -> (Option<Rc<OwnedIcon>>, Option<IconJob>) {
        if let Some(cached) = self.map.get(&tool.id) {
            return (cached.clone(), None);
        }
        if tool.kind == "img" || tool.kind == "tui" {
            self.insert_entry(tool.id.clone(), None);
            return (None, None);
        }
        if self.pending.insert(tool.id.clone()) {
            let candidates = tool_icon_candidates(tool);
            if candidates.is_empty() {
                self.insert_entry(tool.id.clone(), None);
                return (None, None);
            }
            return (
                None,
                Some(IconJob {
                    tool_id: tool.id.clone(),
                    candidates,
                }),
            );
        }
        (None, None)
    }
}

#[repr(C)]
struct GdiplusStartupInput {
    gdiplus_version: u32,
    debug_event_callback: *mut c_void,
    suppress_background_thread: i32,
    suppress_external_codecs: i32,
}

type FnGdiplusStartup = unsafe extern "system" fn(*mut usize, *const GdiplusStartupInput, *mut c_void) -> i32;
type FnGdipLoadImageFromFile = unsafe extern "system" fn(*const u16, *mut *mut c_void) -> i32;
type FnGdipDisposeImage = unsafe extern "system" fn(*mut c_void) -> i32;
type FnGdipGetImageDim = unsafe extern "system" fn(*mut c_void, *mut u32) -> i32;
type FnGdipCreateFromHDC = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32;
type FnGdipDeleteGraphics = unsafe extern "system" fn(*mut c_void) -> i32;
type FnGdipSetInterpolationMode = unsafe extern "system" fn(*mut c_void, i32) -> i32;
type FnGdipDrawImageRectI =
    unsafe extern "system" fn(*mut c_void, *mut c_void, i32, i32, i32, i32) -> i32;

struct GdipApi {
    _module: HMODULE,
    _token: usize,
    load_image_from_file: FnGdipLoadImageFromFile,
    dispose_image: FnGdipDisposeImage,
    get_width: FnGdipGetImageDim,
    get_height: FnGdipGetImageDim,
    create_from_hdc: FnGdipCreateFromHDC,
    delete_graphics: FnGdipDeleteGraphics,
    set_interp_mode: FnGdipSetInterpolationMode,
    draw_image_rect_i: FnGdipDrawImageRectI,
}

unsafe impl Send for GdipApi {}
unsafe impl Sync for GdipApi {}

static GDIP: OnceLock<Option<GdipApi>> = OnceLock::new();

unsafe fn resolve_proc<T>(module: HMODULE, name: &CStr) -> Option<T> {
    let p: FARPROC = GetProcAddress(module, name.as_ptr() as *const u8);
    p.map(|f| core::mem::transmute_copy(&f))
}

fn ensure_gdip() -> Option<&'static GdipApi> {
    GDIP.get_or_init(|| unsafe {
        let dll = to_wide_null("gdiplus.dll");
        let module = LoadLibraryW(dll.as_ptr());
        if module.is_null() {
            return None;
        }
        let startup: FnGdiplusStartup = resolve_proc(module, c"GdiplusStartup")?;
        let load_image_from_file: FnGdipLoadImageFromFile =
            resolve_proc(module, c"GdipLoadImageFromFile")?;
        let dispose_image: FnGdipDisposeImage = resolve_proc(module, c"GdipDisposeImage")?;
        let get_width: FnGdipGetImageDim = resolve_proc(module, c"GdipGetImageWidth")?;
        let get_height: FnGdipGetImageDim = resolve_proc(module, c"GdipGetImageHeight")?;
        let create_from_hdc: FnGdipCreateFromHDC = resolve_proc(module, c"GdipCreateFromHDC")?;
        let delete_graphics: FnGdipDeleteGraphics = resolve_proc(module, c"GdipDeleteGraphics")?;
        let set_interp_mode: FnGdipSetInterpolationMode =
            resolve_proc(module, c"GdipSetInterpolationMode")?;
        let draw_image_rect_i: FnGdipDrawImageRectI =
            resolve_proc(module, c"GdipDrawImageRectI")?;

        let input = GdiplusStartupInput {
            gdiplus_version: 1,
            debug_event_callback: null_mut(),
            suppress_background_thread: 0,
            suppress_external_codecs: 0,
        };
        let mut token = 0usize;
        if startup(&mut token, &input, null_mut()) != 0 {
            return None;
        }
        Some(GdipApi {
            _module: module,
            _token: token,
            load_image_from_file,
            dispose_image,
            get_width,
            get_height,
            create_from_hdc,
            delete_graphics,
            set_interp_mode,
            draw_image_rect_i,
        })
    })
    .as_ref()
}

/// Decodes and scales an image using Windows `gdiplus.dll` (loaded lazily only when called).
pub fn decode_image_preview_gdi(
    path: &Path,
    max_dim: u32,
) -> Result<(GdiBitmap, u32, u32), String> {
    let api = ensure_gdip().ok_or_else(|| "无法初始化 Windows GDI+ 解码器".to_string())?;
    let wide = to_wide_null(path.to_str().ok_or_else(|| "无效图像路径".to_string())?);

    unsafe {
        let mut gp_img: *mut c_void = null_mut();
        if (api.load_image_from_file)(wide.as_ptr(), &mut gp_img) != 0 || gp_img.is_null() {
            return Err("无法解码图像文件".to_string());
        }

        let mut ow = 0u32;
        let mut oh = 0u32;
        let dim_ok =
            (api.get_width)(gp_img, &mut ow) == 0 && (api.get_height)(gp_img, &mut oh) == 0;
        if !dim_ok || ow == 0 || oh == 0 {
            (api.dispose_image)(gp_img);
            return Err("无效图像尺寸".to_string());
        }

        let (rw, rh) = if ow > max_dim || oh > max_dim {
            if ow >= oh {
                let h = ((oh as f64) * (max_dim as f64) / (ow as f64))
                    .round()
                    .max(1.0) as u32;
                (max_dim, h)
            } else {
                let w = ((ow as f64) * (max_dim as f64) / (oh as f64))
                    .round()
                    .max(1.0) as u32;
                (w, max_dim)
            }
        } else {
            (ow, oh)
        };

        let Some((bmp, bits_ptr)) = GdiBitmap::create_dib_target(rw, rh) else {
            (api.dispose_image)(gp_img);
            return Err("分配预览位图失败".to_string());
        };
        let pix = core::slice::from_raw_parts_mut(bits_ptr, (rw * rh) as usize);
        pix.fill(0xFFFF_FFFF);

        let hdc = CreateCompatibleDC(null_mut());
        if hdc.is_null() {
            (api.dispose_image)(gp_img);
            return Err("创建兼容 DC 失败".to_string());
        }
        let old = SelectObject(hdc, bmp.hbmp() as HGDIOBJ);
        let mut graphics: *mut c_void = null_mut();
        if (api.create_from_hdc)(hdc as *mut c_void, &mut graphics) == 0 && !graphics.is_null() {
            (api.set_interp_mode)(graphics, 7); // HighQualityBicubic
            (api.draw_image_rect_i)(graphics, gp_img, 0, 0, rw as i32, rh as i32);
            (api.delete_graphics)(graphics);
        }
        GdiFlush();
        for p in pix.iter_mut() {
            *p |= 0xFF00_0000;
        }
        SelectObject(hdc, old);
        DeleteDC(hdc);
        (api.dispose_image)(gp_img);

        Ok((bmp, ow, oh))
    }
}

#[derive(Default)]
pub struct PreviewImageCache {
    entries: HashMap<String, (Rc<GdiBitmap>, u32, u32)>,
    order: VecDeque<String>,
}

impl PreviewImageCache {
    pub fn get_cached(&self, path: &Path, max_dim: u32) -> Option<(Rc<GdiBitmap>, u32, u32)> {
        let key = format!("{}|{}", path.display(), max_dim);
        self.entries.get(&key).cloned()
    }

    pub fn insert_bitmap(
        &mut self,
        path: &Path,
        max_dim: u32,
        bmp: GdiBitmap,
        orig_w: u32,
        orig_h: u32,
    ) -> (Rc<GdiBitmap>, u32, u32) {
        let key = format!("{}|{}", path.display(), max_dim);
        if !self.entries.contains_key(&key) {
            while !self.entries.is_empty() {
                if let Some(old) = self.order.pop_front() {
                    self.entries.remove(&old);
                } else {
                    break;
                }
            }
            self.order.push_back(key.clone());
        }
        let val = (Rc::new(bmp), orig_w, orig_h);
        self.entries.insert(key, val.clone());
        val
    }

    #[cfg(test)]
    pub fn load_image_preview(
        &mut self,
        path: &Path,
        max_dim: u32,
    ) -> Result<(Rc<GdiBitmap>, u32, u32), String> {
        if let Some(cached) = self.get_cached(path, max_dim) {
            return Ok(cached);
        }
        let (bmp, orig_w, orig_h) = decode_image_preview_gdi(path, max_dim)?;
        Ok(self.insert_bitmap(path, max_dim, bmp, orig_w, orig_h))
    }

    pub fn clear(&mut self) -> bool {
        let had = !self.entries.is_empty();
        self.entries.clear();
        self.order.clear();
        had
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn extract_real_pe_icon() {
        let exe = workspace_root().join("tools/硬盘工具/CrystalDiskInfo/DiskInfo64.exe");
        let hicon = extract_exe_hicon(&exe, 32).expect("extract DiskInfo64 HICON");
        assert!(!hicon.0.is_null(), "expected non-null HICON");
    }

    #[test]
    fn decode_ladder_preview_image() {
        let img_path = workspace_root().join("tools/其他工具/天梯图/CPU天梯图.jpg");
        let mut cache = PreviewImageCache::default();
        let (bmp, ow, oh) = cache
            .load_image_preview(&img_path, 640)
            .expect("decode CPU ladder image via gdiplus");
        assert!(ow > 200 && oh > 200, "unexpected dims {ow}x{oh}");
        assert!(bmp.width > 0 && bmp.height > 0);
    }

    #[test]
    fn gdi_bitmap_from_rgba_creates_valid_handle() {
        let (w, h, rgba) = synthesize_app_window_icon_rgba(&crate::gdi::THEME);
        let bmp = GdiBitmap::from_rgba(w, h, &rgba).expect("create GdiBitmap");
        assert_eq!(bmp.width, w);
        assert_eq!(bmp.height, h);
        // Corners must be transparent
        assert_eq!(rgba[3], 0);
        // Dead center (0.5, 0.5) cross-channel must be mineral sage-teal (#2D6E5C)
        let cx = (w / 2) as usize;
        let cy = (h / 2) as usize;
        let c_idx = (cy * w as usize + cx) * 4;
        assert_eq!(&rgba[c_idx..c_idx + 4], &[45, 110, 92, 255]);
        // Top-left tile center (0.3515, 0.3515) must be warm ivory (#FAF8F3)
        let tx = (w as f32 * 0.3515).round() as usize;
        let ty = (h as f32 * 0.3515).round() as usize;
        let t_idx = (ty * w as usize + tx) * 4;
        assert_eq!(&rgba[t_idx..t_idx + 4], &[250, 248, 243, 255]);
    }

    #[test]
    fn embedded_ico_file_valid_multiresolution() {
        let ico_path = workspace_root().join("assets/app_icon.ico");
        let bytes = std::fs::read(&ico_path).expect("assets/app_icon.ico missing");
        assert!(bytes.len() > 6 + 16 * 7);
        assert_eq!(&bytes[0..6], &[0, 0, 1, 0, 7, 0]);
        let mut widths = Vec::new();
        for i in 0..7 {
            let entry = &bytes[6 + i * 16..6 + (i + 1) * 16];
            let w = if entry[0] == 0 { 256u32 } else { entry[0] as u32 };
            widths.push(w);
        }
        assert_eq!(widths, vec![16, 24, 32, 48, 64, 128, 256]);
    }
}

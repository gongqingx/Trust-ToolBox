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

/// Generates a 64x64 rounded Fluent Light window/taskbar icon as raw RGBA bytes.
pub fn synthesize_app_window_icon_rgba() -> (u32, u32, Vec<u8>) {
    const S: u32 = 64;
    let mut buf = vec![0u8; (S * S * 4) as usize];

    let fill_rect = |b: &mut [u8], x0: i32, y0: i32, x1: i32, y1: i32, col: [u8; 4]| {
        for y in y0.max(0)..y1.min(S as i32) {
            for x in x0.max(0)..x1.min(S as i32) {
                let idx = ((y as u32 * S + x as u32) * 4) as usize;
                b[idx..idx + 4].copy_from_slice(&col);
            }
        }
    };
    let stroke_rect =
        |b: &mut [u8], x0: i32, y0: i32, x1: i32, y1: i32, t: i32, col: [u8; 4]| {
            fill_rect(b, x0, y0, x1, y0 + t, col);
            fill_rect(b, x0, y1 - t, x1, y1, col);
            fill_rect(b, x0, y0, x0 + t, y1, col);
            fill_rect(b, x1 - t, y0, x1, y1, col);
        };

    for y in 0..S {
        for x in 0..S {
            let dx = if x < 12 {
                12 - x as i32
            } else if x >= S - 12 {
                x as i32 - (S as i32 - 13)
            } else {
                0
            };
            let dy = if y < 12 {
                12 - y as i32
            } else if y >= S - 12 {
                y as i32 - (S as i32 - 13)
            } else {
                0
            };
            let dist_sq = dx * dx + dy * dy;
            if dist_sq > 12 * 12 {
                continue;
            }
            let idx = ((y * S + x) * 4) as usize;
            let border = x < 2 || x >= S - 2 || y < 2 || y >= S - 2 || dist_sq >= 10 * 10;
            if border {
                buf[idx..idx + 4].copy_from_slice(&[29, 78, 216, 255]);
            } else {
                buf[idx..idx + 4].copy_from_slice(&[37, 99, 235, 255]);
            }
        }
    }

    for pin in [20, 28, 36, 44] {
        fill_rect(&mut buf, pin - 1, 13, pin + 2, 18, [255, 255, 255, 255]);
        fill_rect(&mut buf, pin - 1, 46, pin + 2, 51, [255, 255, 255, 255]);
        fill_rect(&mut buf, 13, pin - 1, 18, pin + 2, [255, 255, 255, 255]);
        fill_rect(&mut buf, 46, pin - 1, 51, pin + 2, [255, 255, 255, 255]);
    }
    fill_rect(&mut buf, 19, 19, 45, 45, [29, 78, 216, 255]);
    stroke_rect(&mut buf, 19, 19, 45, 45, 2, [255, 255, 255, 255]);
    fill_rect(&mut buf, 24, 25, 40, 29, [255, 255, 255, 255]);
    fill_rect(&mut buf, 30, 29, 34, 40, [219, 234, 254, 255]);

    (S, S, buf)
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
        let (w, h, rgba) = synthesize_app_window_icon_rgba();
        let bmp = GdiBitmap::from_rgba(w, h, &rgba).expect("create GdiBitmap");
        assert_eq!(bmp.width, w);
        assert_eq!(bmp.height, h);
    }
}

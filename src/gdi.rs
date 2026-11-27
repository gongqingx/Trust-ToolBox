use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{COLORREF, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BitBlt,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection, CreateFontW,
    DIB_RGB_COLORS, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CALCRECT, DT_CENTER, DT_END_ELLIPSIS,
    DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, DeleteDC,
    DeleteObject, DrawTextW, FF_DONTCARE, FW_BOLD, FW_NORMAL, FW_SEMIBOLD, GdiAlphaBlend,
    GdiFlush, GetTextExtentPoint32W, HALFTONE, HBITMAP, HDC, HFONT, HGDIOBJ, IntersectClipRect,
    OUT_DEFAULT_PRECIS, RestoreDC, SRCCOPY, SaveDC, SelectObject, SetBkMode, SetBrushOrgEx,
    SetStretchBltMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DrawIconEx, HICON};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const fn hex(v: u32) -> Self {
        Self {
            r: ((v >> 16) & 0xFF) as u8,
            g: ((v >> 8) & 0xFF) as u8,
            b: (v & 0xFF) as u8,
        }
    }

    #[inline]
    pub const fn colorref(self) -> COLORREF {
        ((self.b as u32) << 16) | ((self.g as u32) << 8) | (self.r as u32)
    }

    #[inline]
    pub const fn bgra_opaque(self) -> u32 {
        0xFF00_0000 | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePalette {
    pub canvas_bg: Rgb,
    pub surface_bg: Rgb,
    pub detail_bg: Rgb,
    pub card_bg: Rgb,
    pub card_hover: Rgb,
    pub card_selected: Rgb,
    pub card_selected_unfocused: Rgb,
    pub pill_bg: Rgb,
    pub keycap_bg: Rgb,

    pub border_dim: Rgb,
    pub border_subtle: Rgb,
    pub border_active: Rgb,
    pub border_soft_sel: Rgb,

    pub accent: Rgb,
    pub accent_dark: Rgb,
    pub accent_soft: Rgb,

    pub text_primary: Rgb,
    pub text_secondary: Rgb,
    pub text_muted: Rgb,
    pub text_white: Rgb,
    pub text_slate: Rgb,

    pub green_fg: Rgb,
    pub green_bg: Rgb,
    pub green_border: Rgb,
    pub amber_fg: Rgb,
    pub amber_bg: Rgb,
    pub amber_border: Rgb,
    pub red_fg: Rgb,
    pub red_bg: Rgb,
    pub red_border: Rgb,
    pub dim_backdrop: Rgb,
    pub scroll_thumb: Rgb,
    pub scroll_thumb_hover: Rgb,
}

/// Single cohesive "Warm Paper · Pure Charcoal Ink" palette.
pub const THEME: ThemePalette = ThemePalette {
    canvas_bg: Rgb::hex(0xF3F1EB),
    surface_bg: Rgb::hex(0xFCFBF8),
    detail_bg: Rgb::hex(0xF8F7F3),
    card_bg: Rgb::hex(0xF2EFE9),
    card_hover: Rgb::hex(0xEAE7DF),
    card_selected: Rgb::hex(0xEBE8E0),
    card_selected_unfocused: Rgb::hex(0xF2F0EA),
    pill_bg: Rgb::hex(0xEEECE5),
    keycap_bg: Rgb::hex(0xFCFBF8),

    border_dim: Rgb::hex(0xE3E0D8),
    border_subtle: Rgb::hex(0xEFECE5),
    border_active: Rgb::hex(0x2C2B28),
    border_soft_sel: Rgb::hex(0xD4D0C6),

    accent: Rgb::hex(0x1F1E1C),
    accent_dark: Rgb::hex(0x121110),
    accent_soft: Rgb::hex(0xE7E4DC),

    text_primary: Rgb::hex(0x1A1917),
    text_secondary: Rgb::hex(0x5E5B55),
    text_muted: Rgb::hex(0x8E8A82),
    text_white: Rgb::hex(0xFCFBF8),
    text_slate: Rgb::hex(0x363430),

    green_fg: Rgb::hex(0x2B7352),
    green_bg: Rgb::hex(0xEAF3EE),
    green_border: Rgb::hex(0xBBDCC9),
    amber_fg: Rgb::hex(0x9A5B13),
    amber_bg: Rgb::hex(0xFBF4E6),
    amber_border: Rgb::hex(0xE6CCA0),
    red_fg: Rgb::hex(0xB44036),
    red_bg: Rgb::hex(0xFAEEEC),
    red_border: Rgb::hex(0xE7BDB8),
    dim_backdrop: Rgb::hex(0x181715),
    scroll_thumb: Rgb::hex(0xD5D1C8),
    scroll_thumb_hover: Rgb::hex(0xABA69C),
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    #[inline]
    pub const fn right(self) -> i32 {
        self.x + self.w
    }

    #[inline]
    pub const fn bottom(self) -> i32 {
        self.y + self.h
    }

    #[inline]
    pub const fn contains(self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    #[inline]
    pub fn inset(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            w: (self.w - dx * 2).max(0),
            h: (self.h - dy * 2).max(0),
        }
    }

    #[inline]
    pub fn intersect(self, other: Self) -> Self {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        Self {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0),
            h: (y1 - y0).max(0),
        }
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    #[inline]
    pub fn to_win_rect(self) -> RECT {
        RECT {
            left: self.x,
            top: self.y,
            right: self.x + self.w,
            bottom: self.y + self.h,
        }
    }
}

pub fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().collect()
}

pub fn to_wide_null(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(core::iter::once(0)).collect()
}

pub struct GdiBitmap {
    hbmp: HBITMAP,
    pub width: u32,
    pub height: u32,
}

unsafe impl Send for GdiBitmap {}

impl Drop for GdiBitmap {
    fn drop(&mut self) {
        if !self.hbmp.is_null() {
            unsafe {
                DeleteObject(self.hbmp as HGDIOBJ);
            }
            self.hbmp = null_mut();
        }
    }
}

impl GdiBitmap {
    pub fn create_dib_target(width: u32, height: u32) -> Option<(Self, *mut u32)> {
        if width == 0 || height == 0 {
            return None;
        }
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: core::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
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
        let mut bits_ptr: *mut core::ffi::c_void = null_mut();
        let hbmp = unsafe {
            CreateDIBSection(null_mut(), &bmi, DIB_RGB_COLORS, &mut bits_ptr, null_mut(), 0)
        };
        if hbmp.is_null() || bits_ptr.is_null() {
            if !hbmp.is_null() {
                unsafe {
                    DeleteObject(hbmp as HGDIOBJ);
                }
            }
            return None;
        }
        Some((
            Self {
                hbmp,
                width,
                height,
            },
            bits_ptr as *mut u32,
        ))
    }

    pub fn hbmp(&self) -> HBITMAP {
        self.hbmp
    }

    #[cfg(test)]
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Self> {
        if width == 0 || height == 0 || rgba.len() < (width * height * 4) as usize {
            return None;
        }
        let (bmp, bits_ptr) = Self::create_dib_target(width, height)?;
        let count = (width * height) as usize;
        let dst = unsafe { core::slice::from_raw_parts_mut(bits_ptr as *mut u8, count * 4) };
        for (d, s) in dst.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
            let a = s[3] as u16;
            d[0] = ((s[2] as u16 * a + 127) / 255) as u8;
            d[1] = ((s[1] as u16 * a + 127) / 255) as u8;
            d[2] = ((s[0] as u16 * a + 127) / 255) as u8;
            d[3] = a as u8;
        }
        Some(bmp)
    }

    pub fn draw_into(&self, dst_hdc: HDC, scratch_dc: HDC, dst: Rect) {
        if self.hbmp.is_null() || scratch_dc.is_null() || dst.is_empty() {
            return;
        }
        unsafe {
            let old = SelectObject(scratch_dc, self.hbmp as HGDIOBJ);
            if dst.w != self.width as i32 || dst.h != self.height as i32 {
                SetStretchBltMode(dst_hdc, HALFTONE);
                SetBrushOrgEx(dst_hdc, 0, 0, null_mut());
            }
            let bf = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            GdiAlphaBlend(
                dst_hdc,
                dst.x,
                dst.y,
                dst.w,
                dst.h,
                scratch_dc,
                0,
                0,
                self.width as i32,
                self.height as i32,
                bf,
            );
            SelectObject(scratch_dc, old);
        }
    }
}

pub struct BackBuffer {
    pub hdc: HDC,
    pub scratch_dc: HDC,
    hbmp: HBITMAP,
    old_bmp: HGDIOBJ,
    bits: *mut u32,
    pub width: i32,
    pub height: i32,
}

impl Default for BackBuffer {
    fn default() -> Self {
        Self {
            hdc: null_mut(),
            scratch_dc: null_mut(),
            hbmp: null_mut(),
            old_bmp: null_mut(),
            bits: null_mut(),
            width: 0,
            height: 0,
        }
    }
}

impl Drop for BackBuffer {
    fn drop(&mut self) {
        self.release();
    }
}

impl BackBuffer {
    pub fn release(&mut self) {
        unsafe {
            if !self.hdc.is_null() {
                if !self.old_bmp.is_null() {
                    SelectObject(self.hdc, self.old_bmp);
                    self.old_bmp = null_mut();
                }
                DeleteDC(self.hdc);
                self.hdc = null_mut();
            }
            if !self.scratch_dc.is_null() {
                DeleteDC(self.scratch_dc);
                self.scratch_dc = null_mut();
            }
            if !self.hbmp.is_null() {
                DeleteObject(self.hbmp as HGDIOBJ);
                self.hbmp = null_mut();
            }
        }
        self.bits = null_mut();
        self.width = 0;
        self.height = 0;
    }

    pub fn ensure(&mut self, ref_hdc: HDC, w: i32, h: i32) -> bool {
        let w = w.max(1);
        let h = h.max(1);
        if self.width == w && self.height == h && !self.hdc.is_null() && !self.bits.is_null() {
            return true;
        }
        self.release();

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: core::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
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
            let hdc = CreateCompatibleDC(ref_hdc);
            let scratch_dc = CreateCompatibleDC(ref_hdc);
            let mut bits_ptr: *mut core::ffi::c_void = null_mut();
            let hbmp =
                CreateDIBSection(ref_hdc, &bmi, DIB_RGB_COLORS, &mut bits_ptr, null_mut(), 0);
            if hdc.is_null() || scratch_dc.is_null() || hbmp.is_null() || bits_ptr.is_null() {
                if !hbmp.is_null() {
                    DeleteObject(hbmp as HGDIOBJ);
                }
                if !hdc.is_null() {
                    DeleteDC(hdc);
                }
                if !scratch_dc.is_null() {
                    DeleteDC(scratch_dc);
                }
                return false;
            }
            let old_bmp = SelectObject(hdc, hbmp as HGDIOBJ);
            SetBkMode(hdc, TRANSPARENT as i32);
            self.hdc = hdc;
            self.scratch_dc = scratch_dc;
            self.hbmp = hbmp;
            self.old_bmp = old_bmp;
            self.bits = bits_ptr as *mut u32;
            self.width = w;
            self.height = h;
            true
        }
    }

    pub fn pixels_mut(&mut self) -> &mut [u32] {
        if self.bits.is_null() || self.width <= 0 || self.height <= 0 {
            return &mut [];
        }
        unsafe { core::slice::from_raw_parts_mut(self.bits, (self.width * self.height) as usize) }
    }

    pub fn blit_to(&self, dst_hdc: HDC) {
        if self.hdc.is_null() || self.width <= 0 || self.height <= 0 {
            return;
        }
        unsafe {
            GdiFlush();
            BitBlt(
                dst_hdc,
                0,
                0,
                self.width,
                self.height,
                self.hdc,
                0,
                0,
                SRCCOPY,
            );
        }
    }
}

pub struct FontCache {
    pub dpi: u32,
    pub hero_16b: HFONT,
    pub title_14b: HFONT,
    pub bold_13: HFONT,
    pub semi_125: HFONT,
    pub reg_125: HFONT,
    pub bold_12: HFONT,
    pub reg_12: HFONT,
    pub bold_11: HFONT,
    pub reg_11: HFONT,
    pub mono_11: HFONT,
    pub mono_11b: HFONT,
}

impl Default for FontCache {
    fn default() -> Self {
        Self {
            dpi: 0,
            hero_16b: null_mut(),
            title_14b: null_mut(),
            bold_13: null_mut(),
            semi_125: null_mut(),
            reg_125: null_mut(),
            bold_12: null_mut(),
            reg_12: null_mut(),
            bold_11: null_mut(),
            reg_11: null_mut(),
            mono_11: null_mut(),
            mono_11b: null_mut(),
        }
    }
}

impl Drop for FontCache {
    fn drop(&mut self) {
        self.clear();
    }
}

fn create_cleartype_font(face: &str, px_height: i32, weight: u32) -> HFONT {
    let wide = to_wide_null(face);
    unsafe {
        CreateFontW(
            -px_height,
            0,
            0,
            0,
            weight as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            (DEFAULT_PITCH as u32) | (FF_DONTCARE as u32),
            wide.as_ptr(),
        )
    }
}

impl FontCache {
    pub fn clear(&mut self) {
        let handles = [
            &mut self.hero_16b,
            &mut self.title_14b,
            &mut self.bold_13,
            &mut self.semi_125,
            &mut self.reg_125,
            &mut self.bold_12,
            &mut self.reg_12,
            &mut self.bold_11,
            &mut self.reg_11,
            &mut self.mono_11,
            &mut self.mono_11b,
        ];
        unsafe {
            for h in handles {
                if !(*h).is_null() {
                    DeleteObject(*h as HGDIOBJ);
                    *h = null_mut();
                }
            }
        }
        self.dpi = 0;
    }

    pub fn ensure_dpi(&mut self, dpi: u32) {
        let dpi = dpi.max(96);
        if self.dpi == dpi && !self.reg_12.is_null() {
            return;
        }
        self.clear();
        self.dpi = dpi;
        let sc = |logical_px: f32| -> i32 {
            ((logical_px * (dpi as f32 / 96.0)).round() as i32).max(10)
        };
        let ui = "Microsoft YaHei UI";
        let mono = "Consolas";
        self.hero_16b = create_cleartype_font(ui, sc(17.0), FW_BOLD);
        self.title_14b = create_cleartype_font(ui, sc(15.0), FW_BOLD);
        self.bold_13 = create_cleartype_font(ui, sc(13.5), FW_BOLD);
        self.semi_125 = create_cleartype_font(ui, sc(13.0), FW_SEMIBOLD);
        self.reg_125 = create_cleartype_font(ui, sc(13.0), FW_NORMAL);
        self.bold_12 = create_cleartype_font(ui, sc(12.5), FW_BOLD);
        self.reg_12 = create_cleartype_font(ui, sc(12.5), FW_NORMAL);
        self.bold_11 = create_cleartype_font(ui, sc(11.5), FW_BOLD);
        self.reg_11 = create_cleartype_font(ui, sc(11.5), FW_NORMAL);
        self.mono_11 = create_cleartype_font(mono, sc(11.5), FW_NORMAL);
        self.mono_11b = create_cleartype_font(mono, sc(11.5), FW_BOLD);
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[inline]
fn blend_channel(dst: u8, src: u8, cov_255: u32) -> u8 {
    let inv = 255 - cov_255;
    ((dst as u32 * inv + src as u32 * cov_255 + 127) / 255) as u8
}

#[inline]
fn blend_px_u32(dst_px: u32, fill: Rgb, cov: f32) -> u32 {
    if cov >= 0.996 {
        return fill.bgra_opaque();
    }
    if cov <= 0.004 {
        return dst_px;
    }
    let c = (cov * 255.0 + 0.5) as u32;
    let dr = ((dst_px >> 16) & 0xFF) as u8;
    let dg = ((dst_px >> 8) & 0xFF) as u8;
    let db = (dst_px & 0xFF) as u8;
    let nr = blend_channel(dr, fill.r, c);
    let ng = blend_channel(dg, fill.g, c);
    let nb = blend_channel(db, fill.b, c);
    0xFF00_0000 | ((nr as u32) << 16) | ((ng as u32) << 8) | (nb as u32)
}

pub struct Canvas<'a> {
    hdc: HDC,
    scratch_dc: HDC,
    pixels: &'a mut [u32],
    pub width: i32,
    pub scale: f32,
    clip: Rect,
    clip_stack: Vec<(Rect, i32)>,
    gdi_dirty: bool,
    pub fonts: &'a FontCache,
    pub theme: &'static ThemePalette,
}

impl<'a> Canvas<'a> {
    pub fn new(
        bb: &'a mut BackBuffer,
        fonts: &'a FontCache,
        dpi: u32,
        theme: &'static ThemePalette,
    ) -> Self {
        let w = bb.width;
        let h = bb.height;
        let hdc = bb.hdc;
        let scratch_dc = bb.scratch_dc;
        let pixels = bb.pixels_mut();
        Self {
            hdc,
            scratch_dc,
            pixels,
            width: w,
            scale: (dpi.max(96) as f32) / 96.0,
            clip: Rect::new(0, 0, w, h),
            clip_stack: Vec::with_capacity(8),
            gdi_dirty: false,
            fonts,
            theme,
        }
    }

    #[inline]
    pub fn dp(&self, logical: f32) -> i32 {
        (logical * self.scale).round() as i32
    }

    #[inline]
    pub fn dpf(&self, logical: f32) -> f32 {
        logical * self.scale
    }

    #[inline]
    fn ensure_cpu_sync(&mut self) {
        if self.gdi_dirty {
            unsafe {
                GdiFlush();
            }
            self.gdi_dirty = false;
        }
    }

    pub fn push_clip(&mut self, r: Rect) {
        let saved = unsafe { SaveDC(self.hdc) };
        let next = self.clip.intersect(r);
        self.clip_stack.push((self.clip, saved));
        self.clip = next;
        unsafe {
            IntersectClipRect(self.hdc, next.x, next.y, next.right(), next.bottom());
        }
    }

    pub fn pop_clip(&mut self) {
        if let Some((prev_clip, saved)) = self.clip_stack.pop() {
            self.clip = prev_clip;
            unsafe {
                RestoreDC(self.hdc, saved);
            }
        }
    }

    pub fn clear(&mut self, color: Rgb) {
        self.ensure_cpu_sync();
        self.pixels.fill(color.bgra_opaque());
    }

    pub fn fill_rect(&mut self, r: Rect, color: Rgb) {
        let cr = r.intersect(self.clip);
        if cr.is_empty() {
            return;
        }
        self.ensure_cpu_sync();
        let px = color.bgra_opaque();
        let stride = self.width as usize;
        for y in cr.y..cr.bottom() {
            let row = (y as usize) * stride;
            self.pixels[row + cr.x as usize..row + cr.right() as usize].fill(px);
        }
    }

    pub fn dim_rect(&mut self, r: Rect, color: Rgb, alpha: f32) {
        let cr = r.intersect(self.clip);
        if cr.is_empty() || alpha <= 0.004 {
            return;
        }
        self.ensure_cpu_sync();
        let stride = self.width as usize;
        for y in cr.y..cr.bottom() {
            let row = (y as usize) * stride;
            for x in cr.x..cr.right() {
                let idx = row + x as usize;
                self.pixels[idx] = blend_px_u32(self.pixels[idx], color, alpha);
            }
        }
    }

    /// Draws an analytical anti-aliased rounded rectangle with optional border.
    pub fn fill_round_rect(
        &mut self,
        r: Rect,
        radius_px: f32,
        fill: Option<Rgb>,
        border_px: f32,
        border_color: Rgb,
    ) {
        let cr = r.intersect(self.clip);
        if cr.is_empty() {
            return;
        }
        if radius_px <= 0.5 && border_px <= 0.0 {
            if let Some(c) = fill {
                self.fill_rect(r, c);
            }
            return;
        }
        self.ensure_cpu_sync();
        let hw = r.w as f32 * 0.5;
        let hh = r.h as f32 * 0.5;
        let rad = radius_px.clamp(0.0, hw.min(hh));
        let cx = r.x as f32 + hw;
        let cy = r.y as f32 + hh;
        let qx = hw - rad;
        let qy = hh - rad;
        let stride = self.width as usize;
        let has_border = border_px > 0.05;

        for py in cr.y..cr.bottom() {
            let row = (py as usize) * stride;
            let dy = (py as f32 + 0.5 - cy).abs() - qy;
            for px_x in cr.x..cr.right() {
                let dx = (px_x as f32 + 0.5 - cx).abs() - qx;
                let outer_dist = if dx > 0.0 && dy > 0.0 {
                    (dx * dx + dy * dy).sqrt() + dx.max(dy).min(0.0) - rad
                } else {
                    dx.max(dy) - rad
                };

                let outer_cov = (0.5 - outer_dist).clamp(0.0, 1.0);
                if outer_cov <= 0.0 {
                    continue;
                }

                let idx = row + px_x as usize;
                let mut dst = self.pixels[idx];
                if has_border {
                    let inner_dist = outer_dist + border_px;
                    let inner_cov = (0.5 - inner_dist).clamp(0.0, 1.0);
                    let border_cov = (outer_cov - inner_cov).max(0.0);
                    if let Some(fc) = fill {
                        if inner_cov > 0.0 {
                            dst = blend_px_u32(dst, fc, inner_cov);
                        }
                    }
                    if border_cov > 0.0 {
                        dst = blend_px_u32(dst, border_color, border_cov);
                    }
                    self.pixels[idx] = dst;
                } else if let Some(fc) = fill {
                    self.pixels[idx] = blend_px_u32(dst, fc, outer_cov);
                }
            }
        }
    }

    pub fn draw_circle(&mut self, cx: f32, cy: f32, r_px: f32, color: Rgb) {
        let min_x = ((cx - r_px - 1.0).floor() as i32).max(self.clip.x);
        let max_x = ((cx + r_px + 1.0).ceil() as i32).min(self.clip.right());
        let min_y = ((cy - r_px - 1.0).floor() as i32).max(self.clip.y);
        let max_y = ((cy + r_px + 1.0).ceil() as i32).min(self.clip.bottom());
        if min_x >= max_x || min_y >= max_y {
            return;
        }
        self.ensure_cpu_sync();
        let stride = self.width as usize;
        for py in min_y..max_y {
            let row = (py as usize) * stride;
            let dy = py as f32 + 0.5 - cy;
            for px in min_x..max_x {
                let dx = px as f32 + 0.5 - cx;
                let d = (dx * dx + dy * dy).sqrt() - r_px;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let idx = row + px as usize;
                    self.pixels[idx] = blend_px_u32(self.pixels[idx], color, cov);
                }
            }
        }
    }

    pub fn draw_text_single(
        &mut self,
        font: HFONT,
        text: &str,
        r: Rect,
        color: Rgb,
        align: TextAlign,
    ) {
        if text.is_empty() || r.is_empty() || r.intersect(self.clip).is_empty() {
            return;
        }
        let wide = to_wide(text);
        if wide.is_empty() {
            return;
        }
        let mut wr = r.to_win_rect();
        let align_flag = match align {
            TextAlign::Left => DT_LEFT,
            TextAlign::Center => DT_CENTER,
            TextAlign::Right => DT_RIGHT,
        };
        let flags = align_flag | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX;
        unsafe {
            let old = SelectObject(self.hdc, font as HGDIOBJ);
            SetTextColor(self.hdc, color.colorref());
            DrawTextW(self.hdc, wide.as_ptr(), wide.len() as i32, &mut wr, flags);
            SelectObject(self.hdc, old);
        }
        self.gdi_dirty = true;
    }

    pub fn draw_text_wrap(&mut self, font: HFONT, text: &str, r: Rect, color: Rgb) -> i32 {
        if text.is_empty() || r.w <= 0 {
            return 0;
        }
        let wide = to_wide(text);
        if wide.is_empty() {
            return 0;
        }
        let mut calc = r.to_win_rect();
        let flags = DT_LEFT | DT_WORDBREAK | DT_NOPREFIX;
        unsafe {
            let old = SelectObject(self.hdc, font as HGDIOBJ);
            let h = DrawTextW(
                self.hdc,
                wide.as_ptr(),
                wide.len() as i32,
                &mut calc,
                flags | DT_CALCRECT,
            );
            if !r.intersect(self.clip).is_empty() {
                let mut draw_r = r.to_win_rect();
                SetTextColor(self.hdc, color.colorref());
                DrawTextW(self.hdc, wide.as_ptr(), wide.len() as i32, &mut draw_r, flags);
                self.gdi_dirty = true;
            }
            SelectObject(self.hdc, old);
            h
        }
    }

    pub fn measure_text_wrap_height(&self, font: HFONT, text: &str, width: i32) -> i32 {
        if text.is_empty() || width <= 0 {
            return 0;
        }
        let wide = to_wide(text);
        if wide.is_empty() {
            return 0;
        }
        let mut calc = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: 0,
        };
        unsafe {
            let old = SelectObject(self.hdc, font as HGDIOBJ);
            let h = DrawTextW(
                self.hdc,
                wide.as_ptr(),
                wide.len() as i32,
                &mut calc,
                DT_LEFT | DT_WORDBREAK | DT_NOPREFIX | DT_CALCRECT,
            );
            SelectObject(self.hdc, old);
            h
        }
    }

    pub fn measure_text_width(&self, font: HFONT, text: &str) -> i32 {
        if text.is_empty() {
            return 0;
        }
        let wide = to_wide(text);
        if wide.is_empty() {
            return 0;
        }
        let mut sz = SIZE { cx: 0, cy: 0 };
        unsafe {
            let old = SelectObject(self.hdc, font as HGDIOBJ);
            GetTextExtentPoint32W(self.hdc, wide.as_ptr(), wide.len() as i32, &mut sz);
            SelectObject(self.hdc, old);
        }
        sz.cx
    }

    pub fn draw_bitmap(&mut self, bmp: &GdiBitmap, dst: Rect) {
        if dst.is_empty() || dst.intersect(self.clip).is_empty() {
            return;
        }
        bmp.draw_into(self.hdc, self.scratch_dc, dst);
        self.gdi_dirty = true;
    }

    pub fn draw_hicon(&mut self, hicon: HICON, x: i32, y: i32, w: i32, h: i32) {
        if hicon.is_null() || w <= 0 || h <= 0 || Rect::new(x, y, w, h).intersect(self.clip).is_empty() {
            return;
        }
        unsafe {
            DrawIconEx(self.hdc, x, y, hicon, w, h, 0, null_mut(), DI_NORMAL);
        }
        self.gdi_dirty = true;
    }

    pub fn draw_kind_placeholder(&mut self, kind: &str, r: Rect) {
        if r.is_empty() {
            return;
        }
        let t = self.theme;
        let rad = (r.w.min(r.h) as f32) * 0.25;
        let (bg, border, fg, ico) = match kind {
            "bat" => (t.amber_bg, t.amber_border, t.amber_fg, "terminal"),
            "archive" => (t.amber_bg, t.amber_border, t.amber_fg, "lock"),
            "img" => (t.card_selected, t.border_soft_sel, t.accent, "panel"),
            "tui" => (t.green_bg, t.green_border, t.green_fg, "screen"),
            "html" => (t.card_selected, t.border_soft_sel, t.accent, "web"),
            _ => (t.pill_bg, t.border_dim, t.text_secondary, "wrench"),
        };
        self.fill_round_rect(r, rad, Some(bg), 1.0, border);
        let pad = (r.w.min(r.h) / 5).max(3);
        let isz = (r.w.min(r.h) - pad * 2).max(8);
        self.draw_ui_icon(ico, r.x + (r.w - isz) / 2, r.y + (r.h - isz) / 2, isz, fg);
    }

    pub fn draw_ui_icon(&mut self, key: &str, x: i32, y: i32, size: i32, color: Rgb) {
        let bounds = Rect::new(x, y, size, size).intersect(self.clip);
        if bounds.is_empty() || size <= 2 {
            return;
        }
        self.ensure_cpu_sync();
        let stride = self.width as usize;
        let inv_size = 1.0 / size as f32;
        let px_scale = size as f32;
        let offsets = [0.25f32, 0.75f32];

        for py in bounds.y..bounds.bottom() {
            let row = (py as usize) * stride;
            for px in bounds.x..bounds.right() {
                let mut sum = 0.0f32;
                for &oy in &offsets {
                    let v = ((py - y) as f32 + oy) * inv_size;
                    for &ox in &offsets {
                        let u = ((px - x) as f32 + ox) * inv_size;
                        let sd = sdf_icon(key, u, v);
                        sum += (0.5 - sd * px_scale).clamp(0.0, 1.0);
                    }
                }
                let cov = sum * 0.25;
                if cov > 0.004 {
                    let idx = row + px as usize;
                    self.pixels[idx] = blend_px_u32(self.pixels[idx], color, cov);
                }
            }
        }
    }
}

#[inline]
fn sd_box(u: f32, v: f32, cx: f32, cy: f32, hw: f32, hh: f32) -> f32 {
    let dx = (u - cx).abs() - hw;
    let dy = (v - cy).abs() - hh;
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0)
}

#[inline]
fn sd_round_box(u: f32, v: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    sd_box(u, v, cx, cy, (hw - r).max(0.0), (hh - r).max(0.0)) - r
}

#[inline]
fn sd_circle(u: f32, v: f32, cx: f32, cy: f32, r: f32) -> f32 {
    (u - cx).hypot(v - cy) - r
}

#[inline]
fn sd_ring(u: f32, v: f32, cx: f32, cy: f32, r: f32, half_thick: f32) -> f32 {
    sd_circle(u, v, cx, cy, r).abs() - half_thick
}

#[inline]
fn sd_seg(u: f32, v: f32, ax: f32, ay: f32, bx: f32, by: f32, r: f32) -> f32 {
    let pax = u - ax;
    let pay = v - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let denom = bax * bax + bay * bay;
    let h = if denom > 1e-6 {
        ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (pax - bax * h).hypot(pay - bay * h) - r
}

#[inline]
fn sd_stroke_box(u: f32, v: f32, cx: f32, cy: f32, hw: f32, hh: f32, half_thick: f32) -> f32 {
    sd_box(u, v, cx, cy, hw, hh).abs() - half_thick
}

#[inline]
fn sd_triangle_right(u: f32, v: f32, x0: f32, x1: f32, cy: f32, half_h: f32) -> f32 {
    let d1 = sd_seg(u, v, x0, cy - half_h, x1, cy, 0.0);
    let d2 = sd_seg(u, v, x1, cy, x0, cy + half_h, 0.0);
    let d3 = sd_seg(u, v, x0, cy + half_h, x0, cy - half_h, 0.0);
    let edge = d1.min(d2).min(d3);
    let t = ((u - x0) / (x1 - x0).max(1e-4)).clamp(0.0, 1.0);
    let inside = u >= x0 && u <= x1 && (v - cy).abs() <= half_h * (1.0 - t);
    if inside { -edge } else { edge }
}

fn sdf_icon(key: &str, u: f32, v: f32) -> f32 {
    match key {
        "brand" | "overview" => {
            // Precision gear: toothed ring with center cutout
            let outer = sd_ring(u, v, 0.5, 0.5, 0.25, 0.085);
            let mut teeth = 1.0f32;
            for i in 0..8 {
                let ang = (i as f32) * core::f32::consts::FRAC_PI_4;
                let tx = 0.5 + ang.cos() * 0.35;
                let ty = 0.5 + ang.sin() * 0.35;
                teeth = teeth.min(sd_circle(u, v, tx, ty, 0.075));
            }
            outer.min(teeth)
        }
        "dashboard" => {
            let arc = sd_ring(u, v, 0.5, 0.54, 0.34, 0.06).max(v - 0.72);
            let center = sd_circle(u, v, 0.5, 0.58, 0.075);
            let needle = sd_seg(u, v, 0.5, 0.58, 0.70, 0.36, 0.045);
            arc.min(center).min(needle)
        }
        "star" => {
            let mut min_d = 1.0f32;
            let outer = 0.40f32;
            let inner = 0.17f32;
            let mut pts = [(0.0f32, 0.0f32); 10];
            for i in 0..10 {
                let r = if i % 2 == 0 { outer } else { inner };
                let a = -core::f32::consts::FRAC_PI_2 + (i as f32) * core::f32::consts::PI / 5.0;
                pts[i] = (0.5 + r * a.cos(), 0.52 + r * a.sin());
            }
            for i in 0..5 {
                let a = pts[i * 2];
                let b = pts[i * 2 + 1];
                let c = pts[(i * 2 + 9) % 10];
                min_d = min_d
                    .min(sd_seg(u, v, 0.5, 0.52, a.0, a.1, 0.045))
                    .min(sd_seg(u, v, a.0, a.1, b.0, b.1, 0.04))
                    .min(sd_seg(u, v, a.0, a.1, c.0, c.1, 0.04));
            }
            min_d.min(sd_circle(u, v, 0.5, 0.52, 0.17))
        }
        "grid" => {
            let d1 = sd_round_box(u, v, 0.29, 0.29, 0.16, 0.16, 0.03);
            let d2 = sd_round_box(u, v, 0.71, 0.29, 0.16, 0.16, 0.03);
            let d3 = sd_round_box(u, v, 0.29, 0.71, 0.16, 0.16, 0.03);
            let d4 = sd_round_box(u, v, 0.71, 0.71, 0.16, 0.16, 0.03);
            d1.min(d2).min(d3).min(d4)
        }
        "cpu" => {
            let frame = sd_stroke_box(u, v, 0.5, 0.5, 0.24, 0.24, 0.055);
            let core = sd_round_box(u, v, 0.5, 0.5, 0.11, 0.11, 0.02);
            let mut pins = 1.0f32;
            for p in [0.36, 0.50, 0.64] {
                pins = pins
                    .min(sd_seg(u, v, p, 0.13, p, 0.24, 0.035))
                    .min(sd_seg(u, v, p, 0.76, p, 0.87, 0.035))
                    .min(sd_seg(u, v, 0.13, p, 0.24, p, 0.035))
                    .min(sd_seg(u, v, 0.76, p, 0.87, p, 0.035));
            }
            frame.min(core).min(pins)
        }
        "ram" => {
            let stick = sd_stroke_box(u, v, 0.5, 0.46, 0.36, 0.16, 0.045);
            let mut chips = 1.0f32;
            for cx in [0.27, 0.42, 0.58, 0.73] {
                chips = chips.min(sd_round_box(u, v, cx, 0.46, 0.045, 0.08, 0.01));
            }
            let mut legs = 1.0f32;
            for lx in [0.22, 0.36, 0.50, 0.64, 0.78] {
                legs = legs.min(sd_seg(u, v, lx, 0.63, lx, 0.75, 0.035));
            }
            stick.min(chips).min(legs)
        }
        "gpu" => {
            let card = sd_stroke_box(u, v, 0.53, 0.48, 0.33, 0.21, 0.05);
            let bracket = sd_seg(u, v, 0.15, 0.22, 0.15, 0.78, 0.04);
            let fan1 = sd_ring(u, v, 0.40, 0.48, 0.10, 0.04);
            let fan2 = sd_ring(u, v, 0.68, 0.48, 0.10, 0.04);
            card.min(bracket).min(fan1).min(fan2)
        }
        "disk" => {
            let body = sd_stroke_box(u, v, 0.5, 0.5, 0.28, 0.36, 0.05);
            let platter = sd_ring(u, v, 0.5, 0.40, 0.16, 0.045);
            let hub = sd_circle(u, v, 0.5, 0.40, 0.055);
            let bar = sd_seg(u, v, 0.33, 0.74, 0.67, 0.74, 0.04);
            body.min(platter).min(hub).min(bar)
        }
        "screen" => {
            let frame = sd_stroke_box(u, v, 0.5, 0.43, 0.36, 0.25, 0.055);
            let stem = sd_seg(u, v, 0.5, 0.69, 0.5, 0.82, 0.045);
            let base = sd_seg(u, v, 0.32, 0.82, 0.68, 0.82, 0.045);
            frame.min(stem).min(base)
        }
        "burn" => {
            let base = sd_circle(u, v, 0.5, 0.60, 0.24);
            let mid = sd_circle(u, v, 0.5, 0.45, 0.13);
            let l_tip = sd_seg(u, v, 0.31, 0.54, 0.50, 0.16, 0.06);
            let r_tip = sd_seg(u, v, 0.69, 0.54, 0.50, 0.16, 0.06);
            let outer = base.min(mid).min(l_tip).min(r_tip);
            let inner_core = sd_circle(u, v, 0.5, 0.63, 0.10)
                .min(sd_seg(u, v, 0.5, 0.63, 0.5, 0.44, 0.04));
            outer.max(-inner_core)
        }
        "mobo" => {
            let board = sd_stroke_box(u, v, 0.5, 0.5, 0.34, 0.34, 0.05);
            let socket = sd_round_box(u, v, 0.40, 0.38, 0.11, 0.11, 0.02);
            let pcie = sd_seg(u, v, 0.30, 0.64, 0.70, 0.64, 0.04);
            let dimm = sd_seg(u, v, 0.64, 0.30, 0.64, 0.50, 0.04);
            board.min(socket).min(pcie).min(dimm)
        }
        "periph" => {
            let mouse = sd_round_box(u, v, 0.5, 0.5, 0.23, 0.35, 0.20);
            let hollow = sd_round_box(u, v, 0.5, 0.5, 0.14, 0.26, 0.13);
            let shell = mouse.max(-hollow);
            let wheel = sd_seg(u, v, 0.5, 0.28, 0.5, 0.44, 0.045);
            let line = sd_seg(u, v, 0.32, 0.48, 0.68, 0.48, 0.035);
            shell.min(wheel).min(line)
        }
        "other" => {
            let box_out = sd_stroke_box(u, v, 0.5, 0.5, 0.32, 0.32, 0.05);
            let plus_h = sd_seg(u, v, 0.34, 0.5, 0.66, 0.5, 0.045);
            let plus_v = sd_seg(u, v, 0.5, 0.34, 0.5, 0.66, 0.045);
            box_out.min(plus_h).min(plus_v)
        }
        "search" => {
            let lens = sd_ring(u, v, 0.44, 0.44, 0.24, 0.055);
            let handle = sd_seg(u, v, 0.61, 0.61, 0.82, 0.82, 0.055);
            lens.min(handle)
        }
        "play" => sd_triangle_right(u, v, 0.28, 0.78, 0.5, 0.31),
        "shield" => {
            let top = sd_seg(u, v, 0.22, 0.22, 0.78, 0.22, 0.045);
            let left = sd_seg(u, v, 0.22, 0.22, 0.24, 0.54, 0.045);
            let right = sd_seg(u, v, 0.78, 0.22, 0.76, 0.54, 0.045);
            let bl = sd_seg(u, v, 0.24, 0.54, 0.50, 0.84, 0.045);
            let br = sd_seg(u, v, 0.76, 0.54, 0.50, 0.84, 0.045);
            let c1 = sd_seg(u, v, 0.37, 0.50, 0.47, 0.60, 0.045);
            let c2 = sd_seg(u, v, 0.47, 0.60, 0.65, 0.39, 0.045);
            top.min(left).min(right).min(bl).min(br).min(c1).min(c2)
        }
        "folder" => {
            let body = sd_round_box(u, v, 0.5, 0.56, 0.36, 0.24, 0.05);
            let tab = sd_round_box(u, v, 0.32, 0.30, 0.16, 0.09, 0.04);
            body.min(tab)
        }
        "wrench" => {
            let shaft = sd_seg(u, v, 0.22, 0.78, 0.64, 0.36, 0.065);
            let head = sd_ring(u, v, 0.68, 0.32, 0.16, 0.06);
            let notch = sd_circle(u, v, 0.76, 0.24, 0.09);
            shaft.min(head.max(-notch))
        }
        "lock" => {
            let body = sd_round_box(u, v, 0.5, 0.62, 0.27, 0.21, 0.05);
            let shackle = sd_ring(u, v, 0.5, 0.34, 0.17, 0.05).max(v - 0.42);
            let hole = sd_circle(u, v, 0.5, 0.62, 0.055);
            body.max(-hole).min(shackle)
        }
        "warn" => {
            let tri1 = sd_seg(u, v, 0.5, 0.17, 0.16, 0.81, 0.05);
            let tri2 = sd_seg(u, v, 0.16, 0.81, 0.84, 0.81, 0.05);
            let tri3 = sd_seg(u, v, 0.84, 0.81, 0.5, 0.17, 0.05);
            let ex_line = sd_seg(u, v, 0.5, 0.39, 0.5, 0.58, 0.045);
            let ex_dot = sd_circle(u, v, 0.5, 0.69, 0.045);
            tri1.min(tri2).min(tri3).min(ex_line).min(ex_dot)
        }
        "refresh" => {
            let ring = sd_ring(u, v, 0.5, 0.5, 0.29, 0.055);
            let cut = sd_box(u, v, 0.78, 0.30, 0.16, 0.12);
            let arrow1 = sd_seg(u, v, 0.65, 0.20, 0.79, 0.31, 0.045);
            let arrow2 = sd_seg(u, v, 0.79, 0.17, 0.79, 0.31, 0.045);
            ring.max(-cut).min(arrow1).min(arrow2)
        }
        "copy" => {
            let front = sd_stroke_box(u, v, 0.42, 0.58, 0.21, 0.23, 0.045);
            let b_top = sd_seg(u, v, 0.36, 0.20, 0.76, 0.20, 0.045);
            let b_right = sd_seg(u, v, 0.76, 0.20, 0.76, 0.63, 0.045);
            front.min(b_top).min(b_right)
        }
        "export" => {
            let shaft = sd_seg(u, v, 0.5, 0.18, 0.5, 0.62, 0.048);
            let al = sd_seg(u, v, 0.5, 0.62, 0.33, 0.45, 0.048);
            let ar = sd_seg(u, v, 0.5, 0.62, 0.67, 0.45, 0.048);
            let tray = sd_seg(u, v, 0.20, 0.80, 0.80, 0.80, 0.05);
            shaft.min(al).min(ar).min(tray)
        }
        "terminal" => {
            let frame = sd_stroke_box(u, v, 0.5, 0.5, 0.34, 0.27, 0.05);
            let p1 = sd_seg(u, v, 0.28, 0.39, 0.42, 0.50, 0.045);
            let p2 = sd_seg(u, v, 0.42, 0.50, 0.28, 0.61, 0.045);
            let under = sd_seg(u, v, 0.50, 0.61, 0.70, 0.61, 0.045);
            frame.min(p1).min(p2).min(under)
        }
        "help" => {
            let ring = sd_ring(u, v, 0.5, 0.5, 0.36, 0.055);
            let q1 = sd_ring(u, v, 0.5, 0.37, 0.11, 0.042).max(-(v - 0.41));
            let q2 = sd_seg(u, v, 0.60, 0.40, 0.50, 0.53, 0.042);
            let dot = sd_circle(u, v, 0.50, 0.66, 0.048);
            ring.min(q1).min(q2).min(dot)
        }
        "close" => {
            let d1 = sd_seg(u, v, 0.24, 0.24, 0.76, 0.76, 0.055);
            let d2 = sd_seg(u, v, 0.76, 0.24, 0.24, 0.76, 0.055);
            d1.min(d2)
        }
        "sort" => {
            let b1 = sd_seg(u, v, 0.16, 0.28, 0.84, 0.28, 0.05);
            let b2 = sd_seg(u, v, 0.26, 0.50, 0.74, 0.50, 0.05);
            let b3 = sd_seg(u, v, 0.38, 0.72, 0.62, 0.72, 0.05);
            b1.min(b2).min(b3)
        }
        "panel" => {
            let outer = sd_stroke_box(u, v, 0.5, 0.5, 0.35, 0.27, 0.048);
            let div = sd_seg(u, v, 0.62, 0.23, 0.62, 0.77, 0.045);
            outer.min(div)
        }
        "web" => {
            let outer = sd_ring(u, v, 0.5, 0.5, 0.35, 0.05);
            let h = sd_seg(u, v, 0.17, 0.5, 0.83, 0.5, 0.04);
            let v_line = sd_seg(u, v, 0.5, 0.17, 0.5, 0.83, 0.04);
            outer.min(h).min(v_line)
        }
        _ => sd_round_box(u, v, 0.5, 0.5, 0.28, 0.28, 0.06),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luma(c: Rgb) -> u32 {
        c.r as u32 * 299 + c.g as u32 * 587 + c.b as u32 * 114
    }

    #[test]
    fn warm_paper_ink_surface_hierarchy() {
        assert!(luma(THEME.surface_bg) > luma(THEME.detail_bg));
        assert!(luma(THEME.detail_bg) > luma(THEME.canvas_bg));
        assert!(luma(THEME.accent) < 40_000);
        assert!(luma(THEME.text_white) > 240_000);
    }
}

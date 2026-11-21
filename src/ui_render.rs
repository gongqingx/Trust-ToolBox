use crate::app_state::{
    ActiveModal, ActiveView, AppState, FocusPanel, FrameLayout, HitAction, HitRegion, ScrollTarget,
    ScrollTrack, non_empty, sort_label, tool_kind_badge,
};
use crate::catalog::{Tool, format_bytes};
use crate::gdi::{Canvas, Rect, Rgb, TextAlign, theme};
use crate::icons::IconCache;
use crate::launcher::TrackedProcessSnapshot;

struct Ui<'a, 'b> {
    c: &'a mut Canvas<'b>,
    hovered: Option<HitAction>,
    hits: Vec<HitRegion>,
    scrolls: Vec<ScrollTrack>,
    list_viewport_rect: Option<Rect>,
}

impl<'a, 'b> Ui<'a, 'b> {
    fn new(c: &'a mut Canvas<'b>, hovered: Option<HitAction>) -> Self {
        Self {
            c,
            hovered,
            hits: Vec::with_capacity(96),
            scrolls: Vec::with_capacity(4),
            list_viewport_rect: None,
        }
    }

    #[inline]
    fn dp(&self, v: f32) -> i32 {
        self.c.dp(v)
    }

    #[inline]
    fn dpf(&self, v: f32) -> f32 {
        self.c.dpf(v)
    }

    fn push_hit(&mut self, rect: Rect, action: HitAction, pointer_cursor: bool) {
        if !rect.is_empty() {
            self.hits.push(HitRegion {
                rect,
                action,
                pointer_cursor,
            });
        }
    }

    fn is_hovered(&self, action: &HitAction) -> bool {
        self.hovered.as_ref() == Some(action)
    }

    fn draw_panel(&mut self, r: Rect, focused: bool, fill: Rgb, focus_action: Option<HitAction>) {
        let bw = if focused { self.dpf(2.0).max(2.0) } else { 1.0 };
        let bcol = if focused {
            theme::BORDER_ACTIVE
        } else {
            theme::BORDER_DIM
        };
        self.c
            .fill_round_rect(r, self.dpf(10.0), Some(fill), bw, bcol);
        if let Some(act) = focus_action {
            self.push_hit(r, act, false);
        }
    }

    fn measure_badge_w(&self, text: &str, bold: bool) -> i32 {
        let f = if bold {
            self.c.fonts.bold_11
        } else {
            self.c.fonts.reg_11
        };
        self.c.measure_text_width(f, text) + self.dp(14.0)
    }

    fn draw_badge(&mut self, r: Rect, text: &str, fg: Rgb, bg: Rgb, bold: bool) {
        self.c
            .fill_round_rect(r, self.dpf(5.0), Some(bg), 0.0, theme::BORDER_DIM);
        let f = if bold {
            self.c.fonts.bold_11
        } else {
            self.c.fonts.reg_11
        };
        self.c.draw_text_single(f, text, r, fg, TextAlign::Center);
    }

    fn measure_button_w(&self, icon: Option<&str>, label: &str, bold: bool) -> i32 {
        let f = if bold {
            self.c.fonts.bold_12
        } else {
            self.c.fonts.reg_12
        };
        let mut w = self.dp(20.0);
        if icon.is_some() {
            w += self.dp(13.0);
            if !label.is_empty() {
                w += self.dp(5.0);
            }
        }
        if !label.is_empty() {
            w += self.c.measure_text_width(f, label);
        }
        w
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_button(
        &mut self,
        r: Rect,
        icon: Option<&str>,
        label: &str,
        fg: Rgb,
        bg: Rgb,
        hover_bg: Rgb,
        border_col: Rgb,
        bold: bool,
        action: HitAction,
    ) {
        let hov = self.is_hovered(&action);
        let actual_bg = if hov { hover_bg } else { bg };
        let actual_bc = if hov { theme::ACCENT } else { border_col };
        self.c
            .fill_round_rect(r, self.dpf(6.0), Some(actual_bg), 1.0, actual_bc);

        let f = if bold {
            self.c.fonts.bold_12
        } else {
            self.c.fonts.reg_12
        };
        let isz = self.dp(13.0);
        let gap = if icon.is_some() && !label.is_empty() {
            self.dp(5.0)
        } else {
            0
        };
        let tw = if !label.is_empty() {
            self.c.measure_text_width(f, label)
        } else {
            0
        };
        let total = (if icon.is_some() { isz } else { 0 }) + gap + tw;
        let mut cx = r.x + (r.w - total).max(0) / 2;
        if let Some(ico) = icon {
            self.c.draw_ui_icon(ico, cx, r.y + (r.h - isz) / 2, isz, fg);
            cx += isz + gap;
        }
        if !label.is_empty() {
            self.c.draw_text_single(
                f,
                label,
                Rect::new(cx, r.y, tw + self.dp(4.0), r.h),
                fg,
                TextAlign::Left,
            );
        }
        self.push_hit(r, action, true);
    }

    fn draw_keycap(&mut self, x: i32, y: i32, h: i32, key_text: &str) -> i32 {
        let tw = self.c.measure_text_width(self.c.fonts.mono_11b, key_text);
        let w = (tw + self.dp(12.0)).max(self.dp(22.0));
        let r = Rect::new(x, y, w, h);
        self.c.fill_round_rect(
            r,
            self.dpf(4.0),
            Some(theme::KEYCAP_BG),
            1.0,
            theme::BORDER_DIM,
        );
        self.c
            .draw_text_single(self.c.fonts.mono_11b, key_text, r, theme::ACCENT, TextAlign::Center);
        w
    }

    fn draw_tool_icon(&mut self, cache: &mut IconCache, tool: &Tool, r: Rect) {
        let (hicon_opt, _) = cache.get_or_queue(tool);
        if let Some(hicon) = hicon_opt {
            self.c.draw_hicon(hicon.0, r.x, r.y, r.w, r.h);
        } else {
            self.c.draw_kind_placeholder(&tool.kind, r);
        }
    }

    fn add_scrollbar(&mut self, target: ScrollTarget, viewport: Rect, content_h: i32, offset: &mut f32) {
        let max_scroll = (content_h - viewport.h).max(0) as f32;
        *offset = offset.clamp(0.0, max_scroll);
        if max_scroll <= 1.0 {
            return;
        }
        let bar_w = self.dp(8.0);
        let track = Rect::new(viewport.right() - bar_w, viewport.y, bar_w, viewport.h);
        let off = self.dp(2.0);
        let usable = (track.h - off * 2).max(1);
        let thumb_h = (((viewport.h as f32 / content_h as f32) * usable as f32).round() as i32)
            .clamp(self.dp(28.0), usable);
        let travel = (usable - thumb_h).max(0);
        let ratio = (*offset / max_scroll).clamp(0.0, 1.0);
        let ty = track.y + off + (travel as f32 * ratio).round() as i32;
        let act = HitAction::ScrollbarDrag(target);
        let hov = self.is_hovered(&act);
        let thumb_w = self.dp(if hov { 6.0 } else { 4.0 });
        let tx = track.x + (track.w - thumb_w) / 2;
        let thumb = Rect::new(tx, ty, thumb_w, thumb_h);
        let col = if hov {
            theme::SCROLL_THUMB_HOVER
        } else {
            theme::SCROLL_THUMB
        };
        self.c
            .fill_round_rect(thumb, thumb_w as f32 * 0.5, Some(col), 0.0, col);
        self.scrolls.push(ScrollTrack {
            target,
            viewport,
            track,
            thumb,
            content_h,
        });
        self.push_hit(track, act, true);
    }

    fn finish(self) -> FrameLayout {
        FrameLayout {
            hits: self.hits,
            scrolls: self.scrolls,
            list_viewport_rect: self.list_viewport_rect,
        }
    }
}

pub fn render_app(
    canvas: &mut Canvas,
    state: &mut AppState,
    w: i32,
    h: i32,
) -> FrameLayout {
    if state.active_modal == ActiveModal::ScreenTest {
        return render_screen_test(canvas, state, w, h);
    }

    canvas.clear(theme::CANVAS_BG);
    let hov = state.hovered_action.clone();
    let mut ui = Ui::new(canvas, hov);

    let pad = ui.dp(10.0);
    let gap = ui.dp(10.0);
    let top_h = ui.dp(48.0);
    let status_h = ui.dp(28.0);
    let footer_h = if state.cfg.show_footer {
        ui.dp(140.0)
    } else {
        0
    };

    // 1. Top Command Bar
    let top_r = Rect::new(pad, pad, (w - pad * 2).max(1), top_h);
    render_topbar(&mut ui, state, top_r);

    // 2. Status Bar (anchor bottom)
    let status_r = Rect::new(pad, h - pad - status_h, (w - pad * 2).max(1), status_h);
    render_statusbar(&mut ui, state, status_r);

    // 3. Footer Dock
    let mut work_bottom = status_r.y - ui.dp(6.0);
    if state.cfg.show_footer {
        let foot_r = Rect::new(pad, work_bottom - footer_h, (w - pad * 2).max(1), footer_h);
        render_footer_dock(&mut ui, state, foot_r);
        work_bottom = foot_r.y - ui.dp(8.0);
    }

    // 4. Main Workspace row (Sidebar | Center Main Panel | Detail Panel)
    let ws_y = top_r.bottom() + gap;
    let ws_h = (work_bottom - ws_y).max(ui.dp(140.0));
    let sidebar_w = ui.dp(210.0);
    let sidebar_r = Rect::new(pad, ws_y, sidebar_w, ws_h);

    let show_detail = state.cfg.show_preview && w >= ui.dp(760.0);
    let detail_w = if show_detail { ui.dp(330.0) } else { 0 };
    let center_x = sidebar_r.right() + gap;
    let center_w = (w - pad - center_x - if show_detail { detail_w + gap } else { 0 }).max(ui.dp(220.0));
    let center_r = Rect::new(center_x, ws_y, center_w, ws_h);

    render_sidebar(&mut ui, state, sidebar_r);
    if state.active_view == ActiveView::Hardware {
        render_hardware_main(&mut ui, state, center_r);
    } else {
        render_tools_main(&mut ui, state, center_r);
    }

    if show_detail {
        let detail_r = Rect::new(center_r.right() + gap, ws_y, detail_w, ws_h);
        if state.active_view == ActiveView::Hardware {
            render_hardware_detail(&mut ui, state, detail_r);
        } else {
            render_tool_detail(&mut ui, state, detail_r);
        }
    }

    // 5. Overlays
    match state.active_modal {
        ActiveModal::Help => render_help_modal(&mut ui, state, w, h),
        ActiveModal::Variants => render_variants_modal(&mut ui, state, w, h),
        ActiveModal::None | ActiveModal::ScreenTest => {}
    }

    let layout = ui.finish();
    state.last_frame = layout.clone();
    layout
}

fn render_topbar(ui: &mut Ui, state: &AppState, r: Rect) {
    ui.draw_panel(r, false, theme::SURFACE_BG, None);
    let inner = r.inset(ui.dp(10.0), ui.dp(7.0));
    let mut lx = inner.x;

    // Brand Pill
    let brand_w = ui.dp(122.0);
    let brand_r = Rect::new(lx, inner.y, brand_w, inner.h);
    ui.c.fill_round_rect(
        brand_r,
        ui.dpf(7.0),
        Some(theme::CARD_BG),
        1.0,
        theme::BORDER_DIM,
    );
    let ico_sz = ui.dp(16.0);
    ui.c.draw_ui_icon(
        "brand",
        brand_r.x + ui.dp(10.0),
        brand_r.y + (brand_r.h - ico_sz) / 2,
        ico_sz,
        theme::ACCENT,
    );
    ui.c.draw_text_single(
        ui.c.fonts.bold_13,
        "Trust工具箱",
        Rect::new(
            brand_r.x + ui.dp(32.0),
            brand_r.y,
            brand_r.w - ui.dp(36.0),
            brand_r.h,
        ),
        theme::TEXT_PRIMARY,
        TextAlign::Left,
    );
    lx = brand_r.right() + ui.dp(6.0);

    // PRO Chip
    let pro_w = ui.dp(38.0);
    let pro_h = ui.dp(22.0);
    ui.draw_badge(
        Rect::new(lx, inner.y + (inner.h - pro_h) / 2, pro_w, pro_h),
        "PRO",
        theme::TEXT_SLATE,
        theme::PILL_BG,
        true,
    );
    lx += pro_w + ui.dp(6.0);

    // Hardware Summary Chip (if enough width)
    let hw_txt = state.header_hw_summary();
    let show_hw_chip = !hw_txt.is_empty() && r.w >= ui.dp(920.0);
    if show_hw_chip {
        let hw_tw = ui.c.measure_text_width(ui.c.fonts.reg_11, &hw_txt);
        let hw_w = (hw_tw + ui.dp(36.0)).min(ui.dp(250.0));
        let hw_r = Rect::new(lx, inner.y + ui.dp(3.0), hw_w, inner.h - ui.dp(6.0));
        ui.c.fill_round_rect(
            hw_r,
            ui.dpf(6.0),
            Some(theme::CARD_BG),
            1.0,
            theme::BORDER_DIM,
        );
        let csz = ui.dp(13.0);
        ui.c.draw_ui_icon(
            "cpu",
            hw_r.x + ui.dp(8.0),
            hw_r.y + (hw_r.h - csz) / 2,
            csz,
            theme::ACCENT,
        );
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            &hw_txt,
            Rect::new(
                hw_r.x + ui.dp(25.0),
                hw_r.y,
                hw_r.w - ui.dp(32.0),
                hw_r.h,
            ),
            theme::TEXT_SECONDARY,
            TextAlign::Left,
        );
        lx = hw_r.right() + ui.dp(8.0);
    }

    // Right controls (measure from right to left so Search Box expands to fill remainder)
    let mut rx = inner.right();
    let btn_h = ui.dp(28.0);
    let by = inner.y + (inner.h - btn_h) / 2;

    // `?` help button
    let help_w = ui.dp(32.0);
    rx -= help_w;
    ui.draw_button(
        Rect::new(rx, by, help_w, btn_h),
        Some("help"),
        "",
        theme::TEXT_WHITE,
        theme::TEXT_PRIMARY,
        Rgb::hex(0x1E293B),
        theme::TEXT_PRIMARY,
        true,
        HitAction::OpenHelp,
    );
    rx -= ui.dp(6.0);

    // `底栏` toggle
    let foot_w = ui.measure_button_w(Some("terminal"), "底栏", state.cfg.show_footer);
    rx -= foot_w;
    ui.draw_button(
        Rect::new(rx, by, foot_w, btn_h),
        Some("terminal"),
        "底栏",
        if state.cfg.show_footer {
            theme::ACCENT
        } else {
            theme::TEXT_MUTED
        },
        if state.cfg.show_footer {
            theme::CARD_SELECTED
        } else {
            theme::CARD_BG
        },
        theme::CARD_HOVER,
        if state.cfg.show_footer {
            theme::BORDER_SOFT_SEL
        } else {
            theme::BORDER_DIM
        },
        state.cfg.show_footer,
        HitAction::ToggleFooter,
    );
    rx -= ui.dp(6.0);

    // `详情` toggle
    let det_w = ui.measure_button_w(Some("panel"), "详情", state.cfg.show_preview);
    rx -= det_w;
    ui.draw_button(
        Rect::new(rx, by, det_w, btn_h),
        Some("panel"),
        "详情",
        if state.cfg.show_preview {
            theme::ACCENT
        } else {
            theme::TEXT_MUTED
        },
        if state.cfg.show_preview {
            theme::CARD_SELECTED
        } else {
            theme::CARD_BG
        },
        theme::CARD_HOVER,
        if state.cfg.show_preview {
            theme::BORDER_SOFT_SEL
        } else {
            theme::BORDER_DIM
        },
        state.cfg.show_preview,
        HitAction::TogglePreview,
    );
    rx -= ui.dp(6.0);

    // Running count badge
    let snap = state.tracker.snapshot();
    let run_cnt = snap.iter().filter(|p| p.running).count();
    let run_txt = format!("{run_cnt}");
    let run_tw = ui.c.measure_text_width(ui.c.fonts.bold_11, &run_txt);
    let run_w = run_tw + ui.dp(24.0);
    rx -= run_w;
    let run_r = Rect::new(rx, inner.y + (inner.h - ui.dp(26.0)) / 2, run_w, ui.dp(26.0));
    ui.c.fill_round_rect(
        run_r,
        ui.dpf(6.0),
        Some(if run_cnt > 0 {
            theme::GREEN_BG
        } else {
            theme::CARD_BG
        }),
        1.0,
        if run_cnt > 0 {
            Rgb::hex(0xA7F3D0)
        } else {
            theme::BORDER_DIM
        },
    );
    ui.c.draw_circle(
        (run_r.x + ui.dp(10.0)) as f32,
        (run_r.y + run_r.h / 2) as f32,
        ui.dpf(3.0),
        if run_cnt > 0 {
            theme::GREEN_FG
        } else {
            theme::TEXT_MUTED
        },
    );
    ui.c.draw_text_single(
        ui.c.fonts.bold_11,
        &run_txt,
        Rect::new(
            run_r.x + ui.dp(15.0),
            run_r.y,
            run_r.w - ui.dp(18.0),
            run_r.h,
        ),
        if run_cnt > 0 {
            theme::GREEN_FG
        } else {
            theme::TEXT_MUTED
        },
        TextAlign::Center,
    );
    rx -= ui.dp(6.0);

    // Pinned button `★ N`
    let pin_txt = format!("{}", state.cfg.pinned.len());
    let pin_w = ui.measure_button_w(Some("star"), &pin_txt, true);
    rx -= pin_w;
    ui.draw_button(
        Rect::new(rx, by, pin_w, btn_h),
        Some("star"),
        &pin_txt,
        theme::AMBER_FG,
        theme::AMBER_BG,
        Rgb::hex(0xFEF3C7),
        theme::AMBER_BORDER,
        true,
        HitAction::OpenPinnedView,
    );
    rx -= ui.dp(8.0);

    // Stats summary text
    let stats_txt = format!("{} 分类 · {} 工具", state.cat.categories.len(), state.cat.total_tools());
    let stats_w = ui.c.measure_text_width(ui.c.fonts.reg_11, &stats_txt) + ui.dp(4.0);
    if rx - stats_w - ui.dp(160.0) > lx {
        rx -= stats_w;
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            &stats_txt,
            Rect::new(rx, inner.y, stats_w, inner.h),
            theme::TEXT_SECONDARY,
            TextAlign::Center,
        );
        rx -= ui.dp(8.0);
    }

    // Search Box fills [lx .. rx]
    let search_w = (rx - lx).max(ui.dp(120.0));
    let search_r = Rect::new(lx, inner.y + ui.dp(2.0), search_w, inner.h - ui.dp(4.0));
    let sb_bg = if state.search_focused {
        theme::SURFACE_BG
    } else {
        theme::CARD_BG
    };
    let sb_bw = if state.search_focused {
        ui.dpf(2.0).max(2.0)
    } else {
        1.0
    };
    let sb_bc = if state.search_focused {
        theme::BORDER_ACTIVE
    } else {
        theme::BORDER_DIM
    };
    ui.c.fill_round_rect(search_r, ui.dpf(8.0), Some(sb_bg), sb_bw, sb_bc);

    let s_ico = ui.dp(14.0);
    ui.c.draw_ui_icon(
        "search",
        search_r.x + ui.dp(10.0),
        search_r.y + (search_r.h - s_ico) / 2,
        s_ico,
        if state.search_focused {
            theme::ACCENT
        } else {
            theme::TEXT_MUTED
        },
    );

    let clear_reserve = if !state.search_query.is_empty() {
        ui.dp(28.0)
    } else {
        ui.dp(8.0)
    };
    let text_r = Rect::new(
        search_r.x + ui.dp(30.0),
        search_r.y,
        (search_r.w - ui.dp(30.0) - clear_reserve).max(20),
        search_r.h,
    );
    if state.search_query.is_empty() {
        let ph = format!(
            "按 / 或 Ctrl+F 搜索 {} 款工具 (Prime95 / CrystalDisk / 天梯图)...",
            state.cat.total_tools()
        );
        ui.c.draw_text_single(
            ui.c.fonts.reg_12,
            &ph,
            text_r,
            theme::TEXT_MUTED,
            TextAlign::Left,
        );
    } else {
        ui.c.draw_text_single(
            ui.c.fonts.reg_125,
            &state.search_query,
            text_r,
            theme::TEXT_PRIMARY,
            TextAlign::Left,
        );
    }
    if state.search_focused {
        let cw = if state.search_query.is_empty() {
            0
        } else {
            ui.c.measure_text_width(ui.c.fonts.reg_125, &state.search_query)
                .min(text_r.w - 2)
        };
        let cx = text_r.x + cw + 1;
        ui.c.fill_rect(
            Rect::new(cx, search_r.y + ui.dp(6.0), ui.dp(1.5).max(1), search_r.h - ui.dp(12.0)),
            theme::ACCENT,
        );
    }
    ui.push_hit(search_r, HitAction::FocusSearch, true);

    if !state.search_query.is_empty() {
        let cr = Rect::new(
            search_r.right() - ui.dp(24.0),
            search_r.y + (search_r.h - ui.dp(18.0)) / 2,
            ui.dp(18.0),
            ui.dp(18.0),
        );
        let chov = ui.is_hovered(&HitAction::ClearSearch);
        ui.c.fill_round_rect(
            cr,
            ui.dpf(4.0),
            Some(if chov {
                theme::BORDER_DIM
            } else {
                theme::PILL_BG
            }),
            0.0,
            theme::PILL_BG,
        );
        ui.c.draw_ui_icon(
            "close",
            cr.x + ui.dp(4.0),
            cr.y + ui.dp(4.0),
            ui.dp(10.0),
            theme::TEXT_SECONDARY,
        );
        ui.push_hit(cr, HitAction::ClearSearch, true);
    }
}

fn render_sidebar(ui: &mut Ui, state: &mut AppState, r: Rect) {
    let focused = state.focus_panel == FocusPanel::Sidebar && !state.search_focused;
    ui.draw_panel(
        r,
        focused,
        theme::SURFACE_BG,
        Some(HitAction::FocusPanel(FocusPanel::Sidebar)),
    );

    let pad = ui.dp(10.0);
    let inner = r.inset(pad, pad);

    // Header row
    let hdr_h = ui.dp(20.0);
    let gsz = ui.dp(13.0);
    ui.c.draw_ui_icon(
        "grid",
        inner.x + ui.dp(4.0),
        inner.y + (hdr_h - gsz) / 2,
        gsz,
        theme::ACCENT,
    );
    ui.c.draw_text_single(
        ui.c.fonts.bold_12,
        "导航",
        Rect::new(inner.x + ui.dp(23.0), inner.y, inner.w - ui.dp(24.0), hdr_h),
        theme::TEXT_PRIMARY,
        TextAlign::Left,
    );

    let list_r = Rect::new(
        inner.x,
        inner.y + hdr_h + ui.dp(4.0),
        inner.w,
        (inner.bottom() - (inner.y + hdr_h + ui.dp(4.0))).max(20),
    );
    let entries = state.build_sidebar_entries();
    let header_cnt = entries.iter().filter(|e| e.is_header).count() as i32;
    let btn_cnt = entries.iter().filter(|e| !e.is_header).count() as i32;
    let gap = ui.dp(2.0);
    let header_h = ui.dp(18.0);
    let min_btn_h = ui.dp(24.0);
    let total_gaps = ((entries.len() as i32) - 1).max(0) * gap;
    let avail_for_btns = (list_r.h - header_cnt * header_h - total_gaps).max(0);
    let btn_h = if btn_cnt > 0 {
        (avail_for_btns / btn_cnt).max(min_btn_h)
    } else {
        min_btn_h
    };
    let content_h = header_cnt * header_h + btn_cnt * btn_h + total_gaps;

    ui.c.push_clip(list_r);
    let mut sy = list_r.y - state.scroll_sidebar.round() as i32;
    for (idx, item) in entries.iter().enumerate() {
        let item_h = if item.is_header { header_h } else { btn_h };
        let ir = Rect::new(list_r.x, sy, list_r.w, item_h);
        if ir.bottom() >= list_r.y && ir.y <= list_r.bottom() {
            if item.is_header {
                let lw = ui.c.measure_text_width(ui.c.fonts.bold_10, &item.label);
                ui.c.draw_text_single(
                    ui.c.fonts.bold_10,
                    &item.label,
                    Rect::new(ir.x + ui.dp(4.0), ir.y, lw + ui.dp(4.0), ir.h),
                    theme::TEXT_MUTED,
                    TextAlign::Left,
                );
                let div_x = ir.x + lw + ui.dp(12.0);
                if div_x < ir.right() {
                    ui.c.fill_rect(
                        Rect::new(div_x, ir.y + ir.h / 2, ir.right() - div_x, 1),
                        theme::BORDER_SUBTLE,
                    );
                }
            } else {
                let act = HitAction::SelectSidebar(idx);
                let hov = ui.is_hovered(&act);
                let (bg, bw, bc) = if item.selected {
                    if focused {
                        (theme::CARD_SELECTED, ui.dpf(1.5), theme::BORDER_ACTIVE)
                    } else {
                        (theme::CARD_SELECTED, 1.0, theme::BORDER_SOFT_SEL)
                    }
                } else if hov {
                    (theme::CARD_HOVER, 0.0, theme::CARD_HOVER)
                } else {
                    (theme::SURFACE_BG, 0.0, theme::SURFACE_BG)
                };
                if item.selected || hov {
                    ui.c.fill_round_rect(ir, ui.dpf(7.0), Some(bg), bw, bc);
                }
                if item.selected {
                    let bar_h = ui.dp(14.0).min(ir.h - ui.dp(4.0));
                    ui.c.fill_round_rect(
                        Rect::new(ir.x, ir.y + (ir.h - bar_h) / 2, ui.dp(3.0), bar_h),
                        ui.dpf(2.0),
                        Some(if focused {
                            theme::ACCENT
                        } else {
                            theme::ACCENT_LIGHT
                        }),
                        0.0,
                        theme::ACCENT,
                    );
                }

                let isz = ui.dp(14.0);
                let icol = if item.selected {
                    theme::ACCENT
                } else {
                    theme::TEXT_SECONDARY
                };
                ui.c.draw_ui_icon(
                    item.icon_key,
                    ir.x + ui.dp(10.0),
                    ir.y + (ir.h - isz) / 2,
                    isz,
                    icol,
                );

                let cnt_reserve = if item.show_count {
                    ui.dp(34.0)
                } else {
                    ui.dp(8.0)
                };
                let font = if item.selected {
                    ui.c.fonts.bold_12
                } else {
                    ui.c.fonts.reg_12
                };
                ui.c.draw_text_single(
                    font,
                    &item.label,
                    Rect::new(
                        ir.x + ui.dp(30.0),
                        ir.y,
                        (ir.w - ui.dp(30.0) - cnt_reserve).max(16),
                        ir.h,
                    ),
                    if item.selected {
                        theme::TEXT_PRIMARY
                    } else {
                        theme::TEXT_SECONDARY
                    },
                    TextAlign::Left,
                );

                if item.show_count {
                    let cs = format!("{}", item.count);
                    let cw = ui.dp(24.0);
                    let ch = ui.dp(17.0).min(ir.h - ui.dp(4.0));
                    let cr = Rect::new(
                        ir.right() - ui.dp(8.0) - cw,
                        ir.y + (ir.h - ch) / 2,
                        cw,
                        ch,
                    );
                    ui.draw_badge(
                        cr,
                        &cs,
                        if item.selected {
                            theme::ACCENT
                        } else {
                            theme::TEXT_MUTED
                        },
                        if item.selected {
                            theme::ACCENT_SOFT
                        } else {
                            theme::PILL_BG
                        },
                        true,
                    );
                }

                ui.push_hit(ir, act, true);
            }
        }
        sy += item_h + gap;
    }
    ui.c.pop_clip();
    ui.add_scrollbar(
        ScrollTarget::Sidebar,
        list_r,
        content_h,
        &mut state.scroll_sidebar,
    );
}

fn render_tools_main(ui: &mut Ui, state: &mut AppState, r: Rect) {
    let focused = state.focus_panel == FocusPanel::Main && !state.search_focused;
    ui.draw_panel(
        r,
        focused,
        theme::SURFACE_BG,
        Some(HitAction::FocusPanel(FocusPanel::Main)),
    );

    let pad = ui.dp(12.0);
    let inner = r.inset(pad, pad);
    let hdr_h = ui.dp(28.0);
    let hdr_r = Rect::new(inner.x, inner.y, inner.w, hdr_h);

    // Breadcrumb
    let mut bx = hdr_r.x;
    let b1 = "Trust工具箱";
    let b1w = ui.c.measure_text_width(ui.c.fonts.bold_13, b1);
    ui.c.draw_text_single(
        ui.c.fonts.bold_13,
        b1,
        Rect::new(bx, hdr_r.y, b1w + ui.dp(4.0), hdr_h),
        theme::TEXT_PRIMARY,
        TextAlign::Left,
    );
    bx += b1w + ui.dp(8.0);
    let slw = ui.c.measure_text_width(ui.c.fonts.bold_13, "/");
    ui.c.draw_text_single(
        ui.c.fonts.bold_13,
        "/",
        Rect::new(bx, hdr_r.y, slw + ui.dp(2.0), hdr_h),
        theme::TEXT_MUTED,
        TextAlign::Left,
    );
    bx += slw + ui.dp(8.0);

    let view_title = if !state.search_query.trim().is_empty() {
        format!("搜索: \"{}\"", state.search_query.trim())
    } else {
        state.current_view_title()
    };
    let vtw = ui.c.measure_text_width(ui.c.fonts.bold_13, &view_title).min(inner.w / 2);
    ui.c.draw_text_single(
        ui.c.fonts.bold_13,
        &view_title,
        Rect::new(bx, hdr_r.y, vtw + ui.dp(4.0), hdr_h),
        theme::TEXT_PRIMARY,
        TextAlign::Left,
    );
    bx += vtw + ui.dp(8.0);

    let count_str = if state.cached_tools.is_empty() {
        "0".to_string()
    } else {
        format!("{}/{}", state.selected_tool_idx + 1, state.cached_tools.len())
    };
    let cw = ui.measure_badge_w(&count_str, false);
    ui.draw_badge(
        Rect::new(bx, hdr_r.y + ui.dp(4.0), cw, ui.dp(20.0)),
        &count_str,
        theme::TEXT_SECONDARY,
        theme::PILL_BG,
        false,
    );

    // Sort button
    let sort_txt = format!("排序: {} (s)", sort_label(&state.cfg.sort_mode));
    let sort_w = ui.measure_button_w(Some("sort"), &sort_txt, false);
    ui.draw_button(
        Rect::new(hdr_r.right() - sort_w, hdr_r.y, sort_w, hdr_h),
        Some("sort"),
        &sort_txt,
        theme::TEXT_SLATE,
        theme::CARD_BG,
        theme::CARD_HOVER,
        theme::BORDER_DIM,
        false,
        HitAction::CycleSort,
    );

    // Column headers bar
    let col_y = hdr_r.bottom() + ui.dp(8.0);
    let col_h = ui.dp(24.0);
    let col_r = Rect::new(inner.x, col_y, inner.w, col_h);
    ui.c.fill_round_rect(col_r, ui.dpf(5.0), Some(theme::CARD_BG), 0.0, theme::CARD_BG);
    let size_w = ui.dp(66.0);
    let tag_w = ui.dp(74.0);
    let type_w = ui.dp(50.0);
    let right_cols = size_w + tag_w + type_w + ui.dp(32.0);
    ui.c.draw_text_single(
        ui.c.fonts.bold_11,
        "工具名称",
        Rect::new(col_r.x + ui.dp(10.0), col_r.y, (col_r.w - right_cols).max(40), col_h),
        theme::TEXT_SECONDARY,
        TextAlign::Left,
    );
    ui.c.draw_text_single(
        ui.c.fonts.bold_11,
        "类型",
        Rect::new(col_r.right() - ui.dp(16.0) - size_w - tag_w - type_w, col_r.y, type_w, col_h),
        theme::TEXT_SECONDARY,
        TextAlign::Center,
    );
    ui.c.draw_text_single(
        ui.c.fonts.bold_11,
        "标签",
        Rect::new(col_r.right() - ui.dp(12.0) - size_w - tag_w, col_r.y, tag_w, col_h),
        theme::TEXT_SECONDARY,
        TextAlign::Center,
    );
    ui.c.draw_text_single(
        ui.c.fonts.bold_11,
        "体积",
        Rect::new(col_r.right() - ui.dp(10.0) - size_w, col_r.y, size_w, col_h),
        theme::TEXT_SECONDARY,
        TextAlign::Right,
    );

    // Scrollable list viewport
    let list_y = col_r.bottom() + ui.dp(6.0);
    let list_r = Rect::new(inner.x, list_y, inner.w, (inner.bottom() - list_y).max(20));
    ui.list_viewport_rect = Some(list_r);

    if state.cached_tools.is_empty() {
        let eico = ui.dp(36.0);
        ui.c.draw_ui_icon(
            "search",
            list_r.x + (list_r.w - eico) / 2,
            list_r.y + list_r.h / 2 - ui.dp(36.0),
            eico,
            theme::TEXT_MUTED,
        );
        ui.c.draw_text_single(
            ui.c.fonts.bold_13,
            "没有匹配的工具",
            Rect::new(list_r.x, list_r.y + list_r.h / 2 + ui.dp(8.0), list_r.w, ui.dp(22.0)),
            theme::TEXT_SECONDARY,
            TextAlign::Center,
        );
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            "尝试更换关键词或按 Esc 清除搜索过滤条件",
            Rect::new(list_r.x, list_r.y + list_r.h / 2 + ui.dp(32.0), list_r.w, ui.dp(20.0)),
            theme::TEXT_MUTED,
            TextAlign::Center,
        );
        return;
    }

    let row_gap = ui.dp(4.0);
    let row_normal_h = ui.dp(40.0);
    let row_sel_h = ui.dp(64.0);
    let total_h = (state.cached_tools.len().saturating_sub(1) as i32) * (row_normal_h + row_gap) + row_sel_h;
    let bar_pad = if total_h > list_r.h { ui.dp(10.0) } else { 0 };
    let row_w = (list_r.w - bar_pad).max(120);

    ui.c.push_clip(list_r);
    let mut ry = list_r.y - state.scroll_list.round() as i32;
    let tool_indices = state.cached_tools.clone();
    let sel_idx = state.selected_tool_idx;
    let is_pinned_vec: Vec<bool> = tool_indices
        .iter()
        .map(|&idx| {
            state
                .cat
                .get_by_idx(idx)
                .is_some_and(|t| state.cfg.is_pinned(&t.id))
        })
        .collect();

    for (idx, &cat_idx) in tool_indices.iter().enumerate() {
        let sel = idx == sel_idx;
        let rh = if sel { row_sel_h } else { row_normal_h };
        let rr = Rect::new(list_r.x, ry, row_w, rh);

        if rr.bottom() >= list_r.y && rr.y <= list_r.bottom() {
            let Some(tool) = state.cat.get_by_idx(cat_idx).cloned() else {
                ry += rh + row_gap;
                continue;
            };
            let act = HitAction::SelectTool(idx);
            let hov = ui.is_hovered(&act);
            let (bg, bw, bc) = if sel {
                (
                    theme::CARD_SELECTED,
                    if focused { ui.dpf(1.5) } else { 1.0 },
                    if focused {
                        theme::BORDER_ACTIVE
                    } else {
                        theme::BORDER_SOFT_SEL
                    },
                )
            } else if hov {
                (theme::CARD_HOVER, 1.0, theme::BORDER_DIM)
            } else {
                (theme::SURFACE_BG, 1.0, theme::BORDER_SUBTLE)
            };
            ui.c.fill_round_rect(rr, ui.dpf(8.0), Some(bg), bw, bc);

            let isz = ui.dp(24.0);
            let icon_r = Rect::new(rr.x + ui.dp(10.0), rr.y + (rr.h - isz) / 2, isz, isz);
            ui.draw_tool_icon(&mut state.icon_cache, &tool, icon_r);

            // Right metadata
            let (badge_txt, badge_fg, badge_bg) = tool_kind_badge(&tool);
            let size_txt = if tool.kind == "tui" {
                "内置".to_string()
            } else {
                format_bytes(tool.total_bytes)
            };
            let sr = Rect::new(rr.right() - ui.dp(10.0) - size_w, rr.y, size_w, rr.h);
            ui.c.draw_text_single(
                ui.c.fonts.mono_11,
                &size_txt,
                sr,
                if sel {
                    theme::TEXT_SECONDARY
                } else {
                    theme::TEXT_MUTED
                },
                TextAlign::Right,
            );
            let tr = Rect::new(
                sr.x - ui.dp(6.0) - tag_w,
                rr.y,
                tag_w,
                rr.h,
            );
            ui.c.draw_text_single(
                ui.c.fonts.reg_11,
                &tool.tag,
                tr,
                if sel {
                    theme::ACCENT
                } else {
                    theme::TEXT_SECONDARY
                },
                TextAlign::Center,
            );
            let bdg_r = Rect::new(
                tr.x - ui.dp(6.0) - ui.dp(42.0),
                rr.y + (rr.h - ui.dp(20.0)) / 2,
                ui.dp(42.0),
                ui.dp(20.0),
            );
            ui.draw_badge(bdg_r, badge_txt, badge_fg, badge_bg, true);

            // Name & optional description
            let name_x = icon_r.right() + ui.dp(10.0);
            let name_max_x = bdg_r.x - ui.dp(8.0);
            let text_w = (name_max_x - name_x).max(30);
            let pinned = is_pinned_vec[idx];

            if sel {
                let mut nx = name_x;
                let top_y = rr.y + ui.dp(9.0);
                let line_h = ui.dp(20.0);
                if pinned {
                    let ssz = ui.dp(12.0);
                    ui.c.draw_ui_icon("star", nx, top_y + (line_h - ssz) / 2, ssz, theme::AMBER_FG);
                    nx += ssz + ui.dp(5.0);
                }
                ui.c.draw_text_single(
                    ui.c.fonts.bold_13,
                    &tool.name,
                    Rect::new(nx, top_y, (name_max_x - nx).max(20), line_h),
                    theme::TEXT_PRIMARY,
                    TextAlign::Left,
                );
                ui.c.draw_text_single(
                    ui.c.fonts.reg_11,
                    &tool.description,
                    Rect::new(name_x, top_y + ui.dp(22.0), text_w, ui.dp(18.0)),
                    theme::TEXT_SECONDARY,
                    TextAlign::Left,
                );
            } else {
                let mut nx = name_x;
                if pinned {
                    let ssz = ui.dp(12.0);
                    ui.c.draw_ui_icon(
                        "star",
                        nx,
                        rr.y + (rr.h - ssz) / 2,
                        ssz,
                        theme::AMBER_FG,
                    );
                    nx += ssz + ui.dp(5.0);
                }
                ui.c.draw_text_single(
                    ui.c.fonts.reg_125,
                    &tool.name,
                    Rect::new(nx, rr.y, (name_max_x - nx).max(20), rr.h),
                    theme::TEXT_PRIMARY,
                    TextAlign::Left,
                );
            }

            ui.push_hit(rr, act, true);
        }
        ry += rh + row_gap;
    }
    ui.c.pop_clip();
    ui.add_scrollbar(
        ScrollTarget::MainList,
        list_r,
        total_h,
        &mut state.scroll_list,
    );
}

fn render_hardware_main(ui: &mut Ui, state: &mut AppState, r: Rect) {
    let focused = state.focus_panel == FocusPanel::Main && !state.search_focused;
    ui.draw_panel(
        r,
        focused,
        theme::SURFACE_BG,
        Some(HitAction::FocusPanel(FocusPanel::Main)),
    );

    let inner = r.inset(ui.dp(14.0), ui.dp(14.0));
    let hdr_h = ui.dp(28.0);
    let hdr_r = Rect::new(inner.x, inner.y, inner.w, hdr_h);

    let isz = ui.dp(16.0);
    ui.c.draw_ui_icon(
        "dashboard",
        hdr_r.x,
        hdr_r.y + (hdr_h - isz) / 2,
        isz,
        theme::ACCENT,
    );
    let title = "本机硬件全景概览";
    let tw = ui.c.measure_text_width(ui.c.fonts.title_14b, title);
    ui.c.draw_text_single(
        ui.c.fonts.title_14b,
        title,
        Rect::new(hdr_r.x + ui.dp(24.0), hdr_r.y, tw + ui.dp(4.0), hdr_h),
        theme::TEXT_PRIMARY,
        TextAlign::Left,
    );

    let src_lbl = non_empty(&state.hw.source_label, "静态配置");
    let sb_w = ui.measure_badge_w(src_lbl, false);
    ui.draw_badge(
        Rect::new(hdr_r.x + ui.dp(24.0) + tw + ui.dp(8.0), hdr_r.y + ui.dp(4.0), sb_w, ui.dp(20.0)),
        src_lbl,
        theme::ACCENT,
        theme::ACCENT_SOFT,
        false,
    );

    // Action buttons right-aligned
    let mut rx = hdr_r.right();
    let specs: [(&str, &str, Rgb, Rgb, Rgb, Rgb, bool, HitAction); 4] = [
        ("copy", "复制 (c)", theme::TEXT_SLATE, theme::SURFACE_BG, theme::CARD_HOVER, theme::BORDER_DIM, false, HitAction::CopyHwInfo),
        ("export", "导出 (e)", theme::TEXT_SLATE, theme::SURFACE_BG, theme::CARD_HOVER, theme::BORDER_DIM, false, HitAction::ExportHwInfo),
        ("overview", "硬件检测 (d)", theme::TEXT_SLATE, theme::SURFACE_BG, theme::CARD_HOVER, theme::BORDER_DIM, false, HitAction::LaunchTulading),
        ("refresh", "重读 (r)", theme::ACCENT, theme::CARD_SELECTED, theme::ACCENT_SOFT, theme::BORDER_SOFT_SEL, true, HitAction::ReloadHwInfo),
    ];
    for (ico, lbl, fg, bg, hbg, bc, bold, act) in specs {
        let bw = ui.measure_button_w(Some(ico), lbl, bold);
        rx -= bw;
        ui.draw_button(
            Rect::new(rx, hdr_r.y, bw, hdr_h),
            Some(ico),
            lbl,
            fg,
            bg,
            hbg,
            bc,
            bold,
            act,
        );
        rx -= ui.dp(6.0);
    }

    ui.c.fill_rect(
        Rect::new(inner.x, hdr_r.bottom() + ui.dp(10.0), inner.w, 1),
        theme::BORDER_SUBTLE,
    );

    let vp = Rect::new(
        inner.x,
        hdr_r.bottom() + ui.dp(18.0),
        inner.w,
        (inner.bottom() - (hdr_r.bottom() + ui.dp(18.0))).max(20),
    );
    let cards = state.build_hw_cards();
    let two_cols = vp.w >= ui.dp(580.0);
    let bar_w = ui.dp(10.0);
    let usable_w = vp.w - bar_w;
    let col_gap = ui.dp(12.0);
    let col_w = if two_cols {
        (usable_w - col_gap) / 2
    } else {
        usable_w
    };

    let card_base = ui.dp(12.0) * 2 + ui.dp(22.0) + ui.dp(8.0) * 2 + 1;
    let card_row_h = ui.dp(22.0);
    let card_height = |card: &crate::app_state::HwCard| -> i32 {
        card_base + (card.rows.len().max(1) as i32) * card_row_h
    };

    let total_h = if two_cols {
        let mut h_sum = 0;
        for row_idx in 0..cards.len().div_ceil(2) {
            let h1 = card_height(&cards[row_idx * 2]);
            let h2 = cards.get(row_idx * 2 + 1).map(&card_height).unwrap_or(0);
            h_sum += h1.max(h2) + col_gap;
        }
        h_sum
    } else {
        cards.iter().map(&card_height).sum::<i32>() + (cards.len() as i32) * col_gap
    };

    ui.c.push_clip(vp);
    let mut cy = vp.y - state.scroll_hw.round() as i32;
    if two_cols {
        for row_idx in 0..cards.len().div_ceil(2) {
            let left = &cards[row_idx * 2];
            let right = cards.get(row_idx * 2 + 1);
            let rh = card_height(left).max(right.map(&card_height).unwrap_or(0));
            let lr = Rect::new(vp.x, cy, col_w, rh);
            if lr.bottom() >= vp.y && lr.y <= vp.bottom() {
                draw_hw_card(ui, left, lr);
                if let Some(rc) = right {
                    draw_hw_card(ui, rc, Rect::new(vp.x + col_w + col_gap, cy, col_w, rh));
                }
            }
            cy += rh + col_gap;
        }
    } else {
        for card in &cards {
            let rh = card_height(card);
            let cr = Rect::new(vp.x, cy, col_w, rh);
            if cr.bottom() >= vp.y && cr.y <= vp.bottom() {
                draw_hw_card(ui, card, cr);
            }
            cy += rh + col_gap;
        }
    }
    ui.c.pop_clip();
    ui.add_scrollbar(ScrollTarget::Hardware, vp, total_h, &mut state.scroll_hw);
}

fn draw_hw_card(ui: &mut Ui, card: &crate::app_state::HwCard, r: Rect) {
    ui.c.fill_round_rect(
        r,
        ui.dpf(8.0),
        Some(theme::CARD_BG),
        1.0,
        theme::BORDER_DIM,
    );
    let inr = r.inset(ui.dp(12.0), ui.dp(12.0));
    let hh = ui.dp(22.0);
    let isz = ui.dp(14.0);
    ui.c.draw_ui_icon(
        card.icon_key,
        inr.x,
        inr.y + (hh - isz) / 2,
        isz,
        theme::ACCENT,
    );
    let tw = ui.measure_badge_w(&card.tag, false);
    ui.c.draw_text_single(
        ui.c.fonts.bold_12,
        card.title,
        Rect::new(inr.x + ui.dp(20.0), inr.y, (inr.w - ui.dp(24.0) - tw).max(20), hh),
        theme::TEXT_PRIMARY,
        TextAlign::Left,
    );
    ui.draw_badge(
        Rect::new(inr.right() - tw, inr.y + 1, tw, ui.dp(20.0)),
        &card.tag,
        theme::ACCENT,
        theme::PILL_BG,
        false,
    );

    ui.c.fill_rect(
        Rect::new(inr.x, inr.y + hh + ui.dp(6.0), inr.w, 1),
        theme::BORDER_SUBTLE,
    );

    let mut ry = inr.y + hh + ui.dp(12.0);
    let kw = ui.dp(64.0);
    for (k, v) in &card.rows {
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            k,
            Rect::new(inr.x, ry, kw, ui.dp(18.0)),
            theme::TEXT_MUTED,
            TextAlign::Left,
        );
        ui.c.draw_text_single(
            ui.c.fonts.reg_12,
            v,
            Rect::new(inr.x + kw + ui.dp(6.0), ry, (inr.w - kw - ui.dp(6.0)).max(20), ui.dp(18.0)),
            theme::TEXT_PRIMARY,
            TextAlign::Left,
        );
        ry += ui.dp(22.0);
    }
}

fn render_hardware_detail(ui: &mut Ui, state: &mut AppState, r: Rect) {
    ui.draw_panel(r, false, theme::SURFACE_BG, None);
    let inr = r.inset(ui.dp(14.0), ui.dp(14.0));
    let top_h = ui.dp(34.0);
    ui.c.draw_text_single(
        ui.c.fonts.title_14b,
        "核心配置摘要",
        Rect::new(inr.x, inr.y, inr.w - ui.dp(50.0), ui.dp(18.0)),
        theme::TEXT_PRIMARY,
        TextAlign::Left,
    );
    let up_at = state.hw.updated_at_string();
    ui.c.draw_text_single(
        ui.c.fonts.reg_11,
        &up_at,
        Rect::new(inr.x, inr.y + ui.dp(18.0), inr.w - ui.dp(50.0), ui.dp(16.0)),
        theme::TEXT_MUTED,
        TextAlign::Left,
    );
    let bw = ui.measure_badge_w("静态", false);
    ui.draw_badge(
        Rect::new(inr.right() - bw, inr.y, bw, ui.dp(20.0)),
        "静态",
        theme::TEXT_SLATE,
        theme::PILL_BG,
        false,
    );
    ui.c.fill_rect(
        Rect::new(inr.x, inr.y + top_h + ui.dp(8.0), inr.w, 1),
        theme::BORDER_SUBTLE,
    );

    let mut y = inr.y + top_h + ui.dp(16.0);
    let (_, summary_gpu, _, _) = state.footer_hw_meta();
    let gpu = state
        .hw
        .gpus
        .first()
        .map(|g| format!("{} ({})", g.model.trim_start_matches("NVIDIA GeForce "), g.vram))
        .unwrap_or_else(|| "未记录".to_string());
    let disk = state
        .hw
        .disks
        .first()
        .map(|d| format!("{} ({})", d.model, d.capacity))
        .unwrap_or_else(|| "未记录".to_string());
    let mon = state
        .hw
        .monitors
        .first()
        .map(|m| format!("{} {}", m.model, m.size))
        .unwrap_or_else(|| "未记录".to_string());
    let _ = summary_gpu;
    let items = [
        ("处理器", non_empty(&state.hw.cpu_model, "-").to_string()),
        (
            "核心规格",
            format!(
                "{}核心 {}线程 · {}",
                non_empty(&state.hw.cpu_cores, "?"),
                non_empty(&state.hw.cpu_threads, "?"),
                non_empty(&state.hw.cpu_clock, "?")
            ),
        ),
        (
            "内存总计",
            format!(
                "{} ({} 条 {})",
                non_empty(&state.hw.total_memory_gb(), "?"),
                state.hw.memory_modules.len(),
                non_empty(&state.hw.memory_type, "RAM")
            ),
        ),
        ("主显卡", gpu),
        ("主硬盘", disk),
        ("显示器", mon),
    ];

    for (k, v) in items {
        let cr = Rect::new(inr.x, y, inr.w, ui.dp(46.0));
        ui.c.fill_round_rect(
            cr,
            ui.dpf(6.0),
            Some(theme::CARD_BG),
            1.0,
            theme::BORDER_DIM,
        );
        ui.c.draw_text_single(
            ui.c.fonts.reg_10,
            k,
            Rect::new(cr.x + ui.dp(10.0), cr.y + ui.dp(6.0), cr.w - ui.dp(20.0), ui.dp(14.0)),
            theme::TEXT_MUTED,
            TextAlign::Left,
        );
        ui.c.draw_text_single(
            ui.c.fonts.semi_125,
            &v,
            Rect::new(cr.x + ui.dp(10.0), cr.y + ui.dp(22.0), cr.w - ui.dp(20.0), ui.dp(18.0)),
            theme::TEXT_PRIMARY,
            TextAlign::Left,
        );
        y += ui.dp(52.0);
    }

    let note_r = Rect::new(inr.x, y + ui.dp(4.0), inr.w, ui.dp(56.0));
    if note_r.bottom() <= inr.bottom() {
        ui.c.fill_round_rect(
            note_r,
            ui.dpf(8.0),
            Some(theme::CARD_SELECTED),
            1.0,
            theme::BORDER_SOFT_SEL,
        );
        ui.c.draw_text_wrap(
            ui.c.fonts.reg_11,
            "零后台驻留：静态配置来自 hwinfo.ini；点击「硬件检测 (d)」可调用图拉丁实时识别。",
            note_r.inset(ui.dp(10.0), ui.dp(10.0)),
            theme::TEXT_PRIMARY,
        );
    }
}

fn render_tool_detail(ui: &mut Ui, state: &mut AppState, r: Rect) {
    ui.draw_panel(r, false, theme::SURFACE_BG, None);
    let Some(tool) = state.selected_tool().cloned() else {
        let eico = ui.dp(28.0);
        ui.c.draw_ui_icon(
            "wrench",
            r.x + (r.w - eico) / 2,
            r.y + r.h / 2 - ui.dp(28.0),
            eico,
            theme::TEXT_MUTED,
        );
        ui.c.draw_text_single(
            ui.c.fonts.bold_13,
            "工具详情",
            Rect::new(r.x, r.y + r.h / 2 + ui.dp(4.0), r.w, ui.dp(20.0)),
            theme::TEXT_SECONDARY,
            TextAlign::Center,
        );
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            "请在左侧列表中选择工具",
            Rect::new(r.x, r.y + r.h / 2 + ui.dp(26.0), r.w, ui.dp(18.0)),
            theme::TEXT_MUTED,
            TextAlign::Center,
        );
        return;
    };

    let vp = r.inset(ui.dp(14.0), ui.dp(14.0));
    let cw = (vp.w - ui.dp(10.0)).max(120);
    let mut y = vp.y - state.scroll_detail.round() as i32;
    let start_y = y;

    ui.c.push_clip(vp);

    // Header: 44x44 icon + title + badges
    let ibox = ui.dp(44.0);
    let icon_bg = Rect::new(vp.x, y, ibox, ibox);
    ui.c.fill_round_rect(
        icon_bg,
        ui.dpf(10.0),
        Some(theme::CARD_BG),
        1.0,
        theme::BORDER_DIM,
    );
    ui.draw_tool_icon(&mut state.icon_cache, &tool, icon_bg.inset(ui.dp(8.0), ui.dp(8.0)));

    let tx = icon_bg.right() + ui.dp(12.0);
    let tw = (cw - ibox - ui.dp(12.0)).max(40);
    ui.c.draw_text_single(
        ui.c.fonts.hero_16b,
        &tool.name,
        Rect::new(tx, y + ui.dp(2.0), tw, ui.dp(22.0)),
        theme::TEXT_PRIMARY,
        TextAlign::Left,
    );

    let (badge_txt, badge_fg, badge_bg) = tool_kind_badge(&tool);
    let pinned = state.cfg.is_pinned(&tool.id);
    let mut bx = tx;
    let by = y + ui.dp(27.0);
    let bh = ui.dp(18.0);
    if pinned {
        let pw = ui.measure_badge_w("★ 已收藏", true);
        if bx + pw <= tx + tw {
            ui.draw_badge(
                Rect::new(bx, by, pw, bh),
                "★ 已收藏",
                theme::AMBER_FG,
                theme::AMBER_BG,
                true,
            );
            bx += pw + ui.dp(5.0);
        }
    }
    let bdg_w = ui.measure_badge_w(badge_txt, true);
    if bx + bdg_w <= tx + tw {
        ui.draw_badge(Rect::new(bx, by, bdg_w, bh), badge_txt, badge_fg, badge_bg, true);
        bx += bdg_w + ui.dp(5.0);
    }
    let cat_w = ui.measure_badge_w(&tool.category_name, false);
    if bx + cat_w <= tx + tw {
        ui.draw_badge(
            Rect::new(bx, by, cat_w, bh),
            &tool.category_name,
            theme::TEXT_SECONDARY,
            theme::PILL_BG,
            false,
        );
    }
    y += ibox + ui.dp(12.0);

    // Metadata box
    let meta_r = Rect::new(vp.x, y, cw, ui.dp(92.0));
    ui.c.fill_round_rect(
        meta_r,
        ui.dpf(8.0),
        Some(theme::CARD_BG),
        1.0,
        theme::BORDER_DIM,
    );
    let entry_base = if tool.kind == "tui" {
        "全屏像素坏点检测器".to_string()
    } else {
        tool.entry_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string()
    };
    let variant_desc = if tool.kind == "tui" {
        "内置原生模块".to_string()
    } else if tool.variants.len() > 1 {
        format!("{} 个版本 (按 b 切换)", tool.variants.len())
    } else {
        "单入口直达".to_string()
    };
    let size_txt = if tool.kind == "tui" {
        "内置模块".to_string()
    } else {
        format!("{} · {} 项", format_bytes(tool.total_bytes), tool.file_count)
    };
    let rel_dir = tool
        .rel_dir
        .trim_start_matches("tools/")
        .trim_start_matches("tools\\");

    let meta_rows = [
        ("入口:", entry_base.as_str(), true),
        ("版本:", variant_desc.as_str(), false),
        ("体积:", size_txt.as_str(), false),
        ("目录:", rel_dir, false),
    ];
    let mut my = meta_r.y + ui.dp(8.0);
    for (i, (k, v, bold_mono)) in meta_rows.iter().enumerate() {
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            k,
            Rect::new(meta_r.x + ui.dp(10.0), my, ui.dp(42.0), ui.dp(18.0)),
            theme::TEXT_MUTED,
            TextAlign::Left,
        );
        let font = if *bold_mono {
            ui.c.fonts.mono_11b
        } else {
            ui.c.fonts.reg_11
        };
        let col = if *bold_mono {
            theme::TEXT_PRIMARY
        } else if i == 3 {
            theme::TEXT_MUTED
        } else {
            theme::TEXT_SECONDARY
        };
        ui.c.draw_text_single(
            font,
            v,
            Rect::new(meta_r.x + ui.dp(56.0), my, meta_r.w - ui.dp(66.0), ui.dp(18.0)),
            col,
            TextAlign::Left,
        );
        my += ui.dp(19.0);
    }
    y = meta_r.bottom() + ui.dp(12.0);

    // Action buttons row
    let act_h = ui.dp(28.0);
    let gap = ui.dp(6.0);
    let is_tui = tool.kind == "tui";
    let show_ver = !is_tui && tool.variants.len() > 1;
    let col_cnt = if is_tui { 2 } else if show_ver { 5 } else { 4 };
    let btn_w = (cw - gap * (col_cnt - 1)) / col_cnt;
    let mut ax = vp.x;
    ui.draw_button(
        Rect::new(ax, y, btn_w, act_h),
        Some("play"),
        "启动",
        theme::TEXT_WHITE,
        theme::ACCENT,
        theme::ACCENT_DARK,
        theme::ACCENT,
        true,
        HitAction::LaunchSelected(false),
    );
    ax += btn_w + gap;
    if !is_tui {
        ui.draw_button(
            Rect::new(ax, y, btn_w, act_h),
            Some("shield"),
            "提权",
            theme::TEXT_SLATE,
            theme::SURFACE_BG,
            theme::CARD_HOVER,
            theme::BORDER_DIM,
            false,
            HitAction::LaunchSelected(true),
        );
        ax += btn_w + gap;
        ui.draw_button(
            Rect::new(ax, y, btn_w, act_h),
            Some("folder"),
            "目录",
            theme::TEXT_SLATE,
            theme::SURFACE_BG,
            theme::CARD_HOVER,
            theme::BORDER_DIM,
            false,
            HitAction::OpenToolDir,
        );
        ax += btn_w + gap;
    }
    ui.draw_button(
        Rect::new(ax, y, btn_w, act_h),
        Some("star"),
        if pinned { "已藏" } else { "收藏" },
        if pinned { theme::AMBER_FG } else { theme::TEXT_SLATE },
        if pinned { theme::AMBER_BG } else { theme::SURFACE_BG },
        theme::CARD_HOVER,
        if pinned { theme::AMBER_BORDER } else { theme::BORDER_DIM },
        false,
        HitAction::TogglePin,
    );
    if show_ver {
        ax += btn_w + gap;
        ui.draw_button(
            Rect::new(ax, y, btn_w, act_h),
            Some("wrench"),
            "版本",
            theme::TEXT_SLATE,
            theme::SURFACE_BG,
            theme::CARD_HOVER,
            theme::BORDER_DIM,
            false,
            HitAction::OpenVariants,
        );
    }
    y += act_h + ui.dp(12.0);

    // Description
    let desc_h = ui.c.draw_text_wrap(
        ui.c.fonts.reg_12,
        &tool.description,
        Rect::new(vp.x, y, cw, ui.dp(160.0)),
        theme::TEXT_PRIMARY,
    );
    y += desc_h + ui.dp(12.0);

    // Warning Callout
    if !tool.warning.is_empty() {
        let is_danger = tool.warning.contains("危险") || tool.warning.contains("极限");
        let acc = if is_danger { theme::RED_FG } else { theme::AMBER_FG };
        let cbg = if is_danger { theme::RED_BG } else { theme::AMBER_BG };
        let body_h = ui.c.measure_text_wrap_height(ui.c.fonts.reg_12, &tool.warning, cw - ui.dp(26.0));
        let card_h = ui.dp(46.0) + body_h;
        let cr = Rect::new(vp.x, y, cw, card_h);
        ui.c.fill_round_rect(cr, ui.dpf(8.0), Some(cbg), 1.0, acc);
        ui.c.fill_rect(Rect::new(cr.x + 1, cr.y + 3, ui.dp(3.0), cr.h - 6), acc);
        ui.c.draw_ui_icon("warn", cr.x + ui.dp(12.0), cr.y + ui.dp(11.0), ui.dp(14.0), acc);
        ui.c.draw_text_single(
            ui.c.fonts.bold_12,
            if is_danger { "高危操作警示" } else { "注意与提示" },
            Rect::new(cr.x + ui.dp(31.0), cr.y + ui.dp(9.0), cw - ui.dp(40.0), ui.dp(18.0)),
            acc,
            TextAlign::Left,
        );
        ui.c.draw_text_wrap(
            ui.c.fonts.reg_12,
            &tool.warning,
            Rect::new(cr.x + ui.dp(13.0), cr.y + ui.dp(32.0), cw - ui.dp(24.0), body_h + ui.dp(4.0)),
            theme::TEXT_PRIMARY,
        );
        y += card_h + ui.dp(10.0);
    }

    // Encrypted Archive Password Callout
    if !tool.password_note.is_empty() {
        let body_h =
            ui.c.measure_text_wrap_height(ui.c.fonts.reg_12, &tool.password_note, cw - ui.dp(26.0));
        let card_h = ui.dp(48.0) + body_h;
        let cr = Rect::new(vp.x, y, cw, card_h);
        ui.c.fill_round_rect(cr, ui.dpf(8.0), Some(theme::CARD_SELECTED), 1.0, theme::BORDER_SOFT_SEL);
        ui.c.fill_rect(Rect::new(cr.x + 1, cr.y + 3, ui.dp(3.0), cr.h - 6), theme::ACCENT);
        ui.c.draw_ui_icon("lock", cr.x + ui.dp(12.0), cr.y + ui.dp(11.0), ui.dp(14.0), theme::ACCENT);
        let cpw = ui.measure_button_w(Some("copy"), "复制 (w)", true);
        ui.c.draw_text_single(
            ui.c.fonts.bold_12,
            "解压密码与说明 (w)",
            Rect::new(cr.x + ui.dp(31.0), cr.y + ui.dp(9.0), (cw - ui.dp(44.0) - cpw).max(30), ui.dp(18.0)),
            theme::ACCENT,
            TextAlign::Left,
        );
        ui.draw_button(
            Rect::new(cr.right() - ui.dp(10.0) - cpw, cr.y + ui.dp(7.0), cpw, ui.dp(22.0)),
            Some("copy"),
            "复制 (w)",
            theme::TEXT_WHITE,
            theme::ACCENT,
            theme::ACCENT_DARK,
            theme::ACCENT,
            true,
            HitAction::CopyPassword,
        );
        ui.c.draw_text_wrap(
            ui.c.fonts.reg_12,
            &tool.password_note,
            Rect::new(cr.x + ui.dp(13.0), cr.y + ui.dp(34.0), cw - ui.dp(24.0), body_h + ui.dp(4.0)),
            theme::TEXT_PRIMARY,
        );
        y += card_h + ui.dp(10.0);
    }

    // Image Preview Card (for Ladder Images decoded lazily via GDI+)
    if tool.kind == "img" && !tool.entry_path.as_os_str().is_empty() {
        let pr_h = ui.dp(250.0);
        let pr = Rect::new(vp.x, y, cw, pr_h);
        ui.c.fill_round_rect(pr, ui.dpf(8.0), Some(theme::CARD_BG), 1.0, theme::BORDER_DIM);
        ui.c.draw_text_single(
            ui.c.fonts.bold_12,
            "天梯图预览 (回车打开大图)",
            Rect::new(pr.x + ui.dp(10.0), pr.y + ui.dp(8.0), cw - ui.dp(90.0), ui.dp(18.0)),
            theme::ACCENT,
            TextAlign::Left,
        );
        if let Some((bmp, ow, oh)) = state.preview_cache.get_cached(&tool.entry_path, 480) {
            let dim = format!("{ow}x{oh}");
            ui.c.draw_text_single(
                ui.c.fonts.mono_11,
                &dim,
                Rect::new(pr.right() - ui.dp(86.0), pr.y + ui.dp(8.0), ui.dp(76.0), ui.dp(18.0)),
                theme::TEXT_MUTED,
                TextAlign::Right,
            );
            let box_r = Rect::new(pr.x + ui.dp(10.0), pr.y + ui.dp(32.0), cw - ui.dp(20.0), ui.dp(206.0));
            let s = (box_r.w as f32 / bmp.width.max(1) as f32)
                .min(box_r.h as f32 / bmp.height.max(1) as f32)
                .min(1.0);
            let dw = ((bmp.width as f32 * s).round() as i32).max(1);
            let dh = ((bmp.height as f32 * s).round() as i32).max(1);
            ui.c.draw_bitmap(
                &bmp,
                Rect::new(box_r.x + (box_r.w - dw) / 2, box_r.y + (box_r.h - dh) / 2, dw, dh),
            );
        } else {
            ui.c.draw_text_single(
                ui.c.fonts.reg_11,
                "正在解码预览图像...",
                Rect::new(pr.x, pr.y + ui.dp(120.0), pr.w, ui.dp(20.0)),
                theme::TEXT_MUTED,
                TextAlign::Center,
            );
        }
        y += pr_h + ui.dp(12.0);
    }

    // Directory Files listing
    if !tool.files.is_empty() {
        ui.c.fill_rect(Rect::new(vp.x, y, cw, 1), theme::BORDER_SUBTLE);
        y += ui.dp(10.0);
        ui.c.draw_ui_icon("folder", vp.x, y + ui.dp(2.0), ui.dp(13.0), theme::ACCENT);
        ui.c.draw_text_single(
            ui.c.fonts.bold_12,
            "目录文件",
            Rect::new(vp.x + ui.dp(18.0), y, ui.dp(90.0), ui.dp(18.0)),
            theme::TEXT_PRIMARY,
            TextAlign::Left,
        );
        let fsum = format!("{} 项 · {}", tool.file_count, format_bytes(tool.total_bytes));
        ui.c.draw_text_single(
            ui.c.fonts.mono_11,
            &fsum,
            Rect::new(vp.x + ui.dp(100.0), y, (cw - ui.dp(100.0)).max(20), ui.dp(18.0)),
            theme::TEXT_MUTED,
            TextAlign::Right,
        );
        y += ui.dp(24.0);

        let max_sz = tool
            .files
            .iter()
            .filter(|f| !f.is_dir)
            .map(|f| f.size)
            .max()
            .unwrap_or(1)
            .max(1);

        let fr_h = ui.dp(24.0);
        for f in &tool.files {
            if y + fr_h >= vp.y && y <= vp.bottom() {
                let fr = Rect::new(vp.x, y, cw, fr_h);
                ui.c.fill_round_rect(fr, ui.dpf(5.0), Some(theme::CARD_BG), 0.0, theme::CARD_BG);
                let dot_col = if f.is_dir {
                    theme::ACCENT
                } else if f.name.to_lowercase().ends_with(".exe") {
                    theme::GREEN_FG
                } else {
                    theme::TEXT_MUTED
                };
                ui.c.draw_circle(
                    (fr.x + ui.dp(10.0)) as f32,
                    (fr.y + fr.h / 2) as f32,
                    ui.dpf(3.0),
                    dot_col,
                );
                let sz_txt = if f.is_dir {
                    "<DIR>".to_string()
                } else {
                    format_bytes(f.size)
                };
                let bar_w = ui.dp(34.0);
                let sz_w = ui.dp(56.0);
                ui.c.draw_text_single(
                    ui.c.fonts.reg_11,
                    &f.name,
                    Rect::new(fr.x + ui.dp(19.0), fr.y, (fr.w - ui.dp(32.0) - sz_w - bar_w).max(20), fr_h),
                    theme::TEXT_PRIMARY,
                    TextAlign::Left,
                );
                ui.c.draw_text_single(
                    ui.c.fonts.mono_11,
                    &sz_txt,
                    Rect::new(fr.right() - ui.dp(14.0) - bar_w - sz_w, fr.y, sz_w, fr_h),
                    theme::TEXT_MUTED,
                    TextAlign::Right,
                );
                let track = Rect::new(
                    fr.right() - ui.dp(8.0) - bar_w,
                    fr.y + (fr_h - ui.dp(5.0)) / 2,
                    bar_w,
                    ui.dp(5.0),
                );
                ui.c.fill_round_rect(track, ui.dpf(2.5), Some(theme::BORDER_DIM), 0.0, theme::BORDER_DIM);
                if !f.is_dir && f.size > 0 {
                    let ratio = (f.size as f32 / max_sz as f32).clamp(0.08, 1.0);
                    let fw = ((bar_w as f32 * ratio).round() as i32).max(ui.dp(3.0));
                    ui.c.fill_round_rect(
                        Rect::new(track.x, track.y, fw, track.h),
                        ui.dpf(2.5),
                        Some(dot_col),
                        0.0,
                        dot_col,
                    );
                }
            }
            y += fr_h + ui.dp(3.0);
        }
    }

    ui.c.pop_clip();
    ui.add_scrollbar(
        ScrollTarget::Detail,
        vp,
        (y - start_y).max(1),
        &mut state.scroll_detail,
    );
}

fn render_footer_dock(ui: &mut Ui, state: &mut AppState, r: Rect) {
    let gap = ui.dp(10.0);
    let w = r.w;
    let proc_w = ((w as f32 * 0.37).round() as i32).max(ui.dp(240.0));
    let fav_w = ((w as f32 * 0.27).round() as i32).max(ui.dp(190.0));
    let hw_w = (w - proc_w - fav_w - gap * 2).max(ui.dp(180.0));

    let proc_r = Rect::new(r.x, r.y, proc_w, r.h);
    let hw_r = Rect::new(proc_r.right() + gap, r.y, hw_w, r.h);
    let fav_r = Rect::new(hw_r.right() + gap, r.y, (r.right() - hw_r.right() - gap).max(100), r.h);

    // 1. Process Monitor
    let pfocused = state.focus_panel == FocusPanel::Processes && !state.search_focused;
    ui.draw_panel(
        proc_r,
        pfocused,
        theme::SURFACE_BG,
        Some(HitAction::FocusPanel(FocusPanel::Processes)),
    );
    let pinr = proc_r.inset(ui.dp(12.0), ui.dp(10.0));
    let snap: Vec<TrackedProcessSnapshot> = state.tracker.snapshot();
    let run_cnt = snap.iter().filter(|p| p.running).count();

    let hdr_h = ui.dp(24.0);
    ui.c.draw_ui_icon(
        "terminal",
        pinr.x,
        pinr.y + (hdr_h - ui.dp(13.0)) / 2,
        ui.dp(13.0),
        theme::ACCENT,
    );
    ui.c.draw_text_single(
        ui.c.fonts.bold_12,
        "进程监控",
        Rect::new(pinr.x + ui.dp(19.0), pinr.y, ui.dp(60.0), hdr_h),
        theme::TEXT_PRIMARY,
        TextAlign::Left,
    );
    let rtxt = format!("{run_cnt} 运行");
    let rbw = ui.measure_badge_w(&rtxt, false);
    ui.draw_badge(
        Rect::new(pinr.x + ui.dp(80.0), pinr.y + ui.dp(2.0), rbw, ui.dp(20.0)),
        &rtxt,
        if run_cnt > 0 { theme::GREEN_FG } else { theme::TEXT_MUTED },
        if run_cnt > 0 { theme::GREEN_BG } else { theme::PILL_BG },
        true,
    );

    let mut bx = pinr.right();
    for (lbl, act) in [
        ("清理", HitAction::ClearExitedProcesses),
        ("全关", HitAction::KillAllProcesses),
        ("结束", HitAction::KillSelectedProcess),
    ] {
        let bw = ui.measure_button_w(None, lbl, false);
        bx -= bw;
        ui.draw_button(
            Rect::new(bx, pinr.y + 1, bw, ui.dp(22.0)),
            None,
            lbl,
            theme::TEXT_SLATE,
            theme::SURFACE_BG,
            theme::CARD_HOVER,
            theme::BORDER_DIM,
            false,
            act,
        );
        bx -= ui.dp(4.0);
    }

    let list_r = Rect::new(
        pinr.x,
        pinr.y + hdr_h + ui.dp(8.0),
        pinr.w,
        (pinr.bottom() - (pinr.y + hdr_h + ui.dp(8.0))).max(16),
    );
    if snap.is_empty() {
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            "尚无已启动工具进程",
            Rect::new(list_r.x, list_r.y + ui.dp(12.0), list_r.w, ui.dp(18.0)),
            theme::TEXT_MUTED,
            TextAlign::Left,
        );
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            "选中工具双击或按 Enter 启动 · x 结束 / X 全关",
            Rect::new(list_r.x, list_r.y + ui.dp(34.0), list_r.w, ui.dp(18.0)),
            theme::TEXT_SECONDARY,
            TextAlign::Left,
        );
    } else {
        let row_h = ui.dp(22.0);
        let row_gap = ui.dp(2.0);
        let total_h = snap.len() as i32 * (row_h + row_gap);
        ui.c.push_clip(list_r);
        let mut py = list_r.y - state.scroll_proc.round() as i32;
        for (i, p) in snap.iter().enumerate() {
            let pr = Rect::new(list_r.x, py, list_r.w - ui.dp(10.0), row_h);
            if pr.bottom() >= list_r.y && pr.y <= list_r.bottom() {
                let sel = i == state.selected_proc_idx;
                let act = HitAction::SelectProcess(i);
                let hov = ui.is_hovered(&act);
                let (bg, bw, bc) = if sel {
                    (
                        theme::CARD_SELECTED,
                        if pfocused { ui.dpf(1.5) } else { 1.0 },
                        if pfocused { theme::BORDER_ACTIVE } else { theme::BORDER_SOFT_SEL },
                    )
                } else if hov {
                    (theme::CARD_HOVER, 0.0, theme::CARD_HOVER)
                } else {
                    (theme::SURFACE_BG, 0.0, theme::SURFACE_BG)
                };
                if sel || hov {
                    ui.c.fill_round_rect(pr, ui.dpf(5.0), Some(bg), bw, bc);
                }
                let dcol = if p.running { theme::GREEN_FG } else { theme::TEXT_MUTED };
                ui.c.draw_circle((pr.x + ui.dp(8.0)) as f32, (pr.y + row_h / 2) as f32, ui.dpf(3.0), dcol);
                let st_txt = if p.external {
                    if p.admin { "提权托管".to_string() } else { "外壳托管".to_string() }
                } else if p.running {
                    format!("PID {}", p.pid)
                } else {
                    "已退出".to_string()
                };
                let sw = ui.dp(66.0);
                ui.c.draw_text_single(
                    if sel { ui.c.fonts.bold_11 } else { ui.c.fonts.reg_11 },
                    &p.tool_name,
                    Rect::new(pr.x + ui.dp(18.0), pr.y, (pr.w - ui.dp(24.0) - sw).max(20), row_h),
                    theme::TEXT_PRIMARY,
                    TextAlign::Left,
                );
                ui.c.draw_text_single(
                    ui.c.fonts.mono_11,
                    &st_txt,
                    Rect::new(pr.right() - sw - ui.dp(6.0), pr.y, sw, row_h),
                    dcol,
                    TextAlign::Right,
                );
                ui.push_hit(pr, act, true);
            }
            py += row_h + row_gap;
        }
        ui.c.pop_clip();
        ui.add_scrollbar(ScrollTarget::Processes, list_r, total_h, &mut state.scroll_proc);
    }

    // 2. Hardware Meta Card
    ui.draw_panel(hw_r, false, theme::SURFACE_BG, None);
    let hinr = hw_r.inset(ui.dp(12.0), ui.dp(10.0));
    ui.c.draw_ui_icon(
        "dashboard",
        hinr.x,
        hinr.y + (hdr_h - ui.dp(13.0)) / 2,
        ui.dp(13.0),
        theme::ACCENT,
    );
    ui.c.draw_text_single(
        ui.c.fonts.bold_12,
        "配置与属性",
        Rect::new(hinr.x + ui.dp(19.0), hinr.y, ui.dp(90.0), hdr_h),
        theme::TEXT_PRIMARY,
        TextAlign::Left,
    );
    let sbw = ui.measure_badge_w("STATIC", false);
    ui.draw_badge(
        Rect::new(hinr.right() - sbw, hinr.y + ui.dp(2.0), sbw, ui.dp(20.0)),
        "STATIC",
        theme::TEXT_MUTED,
        theme::PILL_BG,
        false,
    );
    let (mcpu, mgpu, m3l, m3v) = state.footer_hw_meta();
    let mut hy = hinr.y + hdr_h + ui.dp(8.0);
    for (k, v) in [("CPU:", mcpu), ("GPU:", mgpu), (m3l, m3v)] {
        ui.c.draw_text_single(
            ui.c.fonts.bold_11,
            k,
            Rect::new(hinr.x, hy, ui.dp(40.0), ui.dp(18.0)),
            theme::TEXT_MUTED,
            TextAlign::Left,
        );
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            &v,
            Rect::new(hinr.x + ui.dp(44.0), hy, (hinr.w - ui.dp(44.0)).max(20), ui.dp(18.0)),
            theme::TEXT_PRIMARY,
            TextAlign::Left,
        );
        hy += ui.dp(22.0);
    }

    // 3. Pinned Quick Launch [1-5]
    ui.draw_panel(fav_r, false, theme::SURFACE_BG, None);
    let finr = fav_r.inset(ui.dp(12.0), ui.dp(10.0));
    ui.c.draw_ui_icon(
        "star",
        finr.x,
        finr.y + (hdr_h - ui.dp(13.0)) / 2,
        ui.dp(13.0),
        theme::AMBER_FG,
    );
    ui.c.draw_text_single(
        ui.c.fonts.bold_12,
        "快捷收藏 [1-5]",
        Rect::new(finr.x + ui.dp(19.0), finr.y, ui.dp(110.0), hdr_h),
        theme::TEXT_PRIMARY,
        TextAlign::Left,
    );
    let qitems: Vec<(usize, String, String)> = state
        .cfg
        .pinned
        .iter()
        .filter_map(|id| state.cat.get(id))
        .take(5)
        .enumerate()
        .map(|(i, t)| (i, t.name.clone(), t.category_name.clone()))
        .collect();
    let qcnt_txt = format!("{} 项", state.cfg.pinned.len());
    let qbw = ui.measure_badge_w(&qcnt_txt, false);
    ui.draw_badge(
        Rect::new(finr.right() - qbw, finr.y + ui.dp(2.0), qbw, ui.dp(20.0)),
        &qcnt_txt,
        theme::AMBER_FG,
        theme::AMBER_BG,
        true,
    );

    let mut fy = finr.y + hdr_h + ui.dp(6.0);
    let frow_h = ui.dp(18.0);
    if qitems.is_empty() {
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            "按 p 收藏工具 · 1~5 直达",
            Rect::new(finr.x, fy + ui.dp(16.0), finr.w, ui.dp(18.0)),
            theme::TEXT_MUTED,
            TextAlign::Center,
        );
    } else {
        for (slot, name, cat_name) in qitems {
            if fy + frow_h > finr.bottom() {
                break;
            }
            let fr = Rect::new(finr.x, fy, finr.w, frow_h);
            let act = HitAction::JumpPinnedSlot(slot);
            if ui.is_hovered(&act) {
                ui.c.fill_round_rect(fr, ui.dpf(4.0), Some(theme::CARD_HOVER), 0.0, theme::CARD_HOVER);
            }
            let slot_r = Rect::new(fr.x, fr.y + 1, ui.dp(18.0), ui.dp(16.0));
            ui.draw_badge(slot_r, &format!("{}", slot + 1), theme::ACCENT, theme::ACCENT_SOFT, true);
            let cw = ui.dp(64.0);
            ui.c.draw_text_single(
                ui.c.fonts.reg_11,
                &name,
                Rect::new(slot_r.right() + ui.dp(6.0), fr.y, (fr.w - ui.dp(28.0) - cw).max(20), frow_h),
                theme::TEXT_PRIMARY,
                TextAlign::Left,
            );
            ui.c.draw_text_single(
                ui.c.fonts.reg_10,
                &cat_name,
                Rect::new(fr.right() - cw - ui.dp(4.0), fr.y, cw, frow_h),
                theme::TEXT_MUTED,
                TextAlign::Right,
            );
            ui.push_hit(fr, act, true);
            fy += frow_h + ui.dp(2.0);
        }
    }
}

fn render_statusbar(ui: &mut Ui, state: &AppState, r: Rect) {
    let mode_text = if state.active_modal != ActiveModal::None {
        "MODAL"
    } else if state.search_focused {
        "SEARCH"
    } else {
        "NORMAL"
    };
    let mode_bg = if state.search_focused {
        theme::AMBER_FG
    } else if state.active_modal != ActiveModal::None {
        theme::PURPLE_FG
    } else {
        theme::ACCENT
    };
    let mw = (ui.c.measure_text_width(ui.c.fonts.mono_11b, mode_text) + ui.dp(18.0)).max(ui.dp(62.0));
    let mh = ui.dp(22.0);
    let my = r.y + (r.h - mh) / 2;
    ui.draw_badge(Rect::new(r.x, my, mw, mh), mode_text, theme::TEXT_WHITE, mode_bg, true);

    // Right keycaps hint
    let hints = [
        ("Tab", "切焦"),
        ("Enter", "启动"),
        ("/", "搜索"),
        ("F", "底栏"),
        ("?", "帮助"),
        ("q", "退出"),
    ];
    let kh = ui.dp(20.0);
    let ky = r.y + (r.h - kh) / 2;
    let mut rx = r.right();
    for (k, lbl) in hints.iter().rev() {
        let lw = ui.c.measure_text_width(ui.c.fonts.reg_11, lbl);
        rx -= lw;
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            lbl,
            Rect::new(rx, r.y, lw + 2, r.h),
            theme::TEXT_MUTED,
            TextAlign::Left,
        );
        rx -= ui.dp(4.0);
        let kw = (ui.c.measure_text_width(ui.c.fonts.mono_11b, k) + ui.dp(12.0)).max(ui.dp(22.0));
        rx -= kw;
        ui.draw_keycap(rx, ky, kh, k);
        rx -= ui.dp(8.0);
    }

    // Center toast or view title
    let mx = r.x + mw + ui.dp(10.0);
    let msg_r = Rect::new(mx, r.y, (rx - mx - ui.dp(8.0)).max(20), r.h);
    if !state.toast_msg.is_empty() {
        let tb = msg_r.inset(0, ui.dp(2.0));
        ui.c.fill_round_rect(
            tb,
            ui.dpf(6.0),
            Some(theme::CARD_SELECTED),
            1.0,
            theme::BORDER_SOFT_SEL,
        );
        ui.c.draw_text_single(
            ui.c.fonts.bold_11,
            &state.toast_msg,
            tb.inset(ui.dp(10.0), 0),
            theme::ACCENT,
            TextAlign::Left,
        );
    } else {
        let txt = if !state.search_query.trim().is_empty() {
            format!("实时搜索 \"{}\" · 共 {} 个匹配", state.search_query.trim(), state.cached_tools.len())
        } else {
            state.current_view_title()
        };
        ui.c.draw_text_single(
            ui.c.fonts.reg_12,
            &txt,
            msg_r,
            theme::TEXT_SECONDARY,
            TextAlign::Left,
        );
    }
}

fn render_help_modal(ui: &mut Ui, state: &mut AppState, w: i32, h: i32) {
    ui.c.dim_rect(Rect::new(0, 0, w, h), theme::DIM_BACKDROP, 0.35);
    ui.push_hit(Rect::new(0, 0, w, h), HitAction::ModalBackdrop, false);

    let mw = ui.dp(680.0).min(w - ui.dp(40.0));
    let mh = ui.dp(500.0).min(h - ui.dp(40.0));
    let mr = Rect::new((w - mw) / 2, (h - mh) / 2, mw, mh);
    ui.draw_panel(mr, true, theme::SURFACE_BG, Some(HitAction::ModalInner));

    let inr = mr.inset(ui.dp(18.0), ui.dp(18.0));
    let hh = ui.dp(28.0);
    ui.c.draw_ui_icon("help", inr.x, inr.y + ui.dp(5.0), ui.dp(18.0), theme::ACCENT);
    ui.c.draw_text_single(
        ui.c.fonts.title_14b,
        "Trust工具箱 · 快捷键与使用指南",
        Rect::new(inr.x + ui.dp(26.0), inr.y, inr.w - ui.dp(100.0), hh),
        theme::TEXT_PRIMARY,
        TextAlign::Left,
    );
    let cw = ui.measure_button_w(Some("close"), "Esc 关闭", false);
    ui.draw_button(
        Rect::new(inr.right() - cw, inr.y, cw, ui.dp(26.0)),
        Some("close"),
        "Esc 关闭",
        theme::TEXT_SLATE,
        theme::SURFACE_BG,
        theme::CARD_HOVER,
        theme::BORDER_DIM,
        false,
        HitAction::CloseModal,
    );
    ui.c.fill_rect(Rect::new(inr.x, inr.y + hh + ui.dp(10.0), inr.w, 1), theme::BORDER_SUBTLE);

    let vp = Rect::new(inr.x, inr.y + hh + ui.dp(18.0), inr.w, (inr.bottom() - (inr.y + hh + ui.dp(18.0))).max(20));
    let rows: &[(&str, Option<(&str, &str)>)] = &[
        ("全局导航", None),
        ("", Some(("j / k / ↑ / ↓", "移动光标或滚动当前面板"))),
        ("", Some(("h / l / ← / →", "在侧边栏、主面板之间切换焦点"))),
        ("", Some(("Tab / Shift+Tab", "循环切换焦点 (侧边栏 -> 主面板 -> 进程监控)"))),
        ("", Some(("1 ~ 5", "快速跳转至收藏夹前 5 项常用工具"))),
        ("", Some(("/  或  Ctrl+F", "聚焦即时搜索框（支持中英文/别名）；Esc 清除"))),
        ("工具操作", None),
        ("", Some(("Enter / 双击", "启动工具 / 看图器打开天梯图 / 全屏坏点检测"))),
        ("", Some(("a", "以管理员身份 (UAC) 提权启动当前工具"))),
        ("", Some(("b", "选择同目录下的多版本 / x64 / x86 可执行文件"))),
        ("", Some(("o", "在 Windows 资源管理器中打开并定位工具目录"))),
        ("", Some(("p / w", "加入或移出收藏夹 / 复制压缩包解压密码"))),
        ("硬件与视图", None),
        ("", Some(("r / d / e / c", "重读 hwinfo.ini / 图拉丁检测 / 导出TXT / 复制摘要"))),
        ("", Some(("s / F / P", "切换排序 / 开关底部工作坞 / 开关右侧详情"))),
        ("", Some(("x / X / c", "进程栏聚焦时：结束选中 / 全部结束 / 清理退出记录"))),
    ];

    ui.c.push_clip(vp);
    let mut y = vp.y - state.scroll_help.round() as i32;
    let start_y = y;
    for (sec, row_opt) in rows {
        if let Some((keys, desc)) = row_opt {
            let rr = Rect::new(vp.x, y, vp.w - ui.dp(10.0), ui.dp(26.0));
            if rr.bottom() >= vp.y && rr.y <= vp.bottom() {
                let kw = ui.draw_keycap(rr.x, rr.y + ui.dp(3.0), ui.dp(20.0), keys).max(ui.dp(120.0));
                ui.c.draw_text_single(
                    ui.c.fonts.reg_12,
                    desc,
                    Rect::new(rr.x + kw + ui.dp(12.0), rr.y, (rr.w - kw - ui.dp(12.0)).max(20), rr.h),
                    theme::TEXT_SECONDARY,
                    TextAlign::Left,
                );
            }
            y += ui.dp(28.0);
        } else {
            if !sec.starts_with('全') {
                ui.c.fill_rect(Rect::new(vp.x, y + ui.dp(3.0), vp.w - ui.dp(10.0), 1), theme::BORDER_SUBTLE);
                y += ui.dp(10.0);
            }
            ui.c.draw_text_single(
                ui.c.fonts.bold_12,
                sec,
                Rect::new(vp.x, y, vp.w, ui.dp(20.0)),
                theme::ACCENT,
                TextAlign::Left,
            );
            y += ui.dp(24.0);
        }
    }
    ui.c.pop_clip();
    ui.add_scrollbar(ScrollTarget::Help, vp, y - start_y, &mut state.scroll_help);
}

fn render_variants_modal(ui: &mut Ui, state: &mut AppState, w: i32, h: i32) {
    let Some(tool) = state.selected_tool().cloned() else {
        return;
    };
    ui.c.dim_rect(Rect::new(0, 0, w, h), theme::DIM_BACKDROP, 0.35);
    ui.push_hit(Rect::new(0, 0, w, h), HitAction::ModalBackdrop, false);

    let mw = ui.dp(540.0).min(w - ui.dp(40.0));
    let mh = (ui.dp(160.0) + tool.variants.len() as i32 * ui.dp(38.0)).clamp(ui.dp(240.0), ui.dp(440.0));
    let mr = Rect::new((w - mw) / 2, (h - mh) / 2, mw, mh);
    ui.draw_panel(mr, true, theme::SURFACE_BG, Some(HitAction::ModalInner));

    let inr = mr.inset(ui.dp(18.0), ui.dp(18.0));
    let hh = ui.dp(28.0);
    ui.c.draw_ui_icon("wrench", inr.x, inr.y + ui.dp(5.0), ui.dp(18.0), theme::ACCENT);
    ui.c.draw_text_single(
        ui.c.fonts.title_14b,
        &format!("选择启动版本 · {}", tool.name),
        Rect::new(inr.x + ui.dp(26.0), inr.y, inr.w - ui.dp(100.0), hh),
        theme::TEXT_PRIMARY,
        TextAlign::Left,
    );
    let cw = ui.measure_button_w(Some("close"), "Esc 取消", false);
    ui.draw_button(
        Rect::new(inr.right() - cw, inr.y, cw, ui.dp(26.0)),
        Some("close"),
        "Esc 取消",
        theme::TEXT_SLATE,
        theme::SURFACE_BG,
        theme::CARD_HOVER,
        theme::BORDER_DIM,
        false,
        HitAction::CloseModal,
    );
    ui.c.fill_rect(Rect::new(inr.x, inr.y + hh + ui.dp(10.0), inr.w, 1), theme::BORDER_SUBTLE);

    let foot_h = ui.dp(28.0);
    let foot_y = inr.bottom() - foot_h;
    let vp = Rect::new(inr.x, inr.y + hh + ui.dp(18.0), inr.w, (foot_y - ui.dp(12.0) - (inr.y + hh + ui.dp(18.0))).max(20));

    let rh = ui.dp(34.0);
    let rgap = ui.dp(4.0);
    let total_h = tool.variants.len() as i32 * (rh + rgap);
    ui.c.push_clip(vp);
    let mut y = vp.y - state.scroll_variants.round() as i32;
    for (idx, v) in tool.variants.iter().enumerate() {
        let vr = Rect::new(vp.x, y, vp.w - ui.dp(10.0), rh);
        if vr.bottom() >= vp.y && vr.y <= vp.bottom() {
            let sel = idx == state.selected_variant_idx;
            let act = HitAction::SelectVariant(idx);
            let (bg, bw, bc) = if sel {
                (theme::CARD_SELECTED, ui.dpf(1.5), theme::ACCENT)
            } else if ui.is_hovered(&act) {
                (theme::CARD_HOVER, 1.0, theme::BORDER_DIM)
            } else {
                (theme::CARD_BG, 1.0, theme::BORDER_DIM)
            };
            ui.c.fill_round_rect(vr, ui.dpf(7.0), Some(bg), bw, bc);
            let kstr = v.kind.to_uppercase();
            ui.draw_badge(
                Rect::new(vr.x + ui.dp(10.0), vr.y + ui.dp(7.0), ui.dp(48.0), ui.dp(20.0)),
                &kstr,
                theme::ACCENT,
                theme::ACCENT_SOFT,
                true,
            );
            let dw = if idx == 0 { ui.dp(66.0) } else { 0 };
            ui.c.draw_text_single(
                if sel { ui.c.fonts.bold_12 } else { ui.c.fonts.reg_12 },
                &v.label,
                Rect::new(vr.x + ui.dp(66.0), vr.y, (vr.w - ui.dp(76.0) - dw).max(20), rh),
                theme::TEXT_PRIMARY,
                TextAlign::Left,
            );
            if idx == 0 {
                ui.draw_badge(
                    Rect::new(vr.right() - ui.dp(68.0), vr.y + ui.dp(7.0), ui.dp(58.0), ui.dp(20.0)),
                    "推荐默认",
                    theme::GREEN_FG,
                    theme::GREEN_BG,
                    false,
                );
            }
            ui.push_hit(vr, act, true);
        }
        y += rh + rgap;
    }
    ui.c.pop_clip();
    ui.add_scrollbar(ScrollTarget::Variants, vp, total_h, &mut state.scroll_variants);

    // Modal footer buttons
    ui.c.draw_text_single(
        ui.c.fonts.reg_11,
        "j/k 选择 · Enter 启动 · a 管理员提权启动",
        Rect::new(inr.x, foot_y, inr.w - ui.dp(210.0), foot_h),
        theme::TEXT_MUTED,
        TextAlign::Left,
    );
    let b1w = ui.measure_button_w(Some("play"), "普通启动", true);
    let b2w = ui.measure_button_w(Some("shield"), "提权启动 (a)", false);
    ui.draw_button(
        Rect::new(inr.right() - b1w, foot_y, b1w, foot_h),
        Some("play"),
        "普通启动",
        theme::TEXT_WHITE,
        theme::ACCENT,
        theme::ACCENT_DARK,
        theme::ACCENT,
        true,
        HitAction::LaunchVariant(state.selected_variant_idx, false),
    );
    ui.draw_button(
        Rect::new(inr.right() - b1w - ui.dp(8.0) - b2w, foot_y, b2w, foot_h),
        Some("shield"),
        "提权启动 (a)",
        theme::TEXT_SLATE,
        theme::SURFACE_BG,
        theme::CARD_HOVER,
        theme::BORDER_DIM,
        false,
        HitAction::LaunchVariant(state.selected_variant_idx, true),
    );
}

fn render_screen_test(canvas: &mut Canvas, state: &mut AppState, w: i32, h: i32) -> FrameLayout {
    let idx = state.screen_pattern_idx.rem_euclid(8);
    match idx {
        0 => canvas.clear(Rgb::new(255, 0, 0)),
        1 => canvas.clear(Rgb::new(0, 255, 0)),
        2 => canvas.clear(Rgb::new(0, 0, 255)),
        3 => canvas.clear(Rgb::new(255, 255, 255)),
        4 => canvas.clear(Rgb::new(0, 0, 0)),
        5 => canvas.clear(Rgb::new(128, 128, 128)),
        6 => {
            // Smooth 24-bit spectrum gradient
            for x in 0..w {
                let p = x as f32 / w.max(1) as f32;
                let (r, g, b) = if p < 0.25 {
                    let t = p * 4.0;
                    (255, (t * 255.0) as u8, 0)
                } else if p < 0.5 {
                    let t = (p - 0.25) * 4.0;
                    (((1.0 - t) * 255.0) as u8, 255, 0)
                } else if p < 0.75 {
                    let t = (p - 0.5) * 4.0;
                    (0, 255, (t * 255.0) as u8)
                } else {
                    let t = (p - 0.75) * 4.0;
                    (0, ((1.0 - t) * 255.0) as u8, 255)
                };
                canvas.fill_rect(Rect::new(x, 0, 1, h), Rgb::new(r, g, b));
            }
        }
        _ => {
            // High-contrast 48px checkerboard directly drawn
            let tile = canvas.dp(32.0).max(16);
            canvas.clear(Rgb::new(20, 20, 20));
            let mut y = 0;
            let mut ty = 0;
            while y < h {
                let mut x = if ty % 2 == 0 { 0 } else { tile };
                while x < w {
                    canvas.fill_rect(
                        Rect::new(x, y, tile.min(w - x), tile.min(h - y)),
                        Rgb::new(240, 240, 240),
                    );
                    x += tile * 2;
                }
                y += tile;
                ty += 1;
            }
        }
    }

    let mut ui = Ui::new(canvas, state.hovered_action.clone());
    ui.push_hit(Rect::new(0, 0, w, h), HitAction::ScreenStep(1), false);

    if state.show_screen_hud {
        let hud_w = ui.dp(500.0).min(w - ui.dp(32.0));
        let hud_h = ui.dp(42.0);
        let hr = Rect::new((w - hud_w) / 2, h - hud_h - ui.dp(24.0), hud_w, hud_h);
        ui.c.fill_round_rect(hr, ui.dpf(21.0), Some(Rgb::new(24, 24, 30)), 1.0, Rgb::new(71, 85, 105));
        let names = [
            "纯红 Red (#FF0000)",
            "纯绿 Green (#00FF00)",
            "纯蓝 Blue (#0000FF)",
            "纯白 White (#FFFFFF)",
            "纯黑 Black (#000000)",
            "50% 灰 Gray (#808080)",
            "24-bit 连续平滑彩虹渐变",
            "高对比棋盘格 (响应/畸变)",
        ];
        let lbl = format!("[{}/8] {} · Space/点击切换 · h 隐藏提示 · Esc 退出", idx + 1, names[idx as usize]);
        ui.c.draw_text_single(
            ui.c.fonts.semi_125,
            &lbl,
            hr.inset(ui.dp(16.0), 0),
            theme::TEXT_WHITE,
            TextAlign::Center,
        );
        ui.push_hit(hr, HitAction::ToggleScreenHud, true);
    }

    let layout = ui.finish();
    state.last_frame = layout.clone();
    layout
}

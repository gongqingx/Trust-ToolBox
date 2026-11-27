use crate::app_state::{
    ActiveModal, ActiveView, AppState, FocusPanel, FrameLayout, HitAction, HitRegion, ScrollTarget,
    ScrollTrack, non_empty, sort_label, tool_kind_label,
};
use crate::catalog::{Tool, format_bytes};
use crate::gdi::{Canvas, Rect, Rgb, TextAlign, ThemePalette};
use crate::icons::IconCache;
use std::collections::HashSet;

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
    fn t(&self) -> &'static ThemePalette {
        self.c.theme
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

    fn draw_panel(&mut self, r: Rect, fill: Rgb, focus_action: Option<HitAction>) {
        let t = self.t();
        self.c
            .fill_round_rect(r, self.dpf(12.0), Some(fill), 1.0, t.border_dim);
        if let Some(act) = focus_action {
            self.push_hit(r, act, false);
        }
    }

    fn draw_badge(&mut self, r: Rect, text: &str, fg: Rgb, bg: Rgb, bold: bool) {
        self.c
            .fill_round_rect(r, self.dpf(5.0), Some(bg), 0.0, bg);
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

    fn draw_button_content(
        &mut self,
        r: Rect,
        icon: Option<&str>,
        icon_fg: Rgb,
        label: &str,
        fg: Rgb,
        bold: bool,
    ) {
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
            self.c
                .draw_ui_icon(ico, cx, r.y + (r.h - isz) / 2, isz, icon_fg);
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
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_button_ex(
        &mut self,
        r: Rect,
        icon: Option<&str>,
        icon_fg: Option<Rgb>,
        label: &str,
        fg: Rgb,
        bg: Rgb,
        hover_bg: Rgb,
        border_col: Rgb,
        bold: bool,
        action: HitAction,
    ) {
        let t = self.t();
        let hov = self.is_hovered(&action);
        let actual_bg = if hov { hover_bg } else { bg };
        let is_solid_accent = bg == t.accent;
        let (bw, actual_bc) = if is_solid_accent || border_col == bg {
            (0.0, actual_bg)
        } else if hov {
            (1.0, t.border_soft_sel)
        } else {
            (1.0, border_col)
        };
        self.c
            .fill_round_rect(r, self.dpf(6.0), Some(actual_bg), bw, actual_bc);
        self.draw_button_content(r, icon, icon_fg.unwrap_or(fg), label, fg, bold);
        self.push_hit(r, action, true);
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
        self.draw_button_ex(
            r, icon, None, label, fg, bg, hover_bg, border_col, bold, action,
        );
    }

    fn draw_quiet_button(
        &mut self,
        r: Rect,
        icon: Option<&str>,
        label: &str,
        fg: Rgb,
        hover_fg: Rgb,
        hover_bg: Rgb,
        bold: bool,
        action: HitAction,
    ) {
        let hov = self.is_hovered(&action);
        if hov {
            self.c
                .fill_round_rect(r, self.dpf(6.0), Some(hover_bg), 0.0, hover_bg);
        }
        let text_fg = if hov { hover_fg } else { fg };
        self.draw_button_content(r, icon, text_fg, label, text_fg, bold);
        self.push_hit(r, action, true);
    }

    fn draw_keycap(&mut self, x: i32, y: i32, h: i32, key_text: &str) -> i32 {
        let t = self.t();
        let tw = self.c.measure_text_width(self.c.fonts.mono_11b, key_text);
        let w = (tw + self.dp(12.0)).max(self.dp(22.0));
        let r = Rect::new(x, y, w, h);
        self.c.fill_round_rect(
            r,
            self.dpf(4.0),
            Some(t.keycap_bg),
            1.0,
            t.border_dim,
        );
        self.c.draw_text_single(
            self.c.fonts.mono_11b,
            key_text,
            r,
            t.text_slate,
            TextAlign::Center,
        );
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

    fn add_scrollbar(
        &mut self,
        target: ScrollTarget,
        viewport: Rect,
        content_h: i32,
        offset: &mut f32,
    ) {
        let t = self.t();
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
            t.scroll_thumb_hover
        } else {
            t.scroll_thumb
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

    let (running_count, running_ids) = state.tracker.running_summary();
    canvas.clear(canvas.theme.canvas_bg);
    let hov = state.hovered_action.clone();
    let mut ui = Ui::new(canvas, hov);

    // Full-height 3-column workbench (no top bar, no bottom status bar)
    let sidebar_w = ui.dp(204.0);
    let sidebar_r = Rect::new(0, 0, sidebar_w, h.max(1));

    let can_show_detail = w >= ui.dp(780.0);
    let show_detail = state.active_view != ActiveView::Hardware
        && state.cfg.show_preview
        && can_show_detail;
    let detail_w = if show_detail { ui.dp(352.0) } else { 0 };
    let center_x = sidebar_w;
    let center_w = (w - sidebar_w - detail_w).max(ui.dp(240.0));
    let center_r = Rect::new(center_x, 0, center_w, h.max(1));

    render_sidebar(&mut ui, state, sidebar_r, running_count);
    if state.active_view == ActiveView::Hardware {
        render_hardware_main(&mut ui, state, center_r);
    } else {
        render_tools_main(&mut ui, state, center_r, can_show_detail, &running_ids);
    }

    if show_detail {
        let detail_r = Rect::new(center_x + center_w, 0, detail_w, h.max(1));
        render_tool_detail(&mut ui, state, detail_r, &running_ids);
    }

    if !state.toast_msg.is_empty() && state.active_modal == ActiveModal::None {
        render_floating_toast(&mut ui, &state.toast_msg, sidebar_w, w, h);
    }

    match state.active_modal {
        ActiveModal::Help => render_help_modal(&mut ui, state, w, h),
        ActiveModal::Variants => render_variants_modal(&mut ui, state, w, h),
        ActiveModal::None | ActiveModal::ScreenTest => {}
    }

    let layout = ui.finish();
    state.last_frame = layout.clone();
    layout
}

fn render_floating_toast(ui: &mut Ui, msg: &str, sidebar_w: i32, w: i32, h: i32) {
    let t = ui.t();
    let area_x = sidebar_w;
    let area_w = (w - sidebar_w).max(ui.dp(160.0));
    let max_tw = (area_w - ui.dp(40.0)).max(ui.dp(120.0));
    let th = ui.dp(32.0);
    let tw = (ui.c.measure_text_width(ui.c.fonts.bold_11, msg) + ui.dp(32.0)).min(max_tw);
    let tx = area_x + (area_w - tw) / 2;
    let ty = (h - ui.dp(44.0)).max(ui.dp(16.0));
    let tr = Rect::new(tx, ty, tw, th);
    ui.c.fill_round_rect(tr, ui.dpf(16.0), Some(t.accent), 0.0, t.accent);
    ui.c.draw_text_single(
        ui.c.fonts.bold_11,
        msg,
        tr.inset(ui.dp(14.0), 0),
        t.text_white,
        TextAlign::Center,
    );
}

fn render_sidebar(ui: &mut Ui, state: &mut AppState, r: Rect, running_count: usize) {
    let t = ui.t();
    let focused = state.focus_panel == FocusPanel::Sidebar && !state.search_focused;
    ui.c.fill_rect(r, t.canvas_bg);
    ui.c.fill_rect(Rect::new(r.right() - 1, r.y, 1, r.h), t.border_dim);
    ui.push_hit(r, HitAction::FocusPanel(FocusPanel::Sidebar), false);

    // 1. Top Search Band (52dp, aligned horizontally with main header)
    let hdr_h = ui.dp(52.0);
    let inner_w = (r.w - 1).max(40);
    ui.c.fill_rect(Rect::new(r.x, r.y + hdr_h - 1, inner_w, 1), t.border_subtle);

    let sr = Rect::new(
        r.x + ui.dp(12.0),
        r.y + (hdr_h - ui.dp(30.0)) / 2,
        (inner_w - ui.dp(24.0)).max(60),
        ui.dp(30.0),
    );
    let sb_bw = if state.search_focused {
        ui.dpf(1.5).max(1.5)
    } else {
        1.0
    };
    let sb_bc = if state.search_focused {
        t.border_active
    } else {
        t.border_dim
    };
    ui.c.fill_round_rect(sr, ui.dpf(7.0), Some(t.surface_bg), sb_bw, sb_bc);

    let s_ico = ui.dp(13.0);
    ui.c.draw_ui_icon(
        "search",
        sr.x + ui.dp(9.0),
        sr.y + (sr.h - s_ico) / 2,
        s_ico,
        if state.search_focused {
            t.text_primary
        } else {
            t.text_muted
        },
    );

    let right_reserve = if !state.search_query.is_empty() {
        ui.dp(24.0)
    } else if !state.search_focused {
        ui.dp(20.0)
    } else {
        ui.dp(8.0)
    };
    let text_r = Rect::new(
        sr.x + ui.dp(28.0),
        sr.y,
        (sr.w - ui.dp(28.0) - right_reserve).max(20),
        sr.h,
    );
    if state.search_query.is_empty() {
        ui.c.draw_text_single(
            ui.c.fonts.reg_12,
            "搜索工具...",
            text_r,
            t.text_muted,
            TextAlign::Left,
        );
        if !state.search_focused {
            ui.c.draw_text_single(
                ui.c.fonts.mono_11,
                "/",
                Rect::new(sr.right() - ui.dp(20.0), sr.y, ui.dp(14.0), sr.h),
                t.text_muted,
                TextAlign::Center,
            );
        }
    } else {
        ui.c.draw_text_single(
            ui.c.fonts.reg_125,
            &state.search_query,
            text_r,
            t.text_primary,
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
            Rect::new(
                cx,
                sr.y + ui.dp(7.0),
                ui.dp(1.5).max(1),
                sr.h - ui.dp(14.0),
            ),
            t.accent,
        );
    }
    ui.push_hit(sr, HitAction::FocusSearch, true);

    if !state.search_query.is_empty() {
        let csz = ui.dp(16.0);
        let cr = Rect::new(
            sr.right() - ui.dp(22.0),
            sr.y + (sr.h - csz) / 2,
            csz,
            csz,
        );
        let chov = ui.is_hovered(&HitAction::ClearSearch);
        let cbg = if chov { t.card_hover } else { t.pill_bg };
        ui.c.fill_round_rect(cr, ui.dpf(4.0), Some(cbg), 0.0, cbg);
        ui.c.draw_ui_icon(
            "close",
            cr.x + ui.dp(3.5),
            cr.y + ui.dp(3.5),
            ui.dp(9.0),
            t.text_secondary,
        );
        ui.push_hit(cr, HitAction::ClearSearch, true);
    }

    // 2. Bottom Anchored Footer (running status + total count + quiet Help button)
    let footer_h = if running_count > 0 {
        ui.dp(64.0)
    } else {
        ui.dp(40.0)
    };
    let footer_y = (r.bottom() - footer_h).max(r.y + hdr_h + ui.dp(40.0));
    ui.c.fill_rect(
        Rect::new(r.x + ui.dp(12.0), footer_y, (inner_w - ui.dp(24.0)).max(20), 1),
        t.border_subtle,
    );
    if running_count > 0 {
        let run_r = Rect::new(r.x + ui.dp(14.0), footer_y + ui.dp(6.0), inner_w - ui.dp(28.0), ui.dp(22.0));
        ui.c.draw_circle(
            (run_r.x + ui.dp(3.0)) as f32,
            (run_r.y + run_r.h / 2) as f32,
            ui.dpf(3.0),
            t.green_fg,
        );
        let run_txt = format!("{running_count} 个工具运行中");
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            &run_txt,
            Rect::new(run_r.x + ui.dp(11.0), run_r.y, (run_r.w - ui.dp(11.0)).max(20), run_r.h),
            t.green_fg,
            TextAlign::Left,
        );
    }

    let help_w = ui.measure_button_w(Some("help"), "帮助", false);
    let help_h = ui.dp(26.0);
    let foot_row_y = r.bottom() - ui.dp(33.0);
    let help_r = Rect::new(r.right() - 1 - ui.dp(10.0) - help_w, foot_row_y, help_w, help_h);
    ui.draw_quiet_button(
        help_r,
        Some("help"),
        "帮助",
        t.text_secondary,
        t.text_primary,
        t.card_hover,
        false,
        HitAction::OpenHelp,
    );
    let total_txt = format!("{} 款工具", state.cat.total_tools());
    ui.c.draw_text_single(
        ui.c.fonts.reg_11,
        &total_txt,
        Rect::new(
            r.x + ui.dp(14.0),
            foot_row_y,
            (help_r.x - (r.x + ui.dp(18.0))).max(20),
            help_h,
        ),
        t.text_muted,
        TextAlign::Left,
    );

    // 3. Scrollable Navigation List
    let nav_top = r.y + hdr_h + ui.dp(8.0);
    let nav_bottom = (footer_y - ui.dp(6.0)).max(nav_top + ui.dp(20.0));
    let list_r = Rect::new(
        r.x + ui.dp(10.0),
        nav_top,
        (inner_w - ui.dp(20.0)).max(20),
        (nav_bottom - nav_top).max(20),
    );
    let entries = state.build_sidebar_entries();
    let gap = ui.dp(2.0);
    let section_gap = ui.dp(10.0);
    let header_h = ui.dp(24.0);
    let btn_h = ui.dp(32.0);

    let mut content_h = 0;
    for (idx, item) in entries.iter().enumerate() {
        if item.is_header && idx > 0 {
            content_h += section_gap;
        }
        content_h += if item.is_header { header_h } else { btn_h };
        if idx + 1 < entries.len() {
            content_h += gap;
        }
    }

    ui.c.push_clip(list_r);
    let mut sy = list_r.y - state.scroll_sidebar.round() as i32;
    for (idx, item) in entries.iter().enumerate() {
        if item.is_header && idx > 0 {
            sy += section_gap;
        }
        let item_h = if item.is_header { header_h } else { btn_h };
        let ir = Rect::new(list_r.x, sy, list_r.w, item_h);
        if ir.bottom() >= list_r.y && ir.y <= list_r.bottom() {
            if item.is_header {
                ui.c.draw_text_single(
                    ui.c.fonts.bold_11,
                    &item.label,
                    Rect::new(ir.x + ui.dp(10.0), ir.y, (ir.w - ui.dp(14.0)).max(16), ir.h),
                    t.text_muted,
                    TextAlign::Left,
                );
            } else {
                let act = HitAction::SelectSidebar(idx);
                let hov = ui.is_hovered(&act);
                if item.selected {
                    if focused {
                        ui.c.fill_round_rect(
                            ir,
                            ui.dpf(6.0),
                            Some(t.card_selected),
                            1.0,
                            t.border_soft_sel,
                        );
                    } else {
                        ui.c.fill_round_rect(
                            ir,
                            ui.dpf(6.0),
                            Some(t.card_selected_unfocused),
                            0.0,
                            t.card_selected_unfocused,
                        );
                    }
                } else if hov {
                    ui.c.fill_round_rect(
                        ir,
                        ui.dpf(6.0),
                        Some(t.card_hover),
                        0.0,
                        t.card_hover,
                    );
                }

                let isz = ui.dp(15.0);
                let icol = if item.selected && item.view_kind == "pinned" {
                    t.amber_fg
                } else if item.selected {
                    t.text_primary
                } else {
                    t.text_secondary
                };
                ui.c.draw_ui_icon(
                    item.icon_key,
                    ir.x + ui.dp(10.0),
                    ir.y + (ir.h - isz) / 2,
                    isz,
                    icol,
                );

                let cnt_reserve = if item.show_count {
                    ui.dp(36.0)
                } else {
                    ui.dp(10.0)
                };
                let font = if item.selected {
                    ui.c.fonts.semi_125
                } else {
                    ui.c.fonts.reg_12
                };
                ui.c.draw_text_single(
                    font,
                    &item.label,
                    Rect::new(
                        ir.x + ui.dp(32.0),
                        ir.y,
                        (ir.w - ui.dp(32.0) - cnt_reserve).max(16),
                        ir.h,
                    ),
                    if item.selected {
                        t.text_primary
                    } else {
                        t.text_secondary
                    },
                    TextAlign::Left,
                );

                if item.show_count {
                    let cs = format!("{}", item.count);
                    ui.c.draw_text_single(
                        ui.c.fonts.mono_11,
                        &cs,
                        Rect::new(ir.right() - ui.dp(36.0), ir.y, ui.dp(26.0), ir.h),
                        if item.selected {
                            t.text_slate
                        } else {
                            t.text_muted
                        },
                        TextAlign::Right,
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

fn render_tools_main(
    ui: &mut Ui,
    state: &mut AppState,
    r: Rect,
    can_show_detail: bool,
    running_ids: &HashSet<String>,
) {
    let t = ui.t();
    let focused = state.focus_panel == FocusPanel::Main && !state.search_focused;
    ui.c.fill_rect(r, t.surface_bg);
    ui.push_hit(r, HitAction::FocusPanel(FocusPanel::Main), false);

    // Header band (52dp, aligned with sidebar search band)
    let hdr_h = ui.dp(52.0);
    ui.c.fill_rect(Rect::new(r.x, r.y + hdr_h - 1, r.w, 1), t.border_subtle);

    let pad_x = ui.dp(22.0);
    let btn_h = ui.dp(28.0);
    let by = r.y + (hdr_h - btn_h) / 2;
    let mut rx = r.right() - ui.dp(20.0);

    if can_show_detail {
        let det_w = ui.measure_button_w(Some("panel"), "详情", false);
        rx -= det_w;
        let det_r = Rect::new(rx, by, det_w, btn_h);
        if state.cfg.show_preview {
            ui.draw_button(
                det_r,
                Some("panel"),
                "详情",
                t.text_primary,
                t.card_selected,
                t.card_hover,
                t.border_soft_sel,
                false,
                HitAction::TogglePreview,
            );
        } else {
            ui.draw_quiet_button(
                det_r,
                Some("panel"),
                "详情",
                t.text_secondary,
                t.text_primary,
                t.card_hover,
                false,
                HitAction::TogglePreview,
            );
        }
        rx -= ui.dp(6.0);
    }

    let sort_txt = format!("{}排序", sort_label(&state.cfg.sort_mode));
    let sort_w = ui.measure_button_w(Some("sort"), &sort_txt, false);
    rx -= sort_w;
    ui.draw_button(
        Rect::new(rx, by, sort_w, btn_h),
        Some("sort"),
        &sort_txt,
        t.text_slate,
        t.surface_bg,
        t.card_hover,
        t.border_dim,
        false,
        HitAction::CycleSort,
    );

    let view_title = if !state.search_query.trim().is_empty() {
        format!("搜索 “{}”", state.search_query.trim())
    } else {
        state.current_view_title()
    };
    let count_str = format!("{} 款", state.cached_tools.len());
    let cw = ui.c.measure_text_width(ui.c.fonts.reg_12, &count_str) + ui.dp(4.0);
    let title_max_w = (rx - (r.x + pad_x) - cw - ui.dp(18.0)).max(ui.dp(60.0));
    let vtw = ui
        .c
        .measure_text_width(ui.c.fonts.title_14b, &view_title)
        .min(title_max_w);
    ui.c.draw_text_single(
        ui.c.fonts.title_14b,
        &view_title,
        Rect::new(r.x + pad_x, r.y, vtw + ui.dp(4.0), hdr_h),
        t.text_primary,
        TextAlign::Left,
    );
    ui.c.draw_text_single(
        ui.c.fonts.reg_12,
        &count_str,
        Rect::new(r.x + pad_x + vtw + ui.dp(10.0), r.y, cw, hdr_h),
        t.text_muted,
        TextAlign::Left,
    );

    // Scrollable list viewport
    let list_r = Rect::new(
        r.x + ui.dp(14.0),
        r.y + hdr_h + ui.dp(8.0),
        (r.w - ui.dp(28.0)).max(120),
        (r.h - hdr_h - ui.dp(16.0)).max(20),
    );
    ui.list_viewport_rect = Some(list_r);

    if state.cached_tools.is_empty() {
        let eico = ui.dp(34.0);
        ui.c.draw_ui_icon(
            "search",
            list_r.x + (list_r.w - eico) / 2,
            list_r.y + list_r.h / 2 - ui.dp(36.0),
            eico,
            t.text_muted,
        );
        ui.c.draw_text_single(
            ui.c.fonts.bold_13,
            "没有匹配的工具",
            Rect::new(list_r.x, list_r.y + list_r.h / 2 + ui.dp(6.0), list_r.w, ui.dp(22.0)),
            t.text_secondary,
            TextAlign::Center,
        );
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            "尝试更换关键词或按 Esc 清除搜索过滤条件",
            Rect::new(list_r.x, list_r.y + list_r.h / 2 + ui.dp(30.0), list_r.w, ui.dp(20.0)),
            t.text_muted,
            TextAlign::Center,
        );
        return;
    }

    let row_h = ui.dp(54.0);
    let total_h = state.cached_tools.len() as i32 * row_h;
    let bar_pad = if total_h > list_r.h { ui.dp(10.0) } else { 0 };
    let row_w = (list_r.w - bar_pad).max(120);
    let size_w = ui.dp(56.0);
    let kind_w = ui.dp(34.0);

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
        let rr = Rect::new(list_r.x, ry, row_w, row_h);

        if rr.bottom() >= list_r.y && rr.y <= list_r.bottom() {
            let Some(tool) = state.cat.get_by_idx(cat_idx).cloned() else {
                ry += row_h;
                continue;
            };
            let act = HitAction::SelectTool(idx);
            let hov = ui.is_hovered(&act);

            if sel {
                let card_r = rr.inset(0, ui.dp(2.0));
                if focused {
                    ui.c.fill_round_rect(
                        card_r,
                        ui.dpf(7.0),
                        Some(t.card_selected),
                        1.0,
                        t.border_soft_sel,
                    );
                } else {
                    ui.c.fill_round_rect(
                        card_r,
                        ui.dpf(7.0),
                        Some(t.card_selected_unfocused),
                        0.0,
                        t.card_selected_unfocused,
                    );
                }
            } else if hov {
                let card_r = rr.inset(0, ui.dp(2.0));
                ui.c.fill_round_rect(
                    card_r,
                    ui.dpf(7.0),
                    Some(t.card_hover),
                    0.0,
                    t.card_hover,
                );
            }

            let isz = ui.dp(28.0);
            let icon_r = Rect::new(rr.x + ui.dp(10.0), rr.y + (rr.h - isz) / 2, isz, isz);
            ui.draw_tool_icon(&mut state.icon_cache, &tool, icon_r);

            // Right metadata (clean borderless monospace text)
            let kind_txt = tool_kind_label(&tool);
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
                t.text_muted,
                TextAlign::Right,
            );

            let kr = Rect::new(sr.x - ui.dp(6.0) - kind_w, rr.y, kind_w, rr.h);
            ui.c.draw_text_single(
                ui.c.fonts.mono_11,
                kind_txt,
                kr,
                t.text_muted,
                TextAlign::Right,
            );

            let is_running = running_ids.contains(&tool.id);
            let run_reserve = if is_running {
                ui.c.draw_circle(
                    (kr.x - ui.dp(8.0)) as f32,
                    (rr.y + rr.h / 2) as f32,
                    ui.dpf(3.0),
                    t.green_fg,
                );
                ui.dp(16.0)
            } else {
                0
            };

            // Left two-line text
            let name_x = icon_r.right() + ui.dp(12.0);
            let name_max_x = kr.x - ui.dp(8.0) - run_reserve;
            let text_w = (name_max_x - name_x).max(20);
            let pinned = is_pinned_vec[idx];

            let top_y = rr.y + ui.dp(8.0);
            let l1_h = ui.dp(20.0);
            let mut nx = name_x;
            if pinned {
                let ssz = ui.dp(11.0);
                ui.c.draw_ui_icon("star", nx, top_y + (l1_h - ssz) / 2, ssz, t.amber_fg);
                nx += ssz + ui.dp(5.0);
            }
            let name_font = if sel {
                ui.c.fonts.bold_12
            } else {
                ui.c.fonts.semi_125
            };
            let avail_nw = (name_max_x - nx).max(20);
            let name_w = ui.c.measure_text_width(name_font, &tool.name).min(avail_nw);
            ui.c.draw_text_single(
                name_font,
                &tool.name,
                Rect::new(nx, top_y, avail_nw, l1_h),
                t.text_primary,
                TextAlign::Left,
            );
            if !tool.tag.is_empty() {
                let tag_x = nx + name_w + ui.dp(6.0);
                let rem_w = name_max_x - tag_x;
                if rem_w >= ui.dp(32.0) {
                    let tag_lbl = format!("· {}", tool.tag);
                    ui.c.draw_text_single(
                        ui.c.fonts.reg_11,
                        &tag_lbl,
                        Rect::new(tag_x, top_y, rem_w, l1_h),
                        t.text_muted,
                        TextAlign::Left,
                    );
                }
            }

            ui.c.draw_text_single(
                ui.c.fonts.reg_11,
                &tool.description,
                Rect::new(name_x, rr.y + ui.dp(29.0), text_w, ui.dp(17.0)),
                if sel { t.text_secondary } else { t.text_muted },
                TextAlign::Left,
            );

            // Subtle bottom hairline for resting rows (skip around selected row)
            if !sel && !hov && idx + 1 != sel_idx && idx + 1 < tool_indices.len() {
                let line_w = (rr.right() - ui.dp(10.0) - name_x).max(0);
                if line_w > 0 {
                    ui.c.fill_rect(
                        Rect::new(name_x, rr.bottom() - 1, line_w, 1),
                        t.border_subtle,
                    );
                }
            }

            ui.push_hit(rr, act, true);
        }
        ry += row_h;
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
    let t = ui.t();
    ui.c.fill_rect(r, t.surface_bg);
    ui.push_hit(r, HitAction::FocusPanel(FocusPanel::Main), false);

    // Header band (52dp, aligned with sidebar search band)
    let hdr_h = ui.dp(52.0);
    ui.c.fill_rect(Rect::new(r.x, r.y + hdr_h - 1, r.w, 1), t.border_subtle);

    let pad_x = ui.dp(24.0);
    let act_h = ui.dp(28.0);
    let act_y = r.y + (hdr_h - act_h) / 2;
    let mut rx = r.right() - ui.dp(20.0);

    let specs: [(&str, &str, Rgb, Rgb, Rgb, Rgb, bool, HitAction); 4] = [
        ("copy", "复制", t.text_slate, t.surface_bg, t.card_hover, t.border_dim, false, HitAction::CopyHwInfo),
        ("export", "导出", t.text_slate, t.surface_bg, t.card_hover, t.border_dim, false, HitAction::ExportHwInfo),
        ("overview", "硬件检测", t.text_slate, t.surface_bg, t.card_hover, t.border_dim, false, HitAction::LaunchTulading),
        ("refresh", "重读", t.text_primary, t.card_selected, t.card_hover, t.border_soft_sel, false, HitAction::ReloadHwInfo),
    ];
    for (ico, lbl, fg, bg, hbg, bc, bold, act) in specs {
        let bw = ui.measure_button_w(Some(ico), lbl, bold);
        rx -= bw;
        ui.draw_button(
            Rect::new(rx, act_y, bw, act_h),
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

    let title = "本机硬件概览";
    let tw = ui.c.measure_text_width(ui.c.fonts.title_14b, title);
    ui.c.draw_text_single(
        ui.c.fonts.title_14b,
        title,
        Rect::new(r.x + pad_x, r.y, tw + ui.dp(4.0), hdr_h),
        t.text_primary,
        TextAlign::Left,
    );

    let src_lbl = non_empty(&state.hw.source_label, "静态配置");
    let up_at = state.hw.updated_at_string();
    let meta_txt = if up_at.is_empty() {
        src_lbl.to_string()
    } else {
        format!("{src_lbl} · 更新于 {up_at}")
    };
    let meta_x = r.x + pad_x + tw + ui.dp(12.0);
    let meta_avail = rx - ui.dp(8.0) - meta_x;
    if meta_avail >= ui.dp(90.0) {
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            &meta_txt,
            Rect::new(meta_x, r.y, meta_avail, hdr_h),
            t.text_muted,
            TextAlign::Left,
        );
    }

    // Centered max-width editorial document viewport
    let vp = Rect::new(
        r.x,
        r.y + hdr_h,
        r.w,
        (r.h - hdr_h).max(20),
    );
    let doc_w = (vp.w - ui.dp(48.0)).min(ui.dp(820.0)).max(ui.dp(300.0));
    let doc_x = vp.x + (vp.w - doc_w) / 2;

    let summary_items = state.build_hw_summary();
    let two_cols = doc_w < ui.dp(560.0);
    let card_gap = ui.dp(10.0);
    let card_h = if two_cols { ui.dp(74.0) } else { ui.dp(76.0) };
    let strip_h = if two_cols {
        card_h * 2 + card_gap
    } else {
        card_h
    };

    let rows = state.build_hw_rows();
    let ico_col_w = ui.dp(22.0);
    let lbl_w = ui.dp(112.0);
    let col_gap = ui.dp(12.0);
    let anchor_h = ui.dp(32.0);
    let row_pad_v = ui.dp(12.0);
    let min_val_h = ui.dp(18.0);
    let single_thresh = ui.dp(22.0);
    let group_gap = ui.dp(12.0);
    let val_w = (doc_w - ico_col_w - lbl_w - col_gap).max(ui.dp(80.0));

    let mut metrics = Vec::with_capacity(rows.len());
    let mut spec_h = 0;
    for (idx, row) in rows.iter().enumerate() {
        if idx > 0 && row.group_start {
            spec_h += group_gap;
        }
        let val_h = ui
            .c
            .measure_text_wrap_height(ui.c.fonts.reg_12, &row.value, val_w)
            .max(min_val_h);
        let row_h = (val_h + row_pad_v).max(anchor_h);
        metrics.push((val_h, row_h));
        spec_h += row_h;
    }

    let top_pad = ui.dp(20.0);
    let strip_bottom_gap = ui.dp(20.0);
    let bottom_pad = ui.dp(24.0);
    let total_h = top_pad + strip_h + strip_bottom_gap + spec_h + bottom_pad;
    let max_scroll = (total_h - vp.h).max(0) as f32;
    state.scroll_hw = state.scroll_hw.clamp(0.0, max_scroll);

    let mut y = vp.y + top_pad - state.scroll_hw.round() as i32;
    ui.c.push_clip(vp);

    // Top 4-card hardware summary strip
    let cols = if two_cols { 2 } else { 4 };
    let card_w = (doc_w - card_gap * (cols - 1)) / cols;
    for (i, item) in summary_items.iter().enumerate() {
        let col = (i as i32) % cols;
        let r_idx = (i as i32) / cols;
        let cx = doc_x + col * (card_w + card_gap);
        let cy = y + r_idx * (card_h + card_gap);
        let cr = Rect::new(cx, cy, card_w, card_h);
        if cr.bottom() >= vp.y && cr.y <= vp.bottom() {
            ui.c.fill_round_rect(
                cr,
                ui.dpf(8.0),
                Some(t.card_bg),
                1.0,
                t.border_subtle,
            );
            let inner = cr.inset(ui.dp(12.0), ui.dp(10.0));
            let isz = ui.dp(13.0);
            ui.c.draw_ui_icon(
                item.icon_key,
                inner.x,
                inner.y + (ui.dp(17.0) - isz) / 2,
                isz,
                t.text_secondary,
            );
            ui.c.draw_text_single(
                ui.c.fonts.reg_11,
                item.title,
                Rect::new(
                    inner.x + ui.dp(18.0),
                    inner.y,
                    (inner.w - ui.dp(18.0)).max(20),
                    ui.dp(17.0),
                ),
                t.text_muted,
                TextAlign::Left,
            );
            ui.c.draw_text_single(
                ui.c.fonts.bold_12,
                &item.primary,
                Rect::new(inner.x, inner.y + ui.dp(20.0), inner.w, ui.dp(20.0)),
                t.text_primary,
                TextAlign::Left,
            );
            ui.c.draw_text_single(
                ui.c.fonts.reg_11,
                &item.secondary,
                Rect::new(inner.x, inner.y + ui.dp(40.0), inner.w, ui.dp(16.0)),
                t.text_secondary,
                TextAlign::Left,
            );
        }
    }
    y += strip_h + strip_bottom_gap;

    // Borderless editorial specification table directly on surface_bg
    let isz = ui.dp(14.0);
    let lx = doc_x + ico_col_w;
    let vx = lx + lbl_w + col_gap;

    for (idx, (row, &(val_h, row_h))) in rows.iter().zip(metrics.iter()).enumerate() {
        if idx > 0 && row.group_start {
            y += group_gap;
        }
        if y + row_h >= vp.y && y <= vp.bottom() {
            if idx == 0 || row.group_start {
                ui.c.fill_rect(Rect::new(doc_x, y, doc_w, 1), t.border_dim);
            } else {
                ui.c.fill_rect(Rect::new(vx, y, val_w, 1), t.border_subtle);
            }

            if let Some(ico) = row.icon_key {
                ui.c.draw_ui_icon(
                    ico,
                    doc_x,
                    y + (anchor_h - isz) / 2,
                    isz,
                    t.text_secondary,
                );
            }

            let is_primary_row = idx == 0 || row.group_start;
            let lbl_font = if is_primary_row {
                ui.c.fonts.semi_125
            } else {
                ui.c.fonts.reg_11
            };
            let lbl_col = if is_primary_row {
                t.text_primary
            } else {
                t.text_muted
            };
            ui.c.draw_text_single(
                lbl_font,
                &row.label,
                Rect::new(lx, y, lbl_w, anchor_h),
                lbl_col,
                TextAlign::Left,
            );

            if val_h <= single_thresh {
                ui.c.draw_text_single(
                    ui.c.fonts.reg_12,
                    &row.value,
                    Rect::new(vx, y, val_w, row_h),
                    t.text_primary,
                    TextAlign::Left,
                );
            } else {
                let vy = y + (row_h - val_h).max(0) / 2;
                ui.c.draw_text_wrap(
                    ui.c.fonts.reg_12,
                    &row.value,
                    Rect::new(vx, vy, val_w, val_h + ui.dp(2.0)),
                    t.text_primary,
                );
            }
        }
        y += row_h;
    }

    ui.c.pop_clip();
    ui.add_scrollbar(ScrollTarget::Hardware, vp, total_h, &mut state.scroll_hw);
}

fn render_tool_detail(
    ui: &mut Ui,
    state: &mut AppState,
    r: Rect,
    running_ids: &HashSet<String>,
) {
    let t = ui.t();
    ui.c.fill_rect(r, t.detail_bg);
    ui.c.fill_rect(Rect::new(r.x, r.y, 1, r.h), t.border_dim);

    let Some(tool) = state.selected_tool().cloned() else {
        let eico = ui.dp(28.0);
        ui.c.draw_ui_icon(
            "wrench",
            r.x + (r.w - eico) / 2,
            r.y + r.h / 2 - ui.dp(28.0),
            eico,
            t.text_muted,
        );
        ui.c.draw_text_single(
            ui.c.fonts.bold_13,
            "工具详情",
            Rect::new(r.x, r.y + r.h / 2 + ui.dp(4.0), r.w, ui.dp(20.0)),
            t.text_secondary,
            TextAlign::Center,
        );
        ui.c.draw_text_single(
            ui.c.fonts.reg_11,
            "请在左侧列表中选择工具",
            Rect::new(r.x, r.y + r.h / 2 + ui.dp(26.0), r.w, ui.dp(18.0)),
            t.text_muted,
            TextAlign::Center,
        );
        return;
    };

    let vp = Rect::new(r.x + 1, r.y, (r.w - 1).max(1), r.h).inset(ui.dp(22.0), ui.dp(20.0));
    let cw = (vp.w - ui.dp(8.0)).max(120);
    let mut y = vp.y - state.scroll_detail.round() as i32;
    let start_y = y;

    ui.c.push_clip(vp);

    // 1. Header: 44x44 icon + title + single clean subtitle line
    let ibox = ui.dp(44.0);
    let icon_bg = Rect::new(vp.x, y, ibox, ibox);
    ui.c.fill_round_rect(
        icon_bg,
        ui.dpf(10.0),
        Some(t.surface_bg),
        1.0,
        t.border_dim,
    );
    ui.draw_tool_icon(
        &mut state.icon_cache,
        &tool,
        icon_bg.inset(ui.dp(8.0), ui.dp(8.0)),
    );

    let tx = icon_bg.right() + ui.dp(12.0);
    let tw = (cw - ibox - ui.dp(12.0)).max(40);
    ui.c.draw_text_single(
        ui.c.fonts.hero_16b,
        &tool.name,
        Rect::new(tx, y + ui.dp(2.0), tw, ui.dp(22.0)),
        t.text_primary,
        TextAlign::Left,
    );

    let kind_lbl = tool_kind_label(&tool);
    let size_short = if tool.kind == "tui" {
        "内置模块".to_string()
    } else {
        format_bytes(tool.total_bytes)
    };
    let is_running = running_ids.contains(&tool.id);
    let sub_y = y + ui.dp(26.0);
    let sub_h = ui.dp(18.0);
    let mut sx_sub = tx;
    if is_running {
        let run_lbl = "● 运行中";
        let rw = ui.c.measure_text_width(ui.c.fonts.bold_11, run_lbl);
        ui.c.draw_text_single(
            ui.c.fonts.bold_11,
            run_lbl,
            Rect::new(sx_sub, sub_y, rw + ui.dp(4.0), sub_h),
            t.green_fg,
            TextAlign::Left,
        );
        sx_sub += rw + ui.dp(6.0);
    }
    let sub_txt = if is_running {
        format!("· {} · {} · {}", tool.category_name, kind_lbl, size_short)
    } else {
        format!("{} · {} · {}", tool.category_name, kind_lbl, size_short)
    };
    let sub_rem = (tx + tw - sx_sub).max(16);
    ui.c.draw_text_single(
        ui.c.fonts.reg_11,
        &sub_txt,
        Rect::new(sx_sub, sub_y, sub_rem, sub_h),
        t.text_muted,
        TextAlign::Left,
    );
    y += ibox + ui.dp(16.0);

    // 2. Clean 2-Row Action Hierarchy (no shortcut suffixes)
    let act1_h = ui.dp(34.0);
    let act2_h = ui.dp(30.0);
    let gap = ui.dp(8.0);
    let is_tui = tool.kind == "tui";
    let show_ver = !is_tui && tool.variants.len() > 1;
    let pinned = state.cfg.is_pinned(&tool.id);

    if is_tui {
        ui.draw_button(
            Rect::new(vp.x, y, cw, act1_h),
            Some("play"),
            "启动屏幕检测",
            t.text_white,
            t.accent,
            t.accent_dark,
            t.accent,
            true,
            HitAction::LaunchSelected(false),
        );
    } else {
        let primary_w = ((cw - gap) * 58) / 100;
        let admin_w = cw - gap - primary_w;
        ui.draw_button(
            Rect::new(vp.x, y, primary_w, act1_h),
            Some("play"),
            "启动",
            t.text_white,
            t.accent,
            t.accent_dark,
            t.accent,
            true,
            HitAction::LaunchSelected(false),
        );
        ui.draw_button(
            Rect::new(vp.x + primary_w + gap, y, admin_w, act1_h),
            Some("shield"),
            "管理员提权",
            t.text_slate,
            t.surface_bg,
            t.card_hover,
            t.border_dim,
            false,
            HitAction::LaunchSelected(true),
        );
    }
    y += act1_h + gap;

    let sec_cnt = if is_tui { 1 } else if show_ver { 3 } else { 2 };
    let sec_w = (cw - gap * (sec_cnt - 1)) / sec_cnt;
    let mut sx = vp.x;
    if !is_tui {
        ui.draw_button(
            Rect::new(sx, y, sec_w, act2_h),
            Some("folder"),
            "目录",
            t.text_slate,
            t.surface_bg,
            t.card_hover,
            t.border_dim,
            false,
            HitAction::OpenToolDir,
        );
        sx += sec_w + gap;
    }
    ui.draw_button_ex(
        Rect::new(sx, y, sec_w, act2_h),
        Some("star"),
        Some(if pinned { t.amber_fg } else { t.text_slate }),
        if pinned { "已收藏" } else { "收藏" },
        if pinned { t.amber_fg } else { t.text_slate },
        if pinned { t.amber_bg } else { t.surface_bg },
        t.card_hover,
        if pinned { t.amber_border } else { t.border_dim },
        false,
        HitAction::TogglePin,
    );
    if show_ver {
        sx += sec_w + gap;
        let ver_lbl = format!("版本 ({})", tool.variants.len());
        ui.draw_button(
            Rect::new(sx, y, sec_w, act2_h),
            Some("wrench"),
            &ver_lbl,
            t.text_slate,
            t.surface_bg,
            t.card_hover,
            t.border_dim,
            false,
            HitAction::OpenVariants,
        );
    }
    y += act2_h + ui.dp(16.0);

    ui.c.fill_rect(Rect::new(vp.x, y, cw, 1), t.border_subtle);
    y += ui.dp(14.0);

    // 3. Description
    let desc_h = ui.c.draw_text_wrap(
        ui.c.fonts.reg_125,
        &tool.description,
        Rect::new(vp.x, y, cw, ui.dp(180.0)),
        t.text_primary,
    );
    y += desc_h + ui.dp(12.0);

    // 4. Scenarios (subtle editorial pills)
    if !tool.scenarios.is_empty() {
        ui.c.draw_text_single(
            ui.c.fonts.bold_11,
            "适用场景",
            Rect::new(vp.x, y, cw, ui.dp(18.0)),
            t.text_muted,
            TextAlign::Left,
        );
        y += ui.dp(24.0);
        let pill_h = ui.dp(22.0);
        let pill_gap_x = ui.dp(6.0);
        let pill_gap_y = ui.dp(6.0);
        let mut px = vp.x;
        let mut py = y;
        for sc in &tool.scenarios {
            let pw = (ui.c.measure_text_width(ui.c.fonts.reg_11, sc) + ui.dp(16.0)).min(cw);
            if px > vp.x && px + pw > vp.x + cw {
                px = vp.x;
                py += pill_h + pill_gap_y;
            }
            let pr = Rect::new(px, py, pw, pill_h);
            ui.c.fill_round_rect(pr, ui.dpf(5.0), Some(t.pill_bg), 0.0, t.pill_bg);
            ui.c.draw_text_single(
                ui.c.fonts.reg_11,
                sc,
                pr,
                t.text_slate,
                TextAlign::Center,
            );
            px += pw + pill_gap_x;
        }
        y = py + pill_h + ui.dp(14.0);
    }

    // 5. Warning Callout (clean soft card without hard straight edge bar)
    if !tool.warning.is_empty() {
        let is_danger = tool.warning.contains("危险") || tool.warning.contains("极限");
        let acc = if is_danger { t.red_fg } else { t.amber_fg };
        let cbg = if is_danger { t.red_bg } else { t.amber_bg };
        let cbd = if is_danger { t.red_border } else { t.amber_border };
        let body_h =
            ui.c.measure_text_wrap_height(ui.c.fonts.reg_12, &tool.warning, cw - ui.dp(26.0));
        let card_h = ui.dp(44.0) + body_h;
        let cr = Rect::new(vp.x, y, cw, card_h);
        ui.c.fill_round_rect(cr, ui.dpf(8.0), Some(cbg), 1.0, cbd);
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
            Rect::new(cr.x + ui.dp(13.0), cr.y + ui.dp(31.0), cw - ui.dp(26.0), body_h + ui.dp(4.0)),
            t.text_primary,
        );
        y += card_h + ui.dp(12.0);
    }

    // 6. Encrypted Archive Password Callout
    if !tool.password_note.is_empty() {
        let body_h = ui
            .c
            .measure_text_wrap_height(ui.c.fonts.reg_12, &tool.password_note, cw - ui.dp(26.0));
        let card_h = ui.dp(46.0) + body_h;
        let cr = Rect::new(vp.x, y, cw, card_h);
        ui.c.fill_round_rect(cr, ui.dpf(8.0), Some(t.surface_bg), 1.0, t.border_dim);
        ui.c.draw_ui_icon(
            "lock",
            cr.x + ui.dp(12.0),
            cr.y + ui.dp(11.0),
            ui.dp(14.0),
            t.text_primary,
        );
        let cpw = ui.measure_button_w(Some("copy"), "复制", false);
        ui.c.draw_text_single(
            ui.c.fonts.bold_12,
            "解压密码",
            Rect::new(
                cr.x + ui.dp(31.0),
                cr.y + ui.dp(9.0),
                (cw - ui.dp(44.0) - cpw).max(30),
                ui.dp(18.0),
            ),
            t.text_primary,
            TextAlign::Left,
        );
        ui.draw_button(
            Rect::new(
                cr.right() - ui.dp(10.0) - cpw,
                cr.y + ui.dp(7.0),
                cpw,
                ui.dp(24.0),
            ),
            Some("copy"),
            "复制",
            t.text_slate,
            t.card_bg,
            t.card_hover,
            t.border_dim,
            false,
            HitAction::CopyPassword,
        );
        ui.c.draw_text_wrap(
            ui.c.fonts.reg_12,
            &tool.password_note,
            Rect::new(
                cr.x + ui.dp(13.0),
                cr.y + ui.dp(34.0),
                cw - ui.dp(26.0),
                body_h + ui.dp(4.0),
            ),
            t.text_primary,
        );
        y += card_h + ui.dp(12.0);
    }

    // 7. Image Preview Card (for Ladder Images decoded lazily via GDI+)
    if tool.kind == "img" && !tool.entry_path.as_os_str().is_empty() {
        let pr_h = ui.dp(248.0);
        let pr = Rect::new(vp.x, y, cw, pr_h);
        ui.c.fill_round_rect(pr, ui.dpf(8.0), Some(t.surface_bg), 1.0, t.border_dim);
        ui.c.draw_text_single(
            ui.c.fonts.bold_12,
            "预览图",
            Rect::new(pr.x + ui.dp(12.0), pr.y + ui.dp(8.0), cw - ui.dp(96.0), ui.dp(18.0)),
            t.text_primary,
            TextAlign::Left,
        );
        if let Some((bmp, ow, oh)) = state.preview_cache.get_cached(&tool.entry_path, 480) {
            let dim = format!("{ow}×{oh}");
            ui.c.draw_text_single(
                ui.c.fonts.mono_11,
                &dim,
                Rect::new(pr.right() - ui.dp(88.0), pr.y + ui.dp(8.0), ui.dp(76.0), ui.dp(18.0)),
                t.text_muted,
                TextAlign::Right,
            );
            let box_r = Rect::new(
                pr.x + ui.dp(10.0),
                pr.y + ui.dp(32.0),
                cw - ui.dp(20.0),
                ui.dp(204.0),
            );
            let s = (box_r.w as f32 / bmp.width.max(1) as f32)
                .min(box_r.h as f32 / bmp.height.max(1) as f32)
                .min(1.0);
            let dw = ((bmp.width as f32 * s).round() as i32).max(1);
            let dh = ((bmp.height as f32 * s).round() as i32).max(1);
            ui.c.draw_bitmap(
                &bmp,
                Rect::new(
                    box_r.x + (box_r.w - dw) / 2,
                    box_r.y + (box_r.h - dh) / 2,
                    dw,
                    dh,
                ),
            );
        } else {
            ui.c.draw_text_single(
                ui.c.fonts.reg_11,
                "正在解码预览图像...",
                Rect::new(pr.x, pr.y + ui.dp(118.0), pr.w, ui.dp(20.0)),
                t.text_muted,
                TextAlign::Center,
            );
        }
        y += pr_h + ui.dp(14.0);
    }

    // 8. Usage Steps ("怎么使用")
    let has_steps = !tool.usage_steps.is_empty();
    if has_steps {
        ui.c.fill_rect(Rect::new(vp.x, y, cw, 1), t.border_subtle);
        y += ui.dp(14.0);
        ui.c.draw_text_single(
            ui.c.fonts.bold_11,
            "怎么使用",
            Rect::new(vp.x, y, cw, ui.dp(18.0)),
            t.text_muted,
            TextAlign::Left,
        );
        y += ui.dp(26.0);
        let num_w = ui.dp(22.0);
        let step_x = vp.x + num_w + ui.dp(4.0);
        let step_w = (cw - num_w - ui.dp(4.0)).max(60);
        for (i, step) in tool.usage_steps.iter().enumerate() {
            let num_txt = format!("{:02}", i + 1);
            ui.c.draw_text_single(
                ui.c.fonts.mono_11,
                &num_txt,
                Rect::new(vp.x, y, num_w, ui.dp(18.0)),
                t.text_muted,
                TextAlign::Left,
            );
            let sh = ui
                .c
                .draw_text_wrap(
                    ui.c.fonts.reg_12,
                    step,
                    Rect::new(step_x, y, step_w, ui.dp(180.0)),
                    t.text_primary,
                )
                .max(ui.dp(18.0));
            y += sh + ui.dp(8.0);
        }
    }

    // 9. Key Tips ("重点看什么")
    if !tool.key_tips.is_empty() {
        if has_steps {
            y += ui.dp(8.0);
        } else {
            ui.c.fill_rect(Rect::new(vp.x, y, cw, 1), t.border_subtle);
            y += ui.dp(14.0);
        }
        ui.c.draw_text_single(
            ui.c.fonts.bold_11,
            "重点看什么",
            Rect::new(vp.x, y, cw, ui.dp(18.0)),
            t.text_muted,
            TextAlign::Left,
        );
        y += ui.dp(26.0);
        let tip_x = vp.x + ui.dp(15.0);
        let tip_w = (cw - ui.dp(15.0)).max(60);
        for tip in &tool.key_tips {
            ui.c.draw_circle(
                (vp.x + ui.dp(5.0)) as f32,
                (y + ui.dp(8.0)) as f32,
                ui.dpf(2.2),
                t.text_muted,
            );
            let th = ui
                .c
                .draw_text_wrap(
                    ui.c.fonts.reg_12,
                    tip,
                    Rect::new(tip_x, y, tip_w, ui.dp(180.0)),
                    t.text_slate,
                )
                .max(ui.dp(18.0));
            y += th + ui.dp(7.0);
        }
    }

    if has_steps || !tool.key_tips.is_empty() {
        y += ui.dp(8.0);
    }

    ui.c.pop_clip();
    ui.add_scrollbar(
        ScrollTarget::Detail,
        vp,
        (y - start_y).max(1),
        &mut state.scroll_detail,
    );
}

fn render_help_modal(ui: &mut Ui, state: &mut AppState, w: i32, h: i32) {
    let t = ui.t();
    ui.c.dim_rect(Rect::new(0, 0, w, h), t.dim_backdrop, 0.35);
    ui.push_hit(Rect::new(0, 0, w, h), HitAction::ModalBackdrop, false);

    let mw = ui.dp(660.0).min(w - ui.dp(40.0));
    let mh = ui.dp(490.0).min(h - ui.dp(40.0));
    let mr = Rect::new((w - mw) / 2, (h - mh) / 2, mw, mh);
    ui.draw_panel(mr, t.surface_bg, Some(HitAction::ModalInner));

    let inr = mr.inset(ui.dp(20.0), ui.dp(18.0));
    let hh = ui.dp(28.0);
    ui.c.draw_ui_icon(
        "help",
        inr.x,
        inr.y + ui.dp(5.0),
        ui.dp(17.0),
        t.text_primary,
    );
    ui.c.draw_text_single(
        ui.c.fonts.title_14b,
        "Trust工具箱 · 快捷键与使用指南",
        Rect::new(inr.x + ui.dp(25.0), inr.y, inr.w - ui.dp(96.0), hh),
        t.text_primary,
        TextAlign::Left,
    );
    let cw = ui.measure_button_w(Some("close"), "关闭", false);
    ui.draw_button(
        Rect::new(inr.right() - cw, inr.y, cw, ui.dp(28.0)),
        Some("close"),
        "关闭",
        t.text_slate,
        t.surface_bg,
        t.card_hover,
        t.border_dim,
        false,
        HitAction::CloseModal,
    );
    ui.c.fill_rect(
        Rect::new(inr.x, inr.y + hh + ui.dp(10.0), inr.w, 1),
        t.border_subtle,
    );

    let vp = Rect::new(
        inr.x,
        inr.y + hh + ui.dp(18.0),
        inr.w,
        (inr.bottom() - (inr.y + hh + ui.dp(18.0))).max(20),
    );
    let rows: &[(&str, Option<(&str, &str)>)] = &[
        ("全局导航", None),
        ("", Some(("j / k / ↑ / ↓", "移动光标或滚动当前面板"))),
        ("", Some(("h / l / ← / →", "在侧边栏与主面板之间切换焦点"))),
        ("", Some(("Tab / Shift+Tab", "在侧边栏与主面板之间切换焦点"))),
        ("", Some(("/  或  Ctrl+F", "聚焦即时搜索框（支持中英文/别名）；Esc 清除"))),
        ("工具操作", None),
        ("", Some(("Enter / 双击", "启动工具 / 看图器打开天梯图 / 全屏坏点检测"))),
        ("", Some(("a", "以管理员身份 (UAC) 提权启动当前工具"))),
        ("", Some(("b", "选择同目录下的多版本 / x64 / x86 可执行文件"))),
        ("", Some(("o", "在 Windows 资源管理器中打开并定位工具目录"))),
        ("", Some(("p / w", "加入或移出收藏夹 / 复制压缩包解压密码"))),
        ("硬件与视图", None),
        ("", Some(("r / d / e / c", "重读 hwinfo.ini / 图拉丁检测 / 导出TXT / 复制摘要"))),
        ("", Some(("s / P", "切换排序 / 开关右侧详情面板"))),
    ];

    ui.c.push_clip(vp);
    let mut y = vp.y - state.scroll_help.round() as i32;
    let start_y = y;
    for (sec, row_opt) in rows {
        if let Some((keys, desc)) = row_opt {
            let rr = Rect::new(vp.x, y, vp.w - ui.dp(10.0), ui.dp(26.0));
            if rr.bottom() >= vp.y && rr.y <= vp.bottom() {
                let kw = ui
                    .draw_keycap(rr.x, rr.y + ui.dp(3.0), ui.dp(20.0), keys)
                    .max(ui.dp(120.0));
                ui.c.draw_text_single(
                    ui.c.fonts.reg_12,
                    desc,
                    Rect::new(
                        rr.x + kw + ui.dp(12.0),
                        rr.y,
                        (rr.w - kw - ui.dp(12.0)).max(20),
                        rr.h,
                    ),
                    t.text_secondary,
                    TextAlign::Left,
                );
            }
            y += ui.dp(28.0);
        } else {
            if !sec.starts_with('全') {
                ui.c.fill_rect(
                    Rect::new(vp.x, y + ui.dp(3.0), vp.w - ui.dp(10.0), 1),
                    t.border_subtle,
                );
                y += ui.dp(10.0);
            }
            ui.c.draw_text_single(
                ui.c.fonts.bold_12,
                sec,
                Rect::new(vp.x, y, vp.w, ui.dp(20.0)),
                t.text_primary,
                TextAlign::Left,
            );
            y += ui.dp(24.0);
        }
    }
    ui.c.pop_clip();
    ui.add_scrollbar(ScrollTarget::Help, vp, y - start_y, &mut state.scroll_help);
}

fn render_variants_modal(ui: &mut Ui, state: &mut AppState, w: i32, h: i32) {
    let t = ui.t();
    let Some(tool) = state.selected_tool().cloned() else {
        return;
    };
    ui.c.dim_rect(Rect::new(0, 0, w, h), t.dim_backdrop, 0.35);
    ui.push_hit(Rect::new(0, 0, w, h), HitAction::ModalBackdrop, false);

    let mw = ui.dp(540.0).min(w - ui.dp(40.0));
    let mh = (ui.dp(160.0) + tool.variants.len() as i32 * ui.dp(38.0))
        .clamp(ui.dp(240.0), ui.dp(440.0));
    let mr = Rect::new((w - mw) / 2, (h - mh) / 2, mw, mh);
    ui.draw_panel(mr, t.surface_bg, Some(HitAction::ModalInner));

    let inr = mr.inset(ui.dp(20.0), ui.dp(18.0));
    let hh = ui.dp(28.0);
    ui.c.draw_ui_icon(
        "wrench",
        inr.x,
        inr.y + ui.dp(5.0),
        ui.dp(17.0),
        t.text_primary,
    );
    ui.c.draw_text_single(
        ui.c.fonts.title_14b,
        &format!("选择启动版本 · {}", tool.name),
        Rect::new(inr.x + ui.dp(25.0), inr.y, inr.w - ui.dp(96.0), hh),
        t.text_primary,
        TextAlign::Left,
    );
    let cw = ui.measure_button_w(Some("close"), "取消", false);
    ui.draw_button(
        Rect::new(inr.right() - cw, inr.y, cw, ui.dp(28.0)),
        Some("close"),
        "取消",
        t.text_slate,
        t.surface_bg,
        t.card_hover,
        t.border_dim,
        false,
        HitAction::CloseModal,
    );
    ui.c.fill_rect(
        Rect::new(inr.x, inr.y + hh + ui.dp(10.0), inr.w, 1),
        t.border_subtle,
    );

    let foot_h = ui.dp(30.0);
    let foot_y = inr.bottom() - foot_h;
    let vp = Rect::new(
        inr.x,
        inr.y + hh + ui.dp(18.0),
        inr.w,
        (foot_y - ui.dp(12.0) - (inr.y + hh + ui.dp(18.0))).max(20),
    );

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
                (t.card_selected, 1.0, t.border_active)
            } else if ui.is_hovered(&act) {
                (t.card_hover, 1.0, t.border_dim)
            } else {
                (t.card_bg, 1.0, t.border_dim)
            };
            ui.c.fill_round_rect(vr, ui.dpf(7.0), Some(bg), bw, bc);
            let kstr = v.kind.to_uppercase();
            ui.draw_badge(
                Rect::new(vr.x + ui.dp(10.0), vr.y + ui.dp(7.0), ui.dp(48.0), ui.dp(20.0)),
                &kstr,
                t.text_slate,
                t.pill_bg,
                true,
            );
            let dw = if idx == 0 { ui.dp(66.0) } else { 0 };
            ui.c.draw_text_single(
                if sel {
                    ui.c.fonts.bold_12
                } else {
                    ui.c.fonts.reg_12
                },
                &v.label,
                Rect::new(
                    vr.x + ui.dp(66.0),
                    vr.y,
                    (vr.w - ui.dp(76.0) - dw).max(20),
                    rh,
                ),
                t.text_primary,
                TextAlign::Left,
            );
            if idx == 0 {
                ui.draw_badge(
                    Rect::new(
                        vr.right() - ui.dp(68.0),
                        vr.y + ui.dp(7.0),
                        ui.dp(58.0),
                        ui.dp(20.0),
                    ),
                    "推荐默认",
                    t.green_fg,
                    t.green_bg,
                    false,
                );
            }
            ui.push_hit(vr, act, true);
        }
        y += rh + rgap;
    }
    ui.c.pop_clip();
    ui.add_scrollbar(
        ScrollTarget::Variants,
        vp,
        total_h,
        &mut state.scroll_variants,
    );

    ui.c.draw_text_single(
        ui.c.fonts.reg_11,
        "j/k 选择 · Enter 启动 · a 管理员提权",
        Rect::new(inr.x, foot_y, inr.w - ui.dp(200.0), foot_h),
        t.text_muted,
        TextAlign::Left,
    );
    let b1w = ui.measure_button_w(Some("play"), "启动", true);
    let b2w = ui.measure_button_w(Some("shield"), "管理员提权", false);
    ui.draw_button(
        Rect::new(inr.right() - b1w, foot_y, b1w, foot_h),
        Some("play"),
        "启动",
        t.text_white,
        t.accent,
        t.accent_dark,
        t.accent,
        true,
        HitAction::LaunchVariant(state.selected_variant_idx, false),
    );
    ui.draw_button(
        Rect::new(inr.right() - b1w - ui.dp(8.0) - b2w, foot_y, b2w, foot_h),
        Some("shield"),
        "管理员提权",
        t.text_slate,
        t.surface_bg,
        t.card_hover,
        t.border_dim,
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
        ui.c.fill_round_rect(
            hr,
            ui.dpf(21.0),
            Some(Rgb::new(24, 24, 30)),
            1.0,
            Rgb::new(71, 85, 105),
        );
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
        let lbl = format!(
            "[{}/8] {} · Space/点击切换 · h 隐藏提示 · Esc 退出",
            idx + 1,
            names[idx as usize]
        );
        ui.c.draw_text_single(
            ui.c.fonts.semi_125,
            &lbl,
            hr.inset(ui.dp(16.0), 0),
            ui.t().text_white,
            TextAlign::Center,
        );
        ui.push_hit(hr, HitAction::ToggleScreenHud, true);
    }

    let layout = ui.finish();
    state.last_frame = layout.clone();
    layout
}

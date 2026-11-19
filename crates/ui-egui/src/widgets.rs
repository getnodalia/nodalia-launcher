//! Small building blocks in the Nodalia Design Language: captions, pill buttons, badges and
//! segmented controls, over NDL's rounded surfaces.

use std::sync::Arc;

use egui::{Color32, CornerRadius, Galley, Rect, Response, RichText, Sense, Stroke, Ui, vec2};

use crate::model::Tone;
use crate::theme::{self, Tokens};

pub fn tone_color(t: &Tokens, tone: Tone) -> Color32 {
    match tone {
        Tone::Good => t.good,
        Tone::Warn => t.warn,
        Tone::Info => t.accent_ink,
        Tone::Neutral => t.muted,
        Tone::Faint => t.faint,
    }
}

/// `text` laid out in `family`, `spacing` points between letters.
pub fn text_galley(ui: &Ui, text: &str, size: f32, family: egui::FontFamily, spacing: f32, color: Color32) -> Arc<Galley> {
    let format = egui::TextFormat { font_id: egui::FontId::new(size, family), color, extra_letter_spacing: spacing, ..Default::default() };
    ui.painter().layout_job(egui::text::LayoutJob::single_section(text.to_owned(), format))
}

/// A caption: 11 px Inter medium.
pub fn caption_galley(ui: &Ui, text: &str, color: Color32) -> Arc<Galley> {
    text_galley(ui, text, theme::CAPTION_SIZE, theme::medium(), 0.0, color)
}

/// Paints `galley` at the left of `rect`, centred vertically.
fn paint_left(ui: &Ui, rect: Rect, galley: Arc<Galley>, color: Color32) {
    let pos = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    ui.painter().galley_with_override_text_color(pos, galley, color);
}

/// Paints `galley` centred in `rect`.
fn paint_centred(ui: &Ui, rect: Rect, galley: Arc<Galley>, color: Color32) {
    ui.painter().galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, color);
}

/// A caption label.
pub fn caption(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = caption_galley(ui, text, color);
    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::hover());
    paint_left(ui, rect, galley, color);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    resp
}

/// A caption cut to one line with "…" where it is wider than `max_width`; the full text shows
/// on hover and is what screen readers read.
pub fn caption_elided(ui: &mut Ui, text: &str, color: Color32, max_width: f32) -> Response {
    caption_elided_spans(ui, &[(text, color)], max_width)
}

/// [`caption_elided`] with each piece of the text in its own colour.
pub fn caption_elided_spans(ui: &mut Ui, spans: &[(&str, Color32)], max_width: f32) -> Response {
    let mut job = egui::text::LayoutJob::default();
    for (text, color) in spans {
        let format = egui::TextFormat { font_id: egui::FontId::new(theme::CAPTION_SIZE, theme::medium()), color: *color, ..Default::default() };
        job.append(text, 0.0, format);
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.min(ui.available_width()).max(0.0));
    let galley = ui.painter().layout_job(job);
    let elided = galley.elided;
    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::hover());
    let pos = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    // Each piece keeps the colour it was laid out with; nothing overrides it.
    let fallback = spans.first().map_or(Color32::PLACEHOLDER, |(_, c)| *c);
    ui.painter().galley(pos, galley, fallback);
    let text: String = spans.iter().map(|(t, _)| *t).collect();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
    if elided { resp.on_hover_text(text) } else { resp }
}

/// A solid pill with a semibold caption: a status tag.
pub fn tag(ui: &mut Ui, text: &str, fill: Color32, ink: Color32) -> Response {
    let galley = text_galley(ui, text, theme::CAPTION_SIZE, theme::semibold(), 0.0, ink);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(16.0, 8.0), Sense::hover());
    ui.painter().rect_filled(rect, theme::R_PILL, fill);
    paint_centred(ui, rect, galley, ink);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    resp
}

/// A vertical hairline `height` tall: divides the header's cells.
pub fn rule(ui: &mut Ui, height: f32) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, height), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), t.hairline());
}

/// A round header button with a drawn glyph; a quiet fill on hover. While `busy` it shows a
/// spinner and can't be clicked.
pub fn nav_icon(ui: &mut Ui, glyph: Glyph, tip: &str, height: f32, busy: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let (cell, resp) = ui.allocate_exact_size(vec2(height, height), if busy { Sense::hover() } else { Sense::click() });
    let rect = Rect::from_center_size(cell.center(), vec2(height - 12.0, height - 12.0));
    let p = ui.painter();
    let hovered = resp.hovered() && !busy;
    if hovered {
        p.rect_filled(rect, theme::R_PILL, t.sunken);
    }
    if busy {
        egui::Spinner::new().size(16.0).color(t.muted).paint_at(ui, Rect::from_center_size(rect.center(), vec2(16.0, 16.0)));
    } else {
        paint_glyph(p, rect, glyph, if hovered { t.ink_strong } else { t.muted });
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !busy, tip));
    let resp = resp.on_hover_text(tip);
    if busy { resp } else { resp.on_hover_cursor(egui::CursorIcon::PointingHand) }
}

/// How far in from the window's edges a custom title bar's window resizes ([`window_edges`]).
/// The title bar's own controls leave this band alone.
const RESIZE_EDGE: f32 = 5.0;

/// How wide the resize band along the window's edges is right now: nothing while the window is
/// maximized or full screen, when `window_edges` offers no resizing and the edges are the screen's,
/// so the controls there reach all the way to the corner (#374).
fn resize_band(ctx: &egui::Context) -> f32 {
    let (maximized, fullscreen) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
    if maximized || fullscreen { 0.0 } else { RESIZE_EDGE }
}

/// Custom title bars: dragging the header's empty space moves the window, and double-clicking it
/// maximizes or restores it. Call before drawing the header, so its controls stay on top.
pub fn title_bar_drag(ui: &mut Ui) {
    let r = ui.max_rect();
    let band = resize_band(ui.ctx());
    let rect = Rect::from_min_max(r.min + vec2(band, band), r.max - vec2(band, 0.0));
    let resp = ui.interact(rect, ui.scope_id().with("title-bar"), Sense::click_and_drag());
    // Each press moves the window at most once (`sent`). Where the release always arrives —
    // Windows, macOS, the web — egui's own `drag_started_by` says it and nothing else runs.
    // On Linux and the BSDs the window system never reports the release that ends a move
    // (rust-windowing/winit#2999, emilk/egui#7959): egui keeps the button pressed and its drag
    // bookkeeping never ends, so the next press is no press and every second drag attempt does
    // nothing (#1358). A held press from an earlier frame that moves over the bar is taken as
    // that next attempt's start, under three limits: it must have begun on the bar with the
    // primary button (a press belongs to the widget it started on — a card being dragged keeps
    // its drag — and `contains_pointer` cannot say whose the press is; a secondary press over
    // the bar arms nothing), never on a press's own frame, and only
    // once egui itself rules the press a drag rather than a click-sized wander
    // (`is_decidedly_dragging`). A fresh press re-arms everything, and so does a button egui
    // saw go up ([`let_the_button_go`] makes that happen the frame after a move starts).
    let pressed = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::PointerButton { pressed: true, .. })));
    let sent = egui::Id::unique("window-move-sent");
    if pressed || !ui.input(|i| i.pointer.primary_down()) {
        ui.ctx().data_mut(|d| d.remove::<bool>(sent));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    let held_move = {
        let began = egui::Id::unique("window-move-began");
        let press = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. } => Some(*pos),
                _ => None,
            })
        });
        let (down, moved, decidedly) =
            ui.input(|i| (i.pointer.primary_down(), i.pointer.delta() != vec2(0.0, 0.0), i.pointer.is_decidedly_dragging()));
        if press.is_some() || !down {
            ui.ctx().data_mut(|d| {
                if press.is_some_and(|p| resp.rect.contains(p)) {
                    d.insert_temp(began, true);
                } else {
                    d.remove::<bool>(began);
                }
            });
        }
        ui.ctx().data_mut(|d| d.get_temp::<bool>(began).unwrap_or(false)) && press.is_none() && down && moved && decidedly && resp.contains_pointer()
    };
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let held_move = false;
    if (resp.drag_started_by(egui::PointerButton::Primary) || held_move) && !ui.ctx().data_mut(|d| d.get_temp::<bool>(sent).unwrap_or(false)) {
        ui.ctx().data_mut(|d| d.insert_temp(sent, true));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        let_the_button_go(ui.ctx());
    }
    if resp.double_clicked() {
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
}

/// On Linux and the BSDs the window system takes the pointer once a window move or resize
/// starts, and the button release that ends it never reaches the app
/// (rust-windowing/winit#2999, emilk/egui#7959): egui keeps the button pressed and its drag
/// bookkeeping never ends. The pointer is let go the moment the move starts, as though the
/// button had gone up, which is what makes the next press a fresh one — and the drag owner is
/// dropped with [`egui::Context::stop_dragging`], or the bar stays "the dragged widget" in egui's
/// interaction snapshot, no release ever ends that drag, and the next press's drag never
/// "starts": a press that leaves the bar before its wander window closes is then outside
/// `contains_pointer`, where even the held-press fallback cannot see it. The pointer's position
/// is unknown for a frame, which nothing here reads while the window is moving anyway. Where
/// the release does arrive — Windows, macOS, the web — there is nothing to put back.
#[cfg(all(unix, not(target_os = "macos")))]
fn let_the_button_go(ctx: &egui::Context) {
    ctx.input_mut(|i| i.pointer = egui::PointerState::default());
    ctx.stop_dragging();
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn let_the_button_go(_: &egui::Context) {}

/// NDL's round window controls, `height` tall, for a right-to-left layout: they end up at the
/// right of the header. They are drawn only where the launcher draws its own frame (Linux). Under a
/// dialog egui ignores the header, so there they sit in a layer above the dialog's backdrop, as a
/// system title bar does: the first click on Close closes the window, not the dialog.
pub fn window_buttons(ui: &mut Ui, height: f32) {
    use nodalia_theme::window_controls;
    let ctx = ui.ctx().clone();
    let t = Tokens::get(&ctx);
    let (mode, palette) =
        if t.dark { (nodalia_theme::Mode::Dark, &nodalia_theme::DARK) } else { (nodalia_theme::Mode::Light, &nodalia_theme::LIGHT) };
    let (focused, maximized) = ctx.input(|i| (i.focused, i.viewport().maximized.unwrap_or(false)));
    let (rect, _) = ui.allocate_exact_size(vec2(window_controls::WIDTH + nodalia_theme::space::SM, height), Sense::hover());
    let slot = Rect::from_min_max(egui::pos2(rect.left(), rect.top()), egui::pos2(rect.right() - nodalia_theme::space::SM, rect.bottom()));
    let mut row = egui::UiBuilder::new().max_rect(slot).layout(egui::Layout::left_to_right(egui::Align::Center));
    if !ctx.memory(|m| m.is_above_modal_layer(ui.layer_id())) {
        row = row.layer_id(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("window-buttons")));
    }
    let labels = window_controls::Labels { minimize: tl!("Minimize"), maximize: tl!("Maximize"), restore: tl!("Restore"), close: tl!("Close") };
    let action = ui.scope_builder(row, |ui| window_controls::show(ui, slot, palette, mode, focused, maximized, labels)).inner;
    if let Some(action) = action {
        ctx.send_viewport_cmd(window_command(action));
    }
}

/// The viewport command an NDL window control stands for.
fn window_command(action: nodalia_theme::window_controls::Action) -> egui::ViewportCommand {
    use nodalia_theme::window_controls::Action;
    match action {
        Action::Minimize => egui::ViewportCommand::Minimized(true),
        Action::Maximize => egui::ViewportCommand::Maximized(true),
        Action::Restore => egui::ViewportCommand::Maximized(false),
        Action::Close => egui::ViewportCommand::Close,
    }
}

/// Custom title bars: a hairline round the window, and resizing it from its edges and corners.
pub fn window_edges(ctx: &egui::Context) {
    use egui::CursorIcon as C;
    use egui::viewport::ResizeDirection as D;
    let (maximized, fullscreen) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
    if maximized || fullscreen {
        return;
    }
    let t = Tokens::get(ctx);
    let r = ctx.content_rect();
    ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("window-border"))).rect_stroke(
        r,
        CornerRadius::ZERO,
        t.hairline(),
        egui::StrokeKind::Inside,
    );
    // Above every layer, so egui's click tolerance can't hand a resize's press to what's nearby (#571).
    let e = RESIZE_EDGE;
    let bands = [r.with_max_y(r.top() + e), r.with_min_y(r.bottom() - e), r.with_max_x(r.left() + e), r.with_min_x(r.right() - e)];
    for (edge, rect) in bands.into_iter().enumerate() {
        egui::Area::new(egui::Id::unique(("window-edge", edge)))
            .order(egui::Order::Tooltip)
            .movable(false)
            .constrain(false)
            .fixed_pos(rect.min)
            .show(ctx, |ui| ui.allocate_response(rect.size(), egui::Sense::CLICK));
    }
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()).filter(|p| r.contains(*p)) else { return };
    let near = |d: f32| d < RESIZE_EDGE;
    // A corner takes a little of each edge next to it.
    let corner = |d: f32| d < 3.0 * RESIZE_EDGE;
    let (left, right, top, bottom) = (p.x - r.left(), r.right() - p.x, p.y - r.top(), r.bottom() - p.y);
    let hit = if (near(top) && corner(left)) || (near(left) && corner(top)) {
        Some((D::NorthWest, C::ResizeNorthWest))
    } else if (near(top) && corner(right)) || (near(right) && corner(top)) {
        Some((D::NorthEast, C::ResizeNorthEast))
    } else if (near(bottom) && corner(left)) || (near(left) && corner(bottom)) {
        Some((D::SouthWest, C::ResizeSouthWest))
    } else if (near(bottom) && corner(right)) || (near(right) && corner(bottom)) {
        Some((D::SouthEast, C::ResizeSouthEast))
    } else if near(top) {
        Some((D::North, C::ResizeNorth))
    } else if near(bottom) {
        Some((D::South, C::ResizeSouth))
    } else if near(left) {
        Some((D::West, C::ResizeWest))
    } else if near(right) {
        Some((D::East, C::ResizeEast))
    } else {
        None
    };
    if let Some((direction, cursor)) = hit {
        ctx.set_cursor_icon(cursor);
        // A press on the band is a resize; the frame after it starts, [`let_the_button_go`]
        // makes the next press a fresh one, so no second attempt is ever lost to the release
        // the window system never reports (#1358).
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
            let_the_button_go(ctx);
        }
    }
}

/// Whether `url` may be handed to the system/browser opener.
///
/// Release assets, release pages and feed links all come from the network, so every external
/// opener goes through the same check as [`release::safe_web_link`](nodalia_launcher_core::release::safe_web_link).
/// Only https web links are opened; custom schemes stay refused.
pub(crate) fn external_link_allowed(url: &str) -> bool {
    nodalia_launcher_core::release::safe_web_link(url)
}

/// Opens a UI link only after [`external_link_allowed`]; anything else is logged and dropped.
pub(crate) fn open_external_link(ctx: &egui::Context, url: &str) {
    if external_link_allowed(url) {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    } else {
        // Debug-format so control characters in a hostile URL cannot forge a log line.
        log::warn!("refusing to open unsafe URL: {url:?}");
    }
}

/// A caption link with an ↗: muted, ink on hover. Opens `url` in the browser.
pub fn caption_link(ui: &mut Ui, text: &str, url: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let galley = caption_galley(ui, &format!("{text} ↗"), t.muted);
    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());
    paint_left(ui, rect, galley, if resp.hovered() { t.ink_strong } else { t.muted });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, text));
    let resp = resp.on_hover_text(url).on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        open_external_link(ui.ctx(), url);
    }
    resp
}

/// A small status badge: a caption on the sunken surface, in `color`.
pub fn badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let t = Tokens::get(ui.ctx());
    let galley = text_galley(ui, text, 10.5, theme::semibold(), 0.0, color);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 14.0, 20.0), Sense::hover());
    ui.painter().rect(rect, theme::R_PILL, t.sunken, t.hairline(), egui::StrokeKind::Inside);
    paint_centred(ui, rect, galley, color);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    resp
}

/// Which button variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A solid pill in the accent; darkens a little on hover.
    Primary,
    /// Outlined; fills in on hover.
    Secondary,
    /// Solid red; darkens a little on hover.
    Danger,
}

/// A button's label, laid out: [`button`] paints it, [`button_width`] measures it.
fn button_label(ui: &Ui, text: &str) -> Arc<Galley> {
    text_galley(ui, text, theme::BUTTON_SIZE, theme::semibold(), 0.0, Tokens::get(ui.ctx()).ink)
}

/// A button's width around its label: 18 px of padding each side, at least `minimum`.
fn button_width_of(label: &Galley, minimum: f32) -> f32 {
    (label.size().x + 36.0).max(minimum)
}

/// The natural width of a button, including its horizontal padding: what [`button`] takes.
pub fn button_width(ui: &Ui, text: &str, minimum: f32) -> f32 {
    button_width_of(&button_label(ui, text), minimum)
}

/// A pill button, at least `width` wide.
pub fn button(ui: &mut Ui, text: &str, kind: Kind, width: f32) -> Response {
    labelled_button(ui, text, text, kind, width)
}

/// [`button`], read out by assistive technology as `label` rather than its (shortened) `text`.
pub fn labelled_button(ui: &mut Ui, text: &str, label: &str, kind: Kind, width: f32) -> Response {
    shaped_button(ui, text, label, kind, width, theme::R_PILL)
}

/// A button with the given corner `radius`: the halves of a [`split_button`] round only their outer corners.
fn shaped_button(ui: &mut Ui, text: &str, label: &str, kind: Kind, width: f32, radius: CornerRadius) -> Response {
    let t = Tokens::get(ui.ctx());
    let galley = button_label(ui, text);
    let size = vec2(button_width_of(&galley, width), theme::CONTROL_H);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let hovered = resp.hovered() && enabled;
    let (fill, stroke, color) = match kind {
        Kind::Primary => (if hovered { t.accent.gamma_multiply(0.85) } else { t.accent }, Stroke::NONE, t.on_accent),
        Kind::Danger => (if hovered { t.bad.gamma_multiply(0.85) } else { t.bad }, Stroke::NONE, Color32::WHITE),
        Kind::Secondary if hovered => (t.sunken, Stroke::new(1.0, t.line_strong), t.ink_strong),
        Kind::Secondary => (t.panel, Stroke::new(1.0, t.line_strong), t.ink),
    };
    // Disabled: the whole button at half strength.
    let fade = |c: Color32| if enabled { c } else { c.gamma_multiply(0.5) };
    ui.painter().rect(rect, radius, fade(fill), Stroke::new(stroke.width, fade(stroke.color)), egui::StrokeKind::Inside);
    paint_centred(ui, rect, galley, fade(color));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// The gap between the halves of a [`split_button`]: a hairline of background, so a hovered
/// outlined half (which fills in) never runs into a solid neighbour.
const SPLIT_SEAM: f32 = 1.0;

/// One half of a [`split_button`]: the `label` assistive technology reads (the visible text is
/// the one [`split_fit`] chose), and its look.
pub struct Half<'a> {
    pub label: &'a str,
    pub kind: Kind,
}

/// The labels of a side-by-side [`split_button`], and the width each half takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitFit<'a> {
    pub left: &'a str,
    pub right: &'a str,
    pub left_w: f32,
    pub right_w: f32,
}

/// How a [`split_button`] is laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Split<'a> {
    /// Side by side, left then right.
    Side(SplitFit<'a>),
    /// One above the other, each the full `width`: for labels too long to sit side by side (a
    /// language with long words in a narrow slot), where the place has the height to spare.
    Stacked { top: &'a str, bottom: &'a str, width: f32 },
}

/// The height of a [`Split::Stacked`] button: two buttons and the seam between them.
pub const STACKED_H: f32 = 2.0 * theme::CONTROL_H + SPLIT_SEAM;

/// Lays out a [`split_button`] within `width`: side by side when a pair of labels fits
/// ([`split_fit`]); else, where `can_stack`, stacked, with the first pair whose labels each fit the
/// width. `None` when neither fits, so the caller shows a single button instead.
pub fn split_layout<'a>(ui: &Ui, width: f32, lefts: &[&'a str], rights: &[&'a str], can_stack: bool) -> Option<Split<'a>> {
    if let Some(fit) = split_fit(ui, width, lefts, rights) {
        return Some(Split::Side(fit));
    }
    if !can_stack {
        return None;
    }
    let top = lefts.iter().copied().find(|l| button_width(ui, l, 0.0) <= width)?;
    let bottom = rights.iter().copied().find(|r| button_width(ui, r, 0.0) <= width)?;
    Some(Split::Stacked { top, bottom, width })
}

/// Picks the first pair of labels, longest first and the left one shortened last, that fits two
/// halves within `width`: equal halves when both labels fit half the room, else halves sized to
/// their labels with the room left over shared. `None` when even the shortest pair doesn't fit.
fn split_fit<'a>(ui: &Ui, width: f32, lefts: &[&'a str], rights: &[&'a str]) -> Option<SplitFit<'a>> {
    let room = width - SPLIT_SEAM;
    let half = room / 2.0;
    let rights: Vec<(&str, f32)> = rights.iter().map(|&r| (r, button_width(ui, r, 0.0))).collect();
    for &left in lefts {
        let lw = button_width(ui, left, 0.0);
        for &(right, rw) in &rights {
            let left_w = if lw.max(rw) <= half {
                half
            } else if lw + rw <= room {
                lw + (room - lw - rw) / 2.0
            } else {
                continue;
            };
            return Some(SplitFit { left, right, left_w, right_w: room - left_w });
        }
    }
    None
}

/// Two buttons as one control, laid out as [`split_layout`] chose: `left` first (on top when
/// stacked), which is also the keyboard order. Returns each half's response.
pub fn split_button(ui: &mut Ui, left: Half<'_>, right: Half<'_>, split: Split<'_>) -> (Response, Response) {
    ui.scope(|ui| match split {
        Split::Side(fit) => {
            ui.spacing_mut().item_spacing.x = SPLIT_SEAM;
            let (lr, rr) = (CornerRadius { nw: 255, sw: 255, ne: 5, se: 5 }, CornerRadius { nw: 5, sw: 5, ne: 255, se: 255 });
            let l = shaped_button(ui, fit.left, left.label, left.kind, fit.left_w, lr);
            let r = shaped_button(ui, fit.right, right.label, right.kind, fit.right_w, rr);
            (l, r)
        }
        // Its exact size, so the row centres the pair on its line like any other control (a plain
        // vertical group would start at the top of the row).
        Split::Stacked { top, bottom, width } => {
            ui.allocate_ui_with_layout(vec2(width, STACKED_H), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = SPLIT_SEAM;
                let (tr, br) = (CornerRadius { nw: 16, ne: 16, sw: 5, se: 5 }, CornerRadius { nw: 5, ne: 5, sw: 16, se: 16 });
                let l = shaped_button(ui, top, left.label, left.kind, width, tr);
                let r = shaped_button(ui, bottom, right.label, right.kind, width, br);
                (l, r)
            })
            .inner
        }
    })
    .inner
}

/// One option in a [`cells`] group.
pub struct Cell<'a> {
    pub label: &'a str,
    pub tip: &'a str,
    /// A count after the label, and the colour it takes when it isn't zero.
    pub count: Option<(usize, Option<Color32>)>,
}

/// Mutually exclusive options in a segmented control; the chosen one is filled with the accent
/// wash. Returns whether the choice changed.
pub fn cells<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, Cell<'_>)]) -> bool {
    cells_apart(ui, value, options, 0.0)
}

/// Like [`cells`], but with `gap` between the options: with a gap each option has a rounded box
/// of its own instead of sharing one.
pub fn cells_apart<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, Cell<'_>)], gap: f32) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut changed = false;
    // In a right-to-left parent the row fills from the right: keep the options' order.
    let rtl = ui.layout().prefer_right_to_left();
    let ordered: Vec<&(T, Cell<'_>)> = if rtl { options.iter().rev().collect() } else { options.iter().collect() };
    let mut rects = Vec::with_capacity(options.len());
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (opt, cell) in ordered {
            let (rect, chosen) = paint_cell(ui, value, opt, cell, &t);
            changed |= chosen;
            rects.push(rect);
        }
    });
    // The outlines: one each when apart; otherwise one outline round the group.
    if gap > 0.0 {
        for r in &rects {
            ui.painter().rect_stroke(*r, theme::R_PILL, t.hairline(), egui::StrokeKind::Inside);
        }
    } else if let (Some(first), Some(last)) = (rects.first(), rects.last()) {
        ui.painter().rect_stroke(first.union(*last), theme::R_PILL, t.hairline(), egui::StrokeKind::Inside);
    }
    changed
}

/// Paints one cell: its label and count over a rounded box, and its hover/click behaviour.
/// Returns the cell's rect (for the row's outline) and whether the click chose it.
fn paint_cell<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, opt: &T, cell: &Cell<'_>, t: &Tokens) -> (Rect, bool) {
    let mut changed = false;
    let on = *value == *opt;
    let label = text_galley(ui, cell.label, theme::BUTTON_SIZE, theme::medium(), 0.0, t.ink);
    let count = cell.count.map(|(n, highlight)| {
        let c = if on { t.accent_ink.gamma_multiply(0.8) } else { highlight.filter(|_| n > 0).unwrap_or(t.faint) };
        (caption_galley(ui, &n.to_string(), c), c)
    });
    let w = label.size().x + count.as_ref().map_or(0.0, |(g, _)| g.size().x + 8.0) + 28.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, theme::CONTROL_H), Sense::click());
    let hovered = resp.hovered();
    let (fill, ink) = match (on, hovered) {
        (true, _) => (t.accent_wash, t.accent_ink),
        (false, true) => (t.sunken, t.ink_strong),
        (false, false) => (Color32::TRANSPARENT, t.muted),
    };
    ui.painter().rect_filled(rect, theme::R_PILL, fill);
    let text_rect = rect.shrink2(vec2(14.0, 0.0));
    let label_w = label.size().x;
    paint_left(ui, text_rect, label, ink);
    if let Some((g, c)) = count {
        paint_left(ui, text_rect.with_min_x(text_rect.left() + label_w + 8.0), g, c);
    }
    let name = match cell.count {
        Some((n, _)) => format!("{} {n}", cell.label),
        None => cell.label.to_string(),
    };
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, &name));
    if resp.on_hover_text(cell.tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !on {
        *value = *opt;
        changed = true;
    }
    (rect, changed)
}

/// A slim rounded progress bar with a caption above it.
pub fn progress(ui: &mut Ui, fraction: Option<f32>, caption: &str, color: Color32, width: f32) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, theme::CONTROL_H), Sense::hover());
    let p = ui.painter();
    let bar = Rect::from_min_size(egui::pos2(rect.left(), rect.bottom() - 4.0), vec2(width, 4.0));
    p.rect_filled(bar, theme::R_PILL, t.line_strong);
    match fraction {
        Some(f) => {
            p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * f.clamp(0.0, 1.0), bar.height())), theme::R_PILL, color);
        }
        None => {
            // Keep the segment moving through the track boundary without resetting visibly.
            let width = bar.width();
            let segment_width = width * 0.3;
            let x = (ui.input(|i| i.time) * 0.8).fract() as f32 * width;
            for left in [x, x - width] {
                let seg = Rect::from_min_size(egui::pos2(bar.left() + left, bar.top()), vec2(segment_width, bar.height())).intersect(bar);
                if seg.is_positive() {
                    p.rect_filled(seg, theme::R_PILL, color);
                }
            }
            ui.ctx().request_repaint();
        }
    }
    p.text(egui::pos2(rect.left(), rect.top() + 9.0), egui::Align2::LEFT_CENTER, caption, egui::FontId::new(12.0, theme::medium()), t.muted);
    let resp = ui.interact(rect, ui.scope_id().with(("progress", caption)), Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ProgressIndicator, true, caption));
}

pub fn human_bytes(n: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let n = n as f64;
    if n >= 1024.0 * MB {
        format!("{:.1} GB", n / (1024.0 * MB))
    } else if n >= MB {
        format!("{:.0} MB", n / MB)
    } else {
        format!("{:.0} KB", (n / 1024.0).max(1.0))
    }
}

/// Which drawn glyph a [`glyph_button`] shows (drawn, so no font needs the symbol).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Pause,
    Close,
    /// Three bars: the main menu.
    Menu,
    Refresh,
    /// An exact version is locked until the user confirms unlocking it.
    Lock,
    /// Six-dot grip for reordering pinned apps.
    Grip,
    /// Outline bookmark: pin an unpinned app to the top.
    Pin,
    /// Filled bookmark: unpin a pinned app.
    PinFilled,
}

/// Paints one of the launcher's symbols with the painter, centred in `rect`.
///
/// These are shapes rather than font characters: the bundled Inter and Sora faces
/// have no gear or refresh symbol. The app enables egui's monochrome emoji fonts for arbitrary
/// release-note/feed text, but these fixed controls stay painter-drawn so their geometry and
/// accessible names do not depend on font selection.
pub fn paint_glyph(p: &egui::Painter, rect: Rect, glyph: Glyph, color: Color32) {
    p.extend(glyph_shapes(rect, glyph, color));
}

/// The shapes [`paint_glyph`] draws for `glyph`, centred in `rect`.
///
/// Separated from painting so a test can inspect the actual geometry: every shape must stay
/// inside the button it belongs to, at both control sizes.
fn glyph_shapes(rect: Rect, glyph: Glyph, color: Color32) -> Vec<egui::Shape> {
    let m = rect.center();
    match glyph {
        Glyph::Pause => [-3.0, 3.0]
            .into_iter()
            .map(|dx| egui::Shape::rect_filled(Rect::from_center_size(m + vec2(dx, 0.0), vec2(2.5, 10.0)), CornerRadius::ZERO, color))
            .collect(),
        Glyph::Close => {
            let s = 4.5;
            vec![
                egui::Shape::line_segment([m + vec2(-s, -s), m + vec2(s, s)], Stroke::new(1.5, color)),
                egui::Shape::line_segment([m + vec2(-s, s), m + vec2(s, -s)], Stroke::new(1.5, color)),
            ]
        }
        Glyph::Menu => [-4.5, 0.0, 4.5]
            .into_iter()
            .map(|dy| egui::Shape::line_segment([m + vec2(-6.0, dy), m + vec2(6.0, dy)], Stroke::new(1.6, color)))
            .collect(),
        Glyph::Refresh => {
            let stroke = Stroke::new(1.5, color);
            let radius = 6.0;
            let start = -std::f32::consts::FRAC_PI_2;
            let points: Vec<egui::Pos2> = (0..=12)
                .map(|i| {
                    let angle = start + std::f32::consts::TAU * 0.78 * i as f32 / 12.0;
                    m + vec2(angle.cos(), angle.sin()) * radius
                })
                .collect();
            let tip = m + vec2(start.cos(), start.sin()) * radius;
            vec![
                egui::Shape::line(points, stroke),
                egui::Shape::convex_polygon(vec![tip + vec2(-2.4, -0.6), tip + vec2(1.6, -3.0), tip + vec2(1.2, 1.8)], color, Stroke::NONE),
            ]
        }
        Glyph::Lock => {
            let stroke = Stroke::new(1.4, color);
            vec![
                egui::Shape::line(
                    vec![m + vec2(-3.0, -1.0), m + vec2(-3.0, -5.0), m + vec2(0.0, -6.0), m + vec2(3.0, -5.0), m + vec2(3.0, -1.0)],
                    stroke,
                ),
                egui::Shape::rect_stroke(
                    Rect::from_min_max(m + vec2(-5.0, -1.0), m + vec2(5.0, 6.0)),
                    CornerRadius::ZERO,
                    stroke,
                    egui::StrokeKind::Inside,
                ),
                egui::Shape::line_segment([m + vec2(0.0, 1.0), m + vec2(0.0, 3.5)], stroke),
            ]
        }
        Glyph::Grip => {
            let mut shapes = Vec::with_capacity(6);
            for (dx, dy) in [(-3.0, -3.5), (3.0, -3.5), (-3.0, 0.0), (3.0, 0.0), (-3.0, 3.5), (3.0, 3.5)] {
                shapes.push(egui::Shape::circle_filled(m + vec2(dx, dy), 1.35, color));
            }
            shapes
        }
        Glyph::Pin | Glyph::PinFilled => {
            // Bookmark ribbon with a V cut at the bottom — reads clearly at CONTROL_H.
            let half_w = 4.2;
            let top = m.y - 6.5;
            let notch_y = m.y + 3.0;
            let tip = egui::pos2(m.x, m.y + 6.5);
            let left = m.x - half_w;
            let right = m.x + half_w;
            let tl = egui::pos2(left, top);
            let tr = egui::pos2(right, top);
            let bl = egui::pos2(left, notch_y);
            let br = egui::pos2(right, notch_y);
            if glyph == Glyph::PinFilled {
                // Fill is convex-only: body rect + tip triangle (the outline path is concave).
                vec![
                    egui::Shape::rect_filled(Rect::from_min_max(tl, br), CornerRadius::ZERO, color),
                    egui::Shape::convex_polygon(vec![bl, br, tip], color, Stroke::NONE),
                ]
            } else {
                vec![egui::Shape::closed_line(vec![tl, tr, br, tip, bl], Stroke::new(1.5, color))]
            }
        }
    }
}

/// The body of a round tool button: outlined, filled with the sunken surface when `hovered`.
fn tool_button_body(p: &egui::Painter, rect: Rect, t: &Tokens, hovered: bool, outline: Color32) {
    if hovered {
        p.rect(rect, theme::R_PILL, t.sunken, Stroke::new(1.0, t.line_strong), egui::StrokeKind::Inside);
    } else {
        p.rect_stroke(rect, theme::R_PILL, Stroke::new(1.0, outline), egui::StrokeKind::Inside);
    }
}

/// A drag handle for reordering pinned apps: same outline as [`glyph_button`], grab cursor.
pub fn grip_button(ui: &mut Ui, tip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(theme::CONTROL_H, theme::CONTROL_H), Sense::drag());
    let hovered = resp.hovered() || resp.dragged();
    let p = ui.painter();
    tool_button_body(p, rect, &t, hovered, t.line);
    paint_glyph(p, rect, Glyph::Grip, if hovered { t.ink_strong } else { t.muted });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    let cursor = if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab };
    resp.on_hover_text(tip).on_hover_cursor(cursor)
}

/// A small outlined round button with a drawn glyph; fills in on hover.
pub fn glyph_button(ui: &mut Ui, glyph: Glyph, tip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(theme::CONTROL_H, theme::CONTROL_H), Sense::click());
    let hovered = resp.hovered();
    let p = ui.painter();
    tool_button_body(p, rect, &t, hovered, t.line);
    paint_glyph(p, rect, glyph, if hovered { t.ink_strong } else { t.muted });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Pin toggle: outline bookmark when unpinned, filled accent bookmark when pinned.
pub fn pin_button(ui: &mut Ui, pinned: bool, tip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(theme::CONTROL_H, theme::CONTROL_H), Sense::click());
    let hovered = resp.hovered();
    let p = ui.painter();
    tool_button_body(p, rect, &t, hovered, if pinned { t.accent } else { t.line });
    let c = if hovered {
        t.ink_strong
    } else if pinned {
        t.accent_ink
    } else {
        t.muted
    };
    let glyph = if pinned { Glyph::PinFilled } else { Glyph::Pin };
    paint_glyph(p, rect, glyph, c);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small glyph beside inline text, with the same accessible name and hover help as a tool button.
pub fn inline_glyph_button(ui: &mut Ui, glyph: Glyph, tip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
    if resp.hovered() || resp.has_focus() {
        ui.painter().rect_filled(rect, theme::R_PILL, t.sunken);
    }
    paint_glyph(ui.painter(), rect, glyph, t.muted);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The "…" button that opens an app's menu: an outlined round button like [`glyph_button`]. The visible
/// dots are not a name a screen reader can use, so the button is named after the app.
pub fn more_menu<R>(ui: &mut Ui, app_name: &str, add_contents: impl FnOnce(&mut Ui) -> R) {
    let t = Tokens::get(ui.ctx());
    let btn = egui::Button::new(RichText::new("…").size(15.0).color(t.muted))
        .fill(Color32::TRANSPARENT)
        .corner_radius(theme::R_PILL)
        .min_size(vec2(theme::CONTROL_H, theme::CONTROL_H));
    let (response, _) = egui::containers::menu::MenuButton::from_button(btn).ui(ui, |ui| {
        ui.set_min_width(200.0);
        add_contents(ui)
    });
    response.accessible_name(crate::i18n::fmt(tl!("More actions for {name}"), &[("name", app_name)]));
}

/// An app's name in the display face.
pub fn wordmark(ui: &mut Ui, app: &nodalia_launcher_core::SuiteApp, size: f32) -> Response {
    wordmark_in(ui, app, size, true)
}

/// [`wordmark`], with the name in ink or, when `in_colour` is false, in grey.
pub fn wordmark_in(ui: &mut Ui, app: &nodalia_launcher_core::SuiteApp, size: f32, in_colour: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let color = if in_colour { t.ink_strong } else { t.muted };
    ui.add(
        egui::Label::new(RichText::new(app.name).size(size).family(theme::display()).color(color).extra_letter_spacing(-0.01 * size))
            .selectable(false),
    )
}

/// A heading in the display face, tracked in slightly.
pub fn heading(text: &str, size: f32, color: Color32) -> RichText {
    RichText::new(text).size(size).family(theme::display()).color(color).extra_letter_spacing(-0.02 * size)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The release that ends a window move is never delivered on Linux and the BSDs (#1358), so
    /// both drags below are left unreleased on purpose: each must still start its own move.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn every_title_bar_drag_attempt_starts_a_window_move() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, None);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |_| {}).drop_without_applying_deltas();
        let frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let out = ctx.run_ui(input, title_bar_drag);
            let commands = out
                .viewport_output
                .values()
                .flat_map(|viewport| viewport.commands.iter())
                .any(|command| matches!(command, egui::ViewportCommand::StartDrag));
            out.drop_without_applying_deltas();
            commands
        };
        let drag = |from: egui::Pos2, to: egui::Pos2| {
            frame(vec![egui::Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }]);
            frame(vec![egui::Event::PointerMoved(to)])
        };
        assert!(drag(egui::pos2(100.0, 20.0), egui::pos2(140.0, 24.0)), "the first drag starts a window move");
        assert!(drag(egui::pos2(300.0, 20.0), egui::pos2(340.0, 24.0)), "with the release never arriving, the next drag must still start one");
    }

    /// A press that began on the content below — a pinned app's card mid-drag, say — keeps its
    /// owner when the pointer crosses the bar: no window move starts, and the button is not
    /// taken from the widget that holds it.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn a_press_from_below_the_bar_keeps_its_owner() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, None);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |_| {}).drop_without_applying_deltas();
        let bar = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 40.0));
        let frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let out = ctx.run_ui(input, |ui| {
                ui.scope_builder(egui::UiBuilder::new().max_rect(bar), title_bar_drag);
            });
            let moved = out
                .viewport_output
                .values()
                .flat_map(|viewport| viewport.commands.iter())
                .any(|command| matches!(command, egui::ViewportCommand::StartDrag));
            out.drop_without_applying_deltas();
            moved
        };
        frame(vec![egui::Event::PointerButton {
            pos: egui::pos2(400.0, 300.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(!frame(vec![egui::Event::PointerMoved(egui::pos2(400.0, 20.0))]), "the bar may not take a press that began on the content below it");
        assert!(ctx.input(|i| i.pointer.primary_down()), "the widget that owns the press keeps its button");
    }

    /// After a move starts the bar stays the dragged widget in egui's books — the release that
    /// would end the drag never arrives (#1358) — and that stale owner swallows the next press:
    /// its drag never "starts". A second attempt pressed on the bar and out of it before the
    /// wander window closes — past `contains_pointer`, where the held-press fallback cannot see
    /// it — must still move the window.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn a_second_attempt_that_leaves_the_bar_quickly_still_moves_the_window() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, None);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |_| {}).drop_without_applying_deltas();
        let bar = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 40.0));
        let frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let out = ctx.run_ui(input, |ui| {
                ui.scope_builder(egui::UiBuilder::new().max_rect(bar), title_bar_drag);
            });
            let moved = out
                .viewport_output
                .values()
                .flat_map(|viewport| viewport.commands.iter())
                .any(|command| matches!(command, egui::ViewportCommand::StartDrag));
            out.drop_without_applying_deltas();
            moved
        };
        // An empty frame first, so the bar is already in egui's widget rects when the first
        // press is hit-tested — in production the bar is painted every frame, and the first
        // drag must go through egui's own `drag_started_by`, which is what leaves the stale
        // owner behind.
        frame(vec![]);
        // The first move starts and the button is let go; the release never arrives (#1358).
        frame(vec![egui::Event::PointerButton {
            pos: egui::pos2(100.0, 20.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(frame(vec![egui::Event::PointerMoved(egui::pos2(140.0, 24.0))]), "the first drag starts a window move");
        // The second attempt is out of the bar in one step, beyond any click-sized wander.
        frame(vec![egui::Event::PointerButton {
            pos: egui::pos2(300.0, 20.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(
            frame(vec![egui::Event::PointerMoved(egui::pos2(500.0, 300.0))]),
            "a second attempt that leaves the bar quickly must still move the window"
        );
    }

    /// The held-press fallback is armed by a primary press on the bar and by nothing else: a
    /// secondary press over the bar while a primary content drag is under way — a card being
    /// dragged up over the header, say — must not arm it, or the next drag-sized movement would
    /// start a window move and, through [`let_the_button_go`], take the button and the drag from
    /// the content below. The content below is a real draggable widget here, as it is in the
    /// app: egui keeps the press its owner started, and only our own bookkeeping could go wrong.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn a_secondary_press_over_the_bar_does_not_steal_a_content_drag() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, None);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |_| {}).drop_without_applying_deltas();
        let bar = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 40.0));
        let content = egui::Rect::from_min_max(egui::pos2(0.0, 100.0), egui::pos2(800.0, 500.0));
        let frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let out = ctx.run_ui(input, |ui| {
                ui.interact(content, egui::Id::unique("content"), egui::Sense::click_and_drag());
                ui.scope_builder(egui::UiBuilder::new().max_rect(bar), title_bar_drag);
            });
            let moved = out
                .viewport_output
                .values()
                .flat_map(|viewport| viewport.commands.iter())
                .any(|command| matches!(command, egui::ViewportCommand::StartDrag));
            out.drop_without_applying_deltas();
            moved
        };
        let button = |pos: egui::Pos2, button: egui::PointerButton, pressed: bool| {
            vec![egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE }]
        };
        // A primary drag of the content below, held over the bar, keeps its owner.
        frame(vec![]);
        frame(button(egui::pos2(400.0, 300.0), egui::PointerButton::Primary, true));
        assert!(!frame(vec![egui::Event::PointerMoved(egui::pos2(400.0, 20.0))]), "the bar may not take a press that began on the content below it");
        // A secondary press over the bar mid-drag must not arm the held-press fallback.
        frame(button(egui::pos2(400.0, 20.0), egui::PointerButton::Secondary, true));
        assert!(!frame(vec![egui::Event::PointerMoved(egui::pos2(450.0, 22.0))]), "a secondary press over the bar must not start a window move");
        assert!(ctx.input(|i| i.pointer.primary_down()), "the content drag keeps its button");
    }

    /// A click-sized wander while the button is held is a click — egui's own verdict, which the
    /// window buttons and the double-click rely on — and once the movement is drag-sized, the
    /// move still starts.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn a_click_sized_wander_over_the_bar_is_no_window_move() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, None);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |_| {}).drop_without_applying_deltas();
        let frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let out = ctx.run_ui(input, title_bar_drag);
            let moved = out
                .viewport_output
                .values()
                .flat_map(|viewport| viewport.commands.iter())
                .any(|command| matches!(command, egui::ViewportCommand::StartDrag));
            out.drop_without_applying_deltas();
            moved
        };
        frame(vec![egui::Event::PointerButton {
            pos: egui::pos2(100.0, 20.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(!frame(vec![egui::Event::PointerMoved(egui::pos2(103.0, 21.0))]), "a click-sized wander is still a click");
        assert!(frame(vec![egui::Event::PointerMoved(egui::pos2(140.0, 24.0))]), "a drag-sized movement still starts a window move");
    }

    /// Each press moves the window at most once: the movements after the one that started it
    /// send nothing.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn only_the_first_movement_of_a_held_press_moves_the_window() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, None);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |_| {}).drop_without_applying_deltas();
        let frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let out = ctx.run_ui(input, title_bar_drag);
            let moved = out
                .viewport_output
                .values()
                .flat_map(|viewport| viewport.commands.iter())
                .any(|command| matches!(command, egui::ViewportCommand::StartDrag));
            out.drop_without_applying_deltas();
            moved
        };
        frame(vec![egui::Event::PointerButton {
            pos: egui::pos2(100.0, 20.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(frame(vec![egui::Event::PointerMoved(egui::pos2(140.0, 24.0))]));
        assert!(!frame(vec![egui::Event::PointerMoved(egui::pos2(180.0, 28.0))]));
        assert!(!frame(vec![egui::Event::PointerMoved(egui::pos2(220.0, 32.0))]));
    }

    /// The same swallowed release ends a resize (#1358): both presses on the top edge below are
    /// left unreleased on purpose, and each must still start its own resize.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn every_edge_resize_attempt_starts_a_resize() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, None);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |_| {}).drop_without_applying_deltas();
        let frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let out = ctx.run_ui(input, |ctx| window_edges(ctx));
            let commands = out
                .viewport_output
                .values()
                .flat_map(|viewport| viewport.commands.iter())
                .any(|command| matches!(command, egui::ViewportCommand::BeginResize(_)));
            out.drop_without_applying_deltas();
            commands
        };
        let resize = |from: egui::Pos2, to: egui::Pos2| {
            let press = frame(vec![egui::Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }]);
            let pull = frame(vec![egui::Event::PointerMoved(to)]);
            press || pull
        };
        assert!(resize(egui::pos2(400.0, 2.0), egui::pos2(400.0, 20.0)), "the first press on the edge starts a resize");
        assert!(resize(egui::pos2(500.0, 2.0), egui::pos2(500.0, 22.0)), "with the release never arriving, the next press must still start one");
    }

    /// The drawn glyphs replace the old `⚙`/`⟳` text (which 0.37 no longer has a fallback font
    /// for). Their geometry is production code: every shape must stay inside the control, at both
    /// the tool-button and header-cell sizes, or a click target would draw outside its hit rect.
    #[test]
    fn every_painter_glyph_stays_inside_its_control() {
        for glyph in [Glyph::Pause, Glyph::Close, Glyph::Menu, Glyph::Refresh, Glyph::Lock, Glyph::Grip, Glyph::Pin, Glyph::PinFilled] {
            for side in [20.0, crate::theme::CONTROL_H, 48.0] {
                let rect = Rect::from_min_size(egui::pos2(100.0, 200.0), egui::vec2(side, side));
                let shapes = glyph_shapes(rect, glyph, Color32::WHITE);
                assert!(!shapes.is_empty(), "{glyph:?} draws nothing at {side} px");
                for shape in shapes {
                    let bounds = shape.visual_bounding_rect();
                    assert!(rect.expand(1.0).contains_rect(bounds), "{glyph:?} at {side} px draws outside its {rect:?}: {bounds:?}");
                }
            }
        }
    }

    /// A split button's labels always fit its halves, which fill the width less the seam; the
    /// left label keeps its version longest, and below the shortest pair there is no split at all.
    #[test]
    fn a_split_fits_its_width_and_steps_down_to_one_button() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, None);
        ctx.run_ui(egui::RawInput::default(), |_| {}).drop_without_applying_deltas();
        let lefts = ["Open 0.4.1", "Open"];
        let rights = ["Update to 0.5.0", "Update"];
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let natural = |s: &str| button_width(ui, s, 0.0);
            let wide = split_fit(ui, 400.0, &lefts, &rights).expect("400 px fits the full labels");
            assert_eq!((wide.left, wide.right), (lefts[0], rights[0]));
            assert_eq!(wide.left_w, wide.right_w, "room for both: equal halves");

            let shortest = natural("Open") + natural("Update") + SPLIT_SEAM;
            for w in (0..=80).map(|i| 60.5 + i as f32 * 5.0) {
                let fit = split_fit(ui, w, &lefts, &rights);
                assert_eq!(fit.is_none(), w < shortest, "at {w} px (shortest pair {shortest} px): {fit:?}");
                if let Some(f) = fit {
                    assert!(natural(f.left) <= f.left_w + 0.01 && natural(f.right) <= f.right_w + 0.01, "at {w} px: {f:?}");
                    assert!((f.left_w + f.right_w + SPLIT_SEAM - w).abs() < 0.01, "at {w} px the halves fill the width: {f:?}");
                }
            }

            let tight = natural("Open 0.4.1") + natural("Update") + SPLIT_SEAM + 0.5;
            let f = split_fit(ui, tight, &lefts, &rights).expect("the version and the short update label fit");
            assert_eq!((f.left, f.right), ("Open 0.4.1", "Update"), "the update label shortens before Open loses its version");

            // Too narrow to sit side by side: stacked where the place allows, each label fitting.
            let narrow = shortest - 10.0;
            assert_eq!(split_layout(ui, narrow, &lefts, &rights, false), None);
            match split_layout(ui, narrow, &lefts, &rights, true) {
                Some(Split::Stacked { top, bottom, width }) => {
                    assert!(natural(top) <= width && natural(bottom) <= width, "{top} / {bottom} in {width}");
                    assert_eq!(top, "Open 0.4.1", "a stacked Open has the whole width for its version");
                }
                other => panic!("expected a stacked split, got {other:?}"),
            }
            assert!(matches!(split_layout(ui, 400.0, &lefts, &rights, true), Some(Split::Side(_))), "side by side when it fits");
            assert_eq!(split_layout(ui, 20.0, &lefts, &rights, true), None, "nothing fits 20 px");
        })
        .drop_without_applying_deltas();
    }

    /// The URLs the context has asked to open in its current output, for asserting that the opener
    /// was (or was not) reached.
    fn opened_urls(ctx: &egui::Context) -> Vec<String> {
        ctx.output(|out| {
            out.commands
                .iter()
                .filter_map(|command| match command {
                    egui::OutputCommand::OpenUrl(open) => Some(open.url.clone()),
                    _ => None,
                })
                .collect()
        })
    }

    /// Every external opener goes through `open_external_link`; a release asset URL carrying an
    /// opener metacharacter must never reach egui's open command, while a plain https release
    /// URL still does; custom schemes never do.
    #[test]
    fn external_openers_refuse_hostile_urls() {
        let ctx = egui::Context::default();
        let release_asset = "https://github.com/getnodalia/lienzo/releases/download/v0.5.0/lienzo-0.5.0-macos-universal.dmg";
        open_external_link(&ctx, release_asset);
        assert_eq!(opened_urls(&ctx), vec![release_asset.to_string()]);

        ctx.output_mut(|out| out.commands.clear());
        for hostile in [
            "https://github.com/getnodalia/lienzo/releases/download/v0.5.0/lienzo.dmg|calc.exe",
            "https://github.com/getnodalia/lienzo/releases/download/v0.5.0/lienzo.dmg&whoami",
            "https://github.com/getnodalia/lienzo/releases/download/v0.5.0/lienzo.dmg`id`",
            "https://github.com/getnodalia/lienzo/releases/download/v0.5.0/lienzo.dmg\nnext",
            "https://user@evil.example/lienzo.dmg",
            "https:///path",
            "javascript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "ms-msdt:/id",
            "customscheme://-/open/someone",
        ] {
            open_external_link(&ctx, hostile);
        }
        assert!(opened_urls(&ctx).is_empty(), "hostile URLs reached the opener: {:?}", opened_urls(&ctx));
    }

    /// Each glyph must be distinct geometry, not the same fallback shape for every variant.
    #[test]
    fn each_glyph_has_its_own_shapes() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(crate::theme::CONTROL_H, crate::theme::CONTROL_H));
        let box_of = |glyph: Glyph| {
            let union = glyph_shapes(rect, glyph, Color32::WHITE)
                .into_iter()
                .map(|shape| shape.visual_bounding_rect())
                .reduce(|a, b| a.union(b))
                .expect("every glyph draws at least one shape");
            [union.min.x.to_bits(), union.min.y.to_bits(), union.max.x.to_bits(), union.max.y.to_bits()]
        };
        // PinFilled is outline vs fill of Pin — may differ by stroke width; treat Pin as the
        // representative and require every other glyph pair to differ.
        let distinct = [Glyph::Pause, Glyph::Close, Glyph::Menu, Glyph::Refresh, Glyph::Lock, Glyph::Grip, Glyph::Pin];
        let bounds: Vec<_> = distinct.iter().map(|g| (*g, box_of(*g))).collect();
        for (i, (ga, a)) in bounds.iter().enumerate() {
            for (gb, b) in &bounds[i + 1..] {
                assert_ne!(a, b, "{ga:?} and {gb:?} share one bounding box");
            }
        }
        // PinFilled still draws (covered by every_painter_glyph_stays_inside_its_control).
        let _ = box_of(Glyph::PinFilled);
    }

    #[test]
    fn window_controls_map_to_viewport_commands() {
        use nodalia_theme::window_controls::Action;
        assert_eq!(window_command(Action::Minimize), egui::ViewportCommand::Minimized(true));
        assert_eq!(window_command(Action::Maximize), egui::ViewportCommand::Maximized(true));
        assert_eq!(window_command(Action::Restore), egui::ViewportCommand::Maximized(false));
        assert_eq!(window_command(Action::Close), egui::ViewportCommand::Close);
    }
}

use eframe::egui::{self, Color32, FontFamily, FontId, RichText, TextStyle, Vec2};

pub const INK: Color32 = Color32::from_rgb(25, 39, 49);
pub const SECONDARY: Color32 = Color32::from_rgb(79, 98, 109);
pub const ACCENT: Color32 = Color32::from_rgb(20, 91, 104);
pub const SURFACE: Color32 = Color32::WHITE;
pub const CANVAS: Color32 = Color32::from_rgb(230, 235, 238);
pub const SELECTED: Color32 = Color32::from_rgb(223, 239, 241);
pub const BORDER: Color32 = Color32::from_rgb(199, 210, 216);
pub const ERROR: Color32 = Color32::from_rgb(159, 45, 54);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.override_text_color = Some(INK);
    style.visuals.panel_fill = Color32::from_rgb(248, 250, 251);
    style.visuals.window_fill = SURFACE;
    style.visuals.hyperlink_color = ACCENT;
    style.visuals.error_fg_color = ERROR;
    style.visuals.faint_bg_color = CANVAS;
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.selection.stroke = egui::Stroke::new(1.0, SURFACE);
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    style.visuals.widgets.inactive.bg_fill = SURFACE;
    style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    style.visuals.widgets.hovered.bg_fill = SELECTED;
    style.visuals.widgets.hovered.weak_bg_fill = SELECTED;
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    style.visuals.widgets.active.bg_fill = SELECTED;
    style.visuals.widgets.active.weak_bg_fill = SELECTED;
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.5, ACCENT);
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style.spacing.interact_size.y = 30.0;
    style.spacing.window_margin = egui::Margin::same(16);
    for (kind, size) in [
        (TextStyle::Heading, 22.0),
        (TextStyle::Body, 14.0),
        (TextStyle::Button, 14.0),
        (TextStyle::Small, 12.0),
    ] {
        style
            .text_styles
            .insert(kind, FontId::new(size, FontFamily::Proportional));
    }
    ctx.set_style(style);
}

pub fn primary(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).color(SURFACE).strong())
            .fill(ACCENT)
            .min_size(Vec2::new(104.0, 32.0)),
    )
}

pub fn help(ui: &mut egui::Ui, text: impl Into<RichText>) -> egui::Response {
    ui.colored_label(SECONDARY, text)
}

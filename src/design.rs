use egui::{Color32, FontFamily, FontId, RichText, TextStyle, Vec2};

pub const INK: Color32 = Color32::from_rgb(25, 39, 49);
pub const SECONDARY: Color32 = Color32::from_rgb(79, 98, 109);
pub const ACCENT: Color32 = Color32::from_rgb(20, 91, 104);
pub const SURFACE: Color32 = Color32::WHITE;
pub const CANVAS: Color32 = Color32::from_rgb(230, 235, 238);
pub const SELECTED: Color32 = Color32::from_rgb(223, 239, 241);
pub const BORDER: Color32 = Color32::from_rgb(199, 210, 216);
pub const ERROR: Color32 = Color32::from_rgb(159, 45, 54);

pub fn apply(ctx: &egui::Context) {
    // Preserve the existing light design before native input reports the OS theme.
    ctx.set_theme(egui::Theme::Light);
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

#[derive(Clone, Copy)]
pub enum PreviewState {
    Empty,
    Ready,
    Updating,
    Failed,
}

/// One stable row: background completion must not resize or move the sheet.
pub fn preview_status(ui: &mut egui::Ui, pages: usize, sheets: usize, state: PreviewState) -> bool {
    let page_word = if pages == 1 { "page" } else { "pages" };
    let sheet_word = if sheets == 1 { "sheet" } else { "sheets" };
    let summary = format!("{pages} {page_word} on {sheets} {sheet_word}");
    let (text, color) = match state {
        PreviewState::Empty | PreviewState::Ready => (summary, SECONDARY),
        PreviewState::Updating => (format!("{summary} - Updating preview..."), SECONDARY),
        PreviewState::Failed => (format!("{summary} - Preview unavailable"), ERROR),
    };
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), ui.spacing().interact_size.y),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            let retry =
                matches!(state, PreviewState::Failed) && ui.button("Retry preview").clicked();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(text).color(color)).truncate());
            });
            retry
        },
    )
    .inner
}

/// Progress and worker controls share one line instead of resizing the workspace.
pub fn status_panel<R>(
    ctx: &egui::Context,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::TopBottomPanel::bottom("status")
        .exact_height(ctx.style().spacing.interact_size.y + 8.0)
        .show(ctx, |ui| ui.horizontal(contents).inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{CentralPanel, ColorImage, Image, RawInput, Rect, TextureOptions, pos2, vec2};

    #[test]
    fn preview_and_progress_transitions_keep_the_sheet_in_place() {
        for scale in [1.0, 1.5, 2.0] {
            for width in [440.0, 700.0] {
                let ctx = egui::Context::default();
                apply(&ctx);
                ctx.set_pixels_per_point(scale);
                let sheet = ctx.load_texture(
                    "synthetic sheet",
                    ColorImage::filled([700, 1000], SURFACE),
                    TextureOptions::LINEAR,
                );
                let mut previous = None;
                // Settle egui's first-frame panel sizing before changing worker state.
                for (iteration, state) in [
                    PreviewState::Ready,
                    PreviewState::Ready,
                    PreviewState::Updating,
                    PreviewState::Ready,
                    PreviewState::Updating,
                    PreviewState::Failed,
                    PreviewState::Ready,
                ]
                .into_iter()
                .enumerate()
                {
                    let mut geometry = None;
                    let _ = ctx.run(
                        RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                pos2(0.0, 0.0),
                                vec2(width, 640.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            status_panel(ctx, |ui| {
                                if matches!(state, PreviewState::Updating) {
                                    ui.add_sized([160.0, 20.0], egui::ProgressBar::new(0.5));
                                    ui.label("Working...");
                                    let _ = ui.button("Cancel");
                                }
                                ui.add(egui::Label::new("Synthetic status").truncate());
                            });
                            CentralPanel::default().show(ctx, |ui| {
                                ui.heading("Output preview");
                                preview_status(ui, 3, 1, state);
                                geometry = Some(
                                    ui.add(
                                        Image::new(&sheet)
                                            .max_size(ui.available_size())
                                            .maintain_aspect_ratio(true),
                                    )
                                    .rect,
                                );
                            });
                        },
                    );
                    if iteration < 2 {
                        continue;
                    }
                    if let Some(previous) = previous {
                        assert_eq!(
                            geometry.unwrap(),
                            previous,
                            "Preview state resized the sheet"
                        );
                    }
                    previous = geometry;
                }
            }
        }
    }
}

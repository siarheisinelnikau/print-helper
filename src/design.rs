use egui::{Color32, FontFamily, FontId, RichText, TextStyle, Vec2};

pub const INK: Color32 = Color32::from_rgb(32, 39, 53);
pub const SECONDARY: Color32 = Color32::from_rgb(88, 97, 116);
pub const ACCENT: Color32 = Color32::from_rgb(36, 86, 198);
pub const SURFACE: Color32 = Color32::WHITE;
pub const PANEL: Color32 = Color32::from_rgb(250, 251, 253);
pub const CANVAS: Color32 = Color32::from_rgb(237, 240, 245);
pub const SELECTED: Color32 = Color32::from_rgb(235, 242, 255);
pub const BORDER: Color32 = Color32::from_rgb(210, 217, 228);
pub const ERROR: Color32 = Color32::from_rgb(161, 43, 56);
pub const PAGE_ROW_HEIGHT: f32 = 100.0;

pub fn apply(ctx: &egui::Context) {
    // Preserve the existing light design before native input reports the OS theme.
    ctx.set_theme(egui::Theme::Light);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.override_text_color = Some(INK);
    style.visuals.panel_fill = PANEL;
    style.visuals.window_fill = SURFACE;
    style.visuals.hyperlink_color = ACCENT;
    style.visuals.error_fg_color = ERROR;
    style.visuals.faint_bg_color = CANVAS;
    style.visuals.selection.bg_fill = SELECTED;
    style.visuals.selection.stroke = egui::Stroke::new(1.5, ACCENT);
    style.visuals.window_stroke = egui::Stroke::new(1.0, BORDER);
    style.visuals.window_corner_radius = egui::CornerRadius::same(10);
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
    style.spacing.button_padding = Vec2::new(10.0, 7.0);
    style.spacing.interact_size.y = 34.0;
    style.spacing.window_margin = egui::Margin::same(16);
    for (kind, size) in [
        (TextStyle::Heading, 21.0),
        (TextStyle::Body, 15.0),
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
    if !enabled {
        return ui.add_enabled(
            false,
            egui::Button::new(label).min_size(Vec2::new(112.0, 36.0)),
        );
    }
    ui.scope(|ui| {
        let widgets = &mut ui.style_mut().visuals.widgets;
        widgets.inactive.weak_bg_fill = ACCENT;
        widgets.inactive.bg_fill = ACCENT;
        widgets.inactive.bg_stroke = egui::Stroke::NONE;
        widgets.hovered.weak_bg_fill = Color32::from_rgb(28, 70, 170);
        widgets.hovered.bg_fill = widgets.hovered.weak_bg_fill;
        widgets.hovered.bg_stroke = egui::Stroke::NONE;
        widgets.active = widgets.hovered;
        ui.add_enabled(
            enabled,
            egui::Button::new(RichText::new(label).color(SURFACE).strong())
                .min_size(Vec2::new(112.0, 36.0)),
        )
    })
    .inner
}

pub fn help(ui: &mut egui::Ui, text: impl Into<RichText>) -> egui::Response {
    ui.colored_label(SECONDARY, text)
}

pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).strong());
}

pub fn choice(ui: &mut egui::Ui, label: &str, selected: bool, width: f32) -> egui::Response {
    ui.add_sized([width, 36.0], egui::Button::new(label).selected(selected))
}

pub fn page_card(
    ui: &mut egui::Ui,
    index: usize,
    page: &print_helper::layout::Page,
    selected: bool,
    texture: Option<&egui::TextureHandle>,
    draggable: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), PAGE_ROW_HEIGHT),
        if draggable {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::click()
        },
    );
    let emphasized = selected || response.has_focus();
    ui.painter().rect(
        rect,
        7.0,
        if selected { SELECTED } else { SURFACE },
        egui::Stroke::new(
            if emphasized { 2.0 } else { 1.0 },
            if emphasized || response.hovered() {
                ACCENT
            } else {
                BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    let paper =
        egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 12.0), egui::vec2(54.0, 76.0));
    ui.painter().rect_filled(paper, 2.0, CANVAS);
    if let Some(texture) = texture {
        let size = texture.size_vec2();
        let scale = (paper.width() / size.x).min(paper.height() / size.y);
        egui::Image::new(texture).paint_at(
            ui,
            egui::Rect::from_center_size(paper.center(), size * scale),
        );
    } else {
        ui.painter().text(
            paper.center(),
            egui::Align2::CENTER_CENTER,
            "...",
            FontId::proportional(15.0),
            SECONDARY,
        );
    }
    let mut labels = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                rect.min + egui::vec2(76.0, 14.0),
                rect.max - egui::vec2(10.0, 10.0),
            ))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    // Text selection must not intercept clicks intended for the page card.
    labels.add(
        egui::Label::new(RichText::new(format!("Page {}", index + 1)).strong()).selectable(false),
    );
    labels.add(
        egui::Label::new(
            RichText::new(page.path.file_name().unwrap_or_default().to_string_lossy())
                .small()
                .color(SECONDARY),
        )
        .truncate()
        .selectable(false),
    );
    labels.add(
        egui::Label::new(
            RichText::new(
                if page
                    .path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
                {
                    format!("Source page {}", page.source_page + 1)
                } else {
                    "Image".into()
                },
            )
            .small()
            .color(SECONDARY),
        )
        .truncate()
        .selectable(false),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            format!("Page {}: {}", index + 1, page.path.display()),
        )
    });
    if !response.dragged() {
        let response = if draggable {
            response.on_hover_cursor(egui::CursorIcon::Grab)
        } else {
            response
        };
        response.on_hover_text(page.path.display().to_string())
    } else {
        response
    }
}

/// Includes row gaps and the space after the final row, including scrolled lists.
pub fn page_drop_slot(
    pointer: egui::Pos2,
    viewport: egui::Rect,
    offset: f32,
    pitch: f32,
    count: usize,
) -> Option<(usize, f32)> {
    if viewport.height() <= 2.0 || !viewport.contains(pointer) {
        return None;
    }
    let local_y = (pointer.y - viewport.top() + offset).max(0.0);
    let slot = ((local_y + pitch - PAGE_ROW_HEIGHT / 2.0) / pitch)
        .floor()
        .max(0.0) as usize;
    let slot = slot.min(count);
    let marker = (viewport.top() + slot as f32 * pitch - offset - (pitch - PAGE_ROW_HEIGHT) / 2.0)
        .clamp(viewport.top() + 1.0, viewport.bottom() - 1.0);
    Some((slot, marker))
}

/// No timer is needed when the pointer is outside the edge bands or at a limit.
pub fn page_drag_scroll(
    pointer: egui::Pos2,
    viewport: egui::Rect,
    offset: f32,
    content_height: f32,
    dt: f32,
) -> Option<f32> {
    if !viewport.contains(pointer) {
        return None;
    }
    let edge = 28.0_f32.min(viewport.height() / 2.0);
    let direction = if pointer.y < viewport.top() + edge {
        -1.0
    } else if pointer.y > viewport.bottom() - edge {
        1.0
    } else {
        0.0
    };
    let next = (offset + direction * 360.0 * dt.clamp(0.0, 0.05))
        .clamp(0.0, (content_height - viewport.height()).max(0.0));
    (next != offset).then_some(next)
}

/// Reserve a stable stage and center the paper without enlarging the source bitmap.
pub fn sheet_stage(
    ui: &mut egui::Ui,
    texture: Option<&egui::TextureHandle>,
    placeholder: &str,
) -> egui::Rect {
    let size = Vec2::new(
        ui.available_width(),
        (ui.available_height() - 48.0).max(40.0),
    );
    let (stage, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    if let Some(texture) = texture {
        let available = (stage.size() - Vec2::splat(24.0)).max(Vec2::splat(1.0));
        let original = texture.size_vec2();
        let scale = (available.x / original.x)
            .min(available.y / original.y)
            .min(1.0);
        let paper = egui::Rect::from_center_size(stage.center(), original * scale);
        ui.painter().rect_filled(
            paper.translate(egui::vec2(0.0, 3.0)).expand(2.0),
            1.0,
            Color32::from_rgb(216, 222, 232),
        );
        egui::Image::new(texture).paint_at(ui, paper);
        ui.painter().rect_stroke(
            paper,
            0.0,
            egui::Stroke::new(1.0, BORDER),
            egui::StrokeKind::Outside,
        );
        paper
    } else {
        ui.painter().text(
            stage.center(),
            egui::Align2::CENTER_CENTER,
            placeholder,
            FontId::proportional(15.0),
            SECONDARY,
        );
        stage
    }
}

#[derive(Clone, Copy)]
pub enum PreviewState {
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
        PreviewState::Ready => (summary, SECONDARY),
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
    use egui::{CentralPanel, ColorImage, RawInput, Rect, TextureOptions, pos2, vec2};

    #[test]
    fn page_drop_slots_and_edge_scroll_respect_viewport_and_limits() {
        let viewport = Rect::from_min_max(pos2(10.0, 100.0), pos2(200.0, 500.0));
        for (y, slot) in [
            (100.0, 0),
            (149.0, 0),
            (151.0, 1),
            (207.0, 1),
            (259.0, 2),
            (499.0, 4),
        ] {
            let (actual, marker) = page_drop_slot(pos2(50.0, y), viewport, 0.0, 108.0, 4).unwrap();
            assert_eq!(actual, slot);
            assert!((viewport.top()..=viewport.bottom()).contains(&marker));
        }
        assert_eq!(
            page_drop_slot(pos2(50.0, 100.0), viewport, 216.0, 108.0, 20)
                .unwrap()
                .0,
            2
        );
        assert!(page_drop_slot(pos2(250.0, 200.0), viewport, 0.0, 108.0, 4).is_none());
        assert!(page_drag_scroll(pos2(50.0, 300.0), viewport, 0.0, 1000.0, 0.016).is_none());
        assert!(page_drag_scroll(pos2(50.0, 101.0), viewport, 0.0, 1000.0, 0.016).is_none());
        assert!(page_drag_scroll(pos2(50.0, 499.0), viewport, 600.0, 1000.0, 0.016).is_none());
        assert!(page_drag_scroll(pos2(250.0, 499.0), viewport, 0.0, 1000.0, 0.016).is_none());
        assert!(page_drag_scroll(pos2(50.0, 499.0), viewport, 0.0, 1000.0, 0.016).unwrap() > 0.0);
    }

    #[test]
    fn palette_keeps_small_text_readable() {
        fn luminance(color: Color32) -> f64 {
            let channels = [color.r(), color.g(), color.b()].map(|c| {
                let value = c as f64 / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            });
            channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722
        }
        for (foreground, background) in [
            (INK, SURFACE),
            (SECONDARY, PANEL),
            (SECONDARY, CANVAS),
            (SURFACE, ACCENT),
            (INK, SELECTED),
            (ACCENT, SELECTED),
            (ERROR, SURFACE),
            (ERROR, CANVAS),
        ] {
            let (a, b) = (luminance(foreground), luminance(background));
            let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
            assert!(ratio >= 4.5, "Small-text contrast is {ratio:.2}:1");
        }
    }

    #[test]
    fn compact_page_card_keeps_bounds_and_accepts_clicks_with_long_filenames() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let page = print_helper::layout::Page {
            path: "A very long document filename that must fit in the page list.pdf".into(),
            sha256: "0".repeat(64),
            source_page: 9,
            width_pt: 60.0,
            height_pt: 80.0,
            crop: [0.0; 4],
            quarter_turns: 0,
        };
        let texture = ctx.load_texture(
            "thumbnail",
            ColorImage::filled([60, 80], SURFACE),
            TextureOptions::LINEAR,
        );
        let mut previous: Option<Rect> = None;
        for (index, (selected, loaded)) in [
            (false, false),
            (false, false),
            (true, false),
            (true, true),
            (false, true),
        ]
        .into_iter()
        .enumerate()
        {
            let mut input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, 240.0))),
                ..Default::default()
            };
            if index == 4 {
                let position = previous.unwrap().center();
                input.events = vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    },
                ];
            }
            let mut geometry = None;
            let _ = ctx.run(input, |ctx| {
                CentralPanel::default()
                    .frame(egui::Frame::new().inner_margin(24))
                    .show(ctx, |ui| {
                        let response =
                            page_card(ui, 9, &page, selected, loaded.then_some(&texture), true);
                        geometry = Some(response.rect);
                        if index == 4 {
                            assert!(response.clicked());
                        }
                    });
            });
            let rect = geometry.unwrap();
            assert_eq!(rect.size(), vec2(192.0, PAGE_ROW_HEIGHT));
            if let Some(previous) = previous {
                assert_eq!(rect, previous);
            }
            previous = Some(rect);
        }
    }

    #[test]
    fn preview_and_progress_transitions_keep_the_sheet_in_place() {
        for scale in [1.0, 1.5, 2.0] {
            for width in [440.0, 700.0] {
                let ctx = egui::Context::default();
                apply(&ctx);
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
                            viewports: [(
                                egui::ViewportId::ROOT,
                                egui::ViewportInfo {
                                    native_pixels_per_point: Some(scale),
                                    ..Default::default()
                                },
                            )]
                            .into_iter()
                            .collect(),
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
                                ui.heading("Preview");
                                preview_status(ui, 3, 1, state);
                                geometry =
                                    Some(sheet_stage(ui, Some(&sheet), "Preparing preview..."));
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

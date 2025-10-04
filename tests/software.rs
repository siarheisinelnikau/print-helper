//! Synthetic in-memory tests: no event loop, native window, application or device.
use egui::{Color32, Mesh, Rect, TextureId, TextureOptions, pos2};
use print_helper::software::{BackgroundWake, CpuFrame, RepaintSchedule};
use std::time::{Duration, Instant};

#[test]
fn worker_notifications_keep_waking_without_any_gui_frames() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let wake = BackgroundWake::new(move || {
        seen.fetch_add(1, Ordering::Relaxed);
    });
    let other_worker = wake.clone();
    for _ in 0..100 {
        wake.notify();
        other_worker.notify();
    }
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    wake.acknowledge();
    other_worker.notify();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    other_worker.acknowledge();
    wake.notify();
    assert_eq!(calls.load(Ordering::Relaxed), 3);
}

#[test]
fn input_and_timer_work_settle_back_to_idle() {
    let now = Instant::now();
    let mut schedule = RepaintSchedule::default();
    schedule.set_drawable(true);
    assert!(schedule.queue_redraw(now));
    assert!(!schedule.queue_redraw(now));
    assert!(schedule.begin_redraw(now));
    assert!(!schedule.due(now));
    assert_eq!(schedule.next_deadline(), None);
    let later = now + Duration::from_secs(5);
    schedule.request_at(later);
    schedule.request_at(later + Duration::from_secs(20));
    assert_eq!(schedule.next_deadline(), Some(later));
    assert!(!schedule.queue_redraw(now));
    assert!(schedule.queue_redraw(later));
    // A queued OS paint must not leave an already expired WaitUntil busy-looping.
    assert_eq!(schedule.next_deadline(), None);
    assert!(schedule.begin_redraw(later));
    assert!(!schedule.begin_redraw(later));
    assert!(!schedule.queue_redraw(later + Duration::from_secs(100)));
}

#[test]
fn hidden_work_and_expired_animations_wait_for_restore() {
    let now = Instant::now();
    let mut schedule = RepaintSchedule::default();
    schedule.set_drawable(true);
    schedule.queue_redraw(now);
    schedule.set_drawable(false);
    for n in 0..10 {
        schedule.invalidate();
        schedule.request_at(now + Duration::from_millis(n));
        assert!(!schedule.queue_redraw(now + Duration::from_secs(1)));
        assert!(!schedule.begin_redraw(now + Duration::from_secs(1)));
        assert_eq!(schedule.next_deadline(), None);
    }
    schedule.set_drawable(true);
    assert!(schedule.queue_redraw(now + Duration::from_secs(2)));
    assert!(schedule.begin_redraw(now + Duration::from_secs(2)));
    assert_eq!(schedule.next_deadline(), None);
    assert!(!schedule.queue_redraw(now + Duration::from_secs(3)));
}

#[test]
fn os_exposure_without_changes_can_reuse_the_cached_bitmap() {
    let now = Instant::now();
    let mut schedule = RepaintSchedule::default();
    schedule.set_drawable(true);
    assert!(schedule.begin_redraw(now));
    assert!(!schedule.begin_redraw(now));
    schedule.invalidate();
    assert!(schedule.begin_redraw(now));
    assert!(!schedule.begin_redraw(now));
}

fn mesh(id: TextureId, side: f32) -> Vec<egui::ClippedPrimitive> {
    let mut mesh = Mesh::with_texture(id);
    mesh.add_rect_with_uv(
        Rect::from_min_max(pos2(0.0, 0.0), pos2(side, side)),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    vec![egui::ClippedPrimitive {
        clip_rect: Rect::EVERYTHING,
        primitive: egui::epaint::Primitive::Mesh(mesh),
    }]
}
fn upload(id: TextureId, color: Color32) -> egui::TexturesDelta {
    egui::TexturesDelta {
        set: vec![(
            id,
            egui::epaint::ImageDelta::full(
                egui::ColorImage::filled([4, 4], color),
                TextureOptions::LINEAR,
            ),
        )],
        free: Vec::new(),
    }
}

#[test]
fn cpu_texture_updates_frees_and_bgra_presentation_preserve_colors() {
    let mut frame = CpuFrame::default();
    let id = TextureId::Managed(1);
    frame.retain_textures(upload(id, Color32::from_rgb(240, 20, 10)));
    frame.rasterize(16, 16, &mesh(id, 16.0), 1.0).unwrap();
    let mut pixels = vec![0; 256];
    frame.copy_to(&mut pixels).unwrap();
    assert_eq!(pixels[8 * 16 + 8], 0x00f0_140a);

    frame.retain_textures(egui::TexturesDelta {
        set: vec![(
            id,
            egui::epaint::ImageDelta::partial(
                [0, 0],
                egui::ColorImage::filled([2, 2], Color32::GREEN),
                TextureOptions::LINEAR,
            ),
        )],
        free: Vec::new(),
    });
    frame.rasterize(16, 16, &mesh(id, 16.0), 1.0).unwrap();
    frame.copy_to(&mut pixels).unwrap();
    assert_eq!(pixels[4 * 16 + 4], 0x0000_ff00);
    assert_eq!(pixels[12 * 16 + 12], 0x00f0_140a);

    let next = TextureId::Managed(2);
    let mut delta = upload(next, Color32::BLUE);
    delta.free.push(id);
    frame.retain_textures(delta);
    frame.rasterize(8, 8, &mesh(next, 8.0), 1.0).unwrap();
    assert_eq!(frame.size(), (8, 8));
    let mut smaller = vec![0; 64];
    frame.copy_to(&mut smaller).unwrap();
    assert_eq!(smaller[4 * 8 + 4], 0x0000_00ff);
    frame.rasterize(8, 8, &mesh(next, 8.0), 1.0).unwrap();
    let mut retained = vec![0; 64];
    frame.copy_to(&mut retained).unwrap();
    assert_eq!(retained, smaller);
}

#[test]
fn font_upload_from_a_suppressed_frame_survives_until_the_next_raster() {
    let ctx = egui::Context::default();
    let mut frame = CpuFrame::default();
    let draw = |ctx: &egui::Context| {
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("font-fixture"),
        ))
        .text(
            pos2(12.0, 12.0),
            egui::Align2::LEFT_TOP,
            "CPU text",
            egui::FontId::proportional(20.0),
            Color32::BLACK,
        );
    };
    let input = || egui::RawInput {
        screen_rect: Some(Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 80.0))),
        ..Default::default()
    };
    let hidden = ctx.run(input(), draw);
    assert!(!hidden.textures_delta.set.is_empty());
    frame.retain_textures(hidden.textures_delta);
    // No rasterize call for the suppressed output. The next output reuses the atlas.
    let visible = ctx.run(input(), draw);
    frame.retain_textures(visible.textures_delta);
    let primitives = ctx.tessellate(visible.shapes, visible.pixels_per_point);
    frame
        .rasterize(200, 80, &primitives, visible.pixels_per_point)
        .unwrap();
    let mut pixels = vec![0; 200 * 80];
    frame.copy_to(&mut pixels).unwrap();
    assert!(
        pixels
            .iter()
            .filter(|p| (**p & 0x00ff_ffff) < 0x0020_2020)
            .count()
            > 20
    );
}

#[test]
fn invalid_or_zero_framebuffers_fail_without_losing_the_last_frame() {
    let mut frame = CpuFrame::default();
    frame.rasterize(8, 8, &[], 1.0).unwrap();
    for (w, h, scale) in [
        (0, 8, 1.0),
        (8, 0, 1.0),
        (usize::MAX, 2, 1.0),
        (8, 8, f32::NAN),
        (8, 8, 0.0),
    ] {
        assert!(frame.rasterize(w, h, &[], scale).is_err());
    }
    assert_eq!(frame.size(), (8, 8));
    assert!(frame.copy_to(&mut [0; 1]).is_err());
    frame.copy_to(&mut [0; 64]).unwrap();
}

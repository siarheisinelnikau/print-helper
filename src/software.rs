//! Window-independent CPU rasterizer and reactive redraw policy.
use anyhow::{Result, ensure};
use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

pub const MAX_FRAME_PIXELS: usize = 16_000_000;

pub trait DesktopApp {
    fn update(&mut self, ctx: &egui::Context);
    fn background(&mut self, ctx: &egui::Context);
    fn request_close(&mut self, ctx: &egui::Context) -> bool;
    fn should_exit(&self) -> bool;
}

/// Independent, coalesced worker wakeups also work while no egui frames run.
#[derive(Clone)]
pub struct BackgroundWake(Arc<WakeState>);
struct WakeState {
    pending: AtomicBool,
    send: Box<dyn Fn() + Send + Sync>,
}
impl BackgroundWake {
    pub fn new(send: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(WakeState {
            pending: AtomicBool::new(false),
            send: Box::new(send),
        }))
    }
    pub fn notify(&self) {
        if !self.0.pending.swap(true, Ordering::AcqRel) {
            (self.0.send)();
        }
    }
    pub fn acknowledge(&self) {
        self.0.pending.store(false, Ordering::Release);
    }
}

/// No polling: requests are coalesced, and hidden windows retain work until shown.
#[derive(Debug, Default)]
pub struct RepaintSchedule {
    drawable: bool,
    dirty: bool,
    queued: bool,
    deadline: Option<Instant>,
}
impl RepaintSchedule {
    pub fn set_drawable(&mut self, drawable: bool) {
        if drawable && !self.drawable {
            self.dirty = true;
        }
        self.drawable = drawable;
        if !drawable {
            self.queued = false;
        }
    }
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
    pub fn request_at(&mut self, when: Instant) {
        self.deadline = Some(self.deadline.map_or(when, |old| old.min(when)));
    }
    pub fn due(&self, now: Instant) -> bool {
        self.drawable && (self.dirty || self.deadline.is_some_and(|when| when <= now))
    }
    pub fn queue_redraw(&mut self, now: Instant) -> bool {
        if self.due(now) && !self.queued {
            self.queued = true;
            true
        } else {
            false
        }
    }
    /// False means an OS exposure can reuse the retained bitmap without UI work.
    pub fn begin_redraw(&mut self, now: Instant) -> bool {
        self.queued = false;
        if !self.due(now) {
            return false;
        }
        self.dirty = false;
        self.deadline = None;
        true
    }
    pub fn next_deadline(&self) -> Option<Instant> {
        if self.drawable && !self.queued {
            self.deadline
        } else {
            None
        }
    }
    pub fn drawable(&self) -> bool {
        self.drawable
    }
}

pub struct CpuFrame {
    renderer: EguiSoftwareRender,
    pixels: Vec<[u8; 4]>,
    size: (usize, usize),
    pending: egui::TexturesDelta,
}
impl Default for CpuFrame {
    fn default() -> Self {
        Self {
            // 0.0.2 caches meshes without texture-content revisions. A partial
            // texture update otherwise leaves unchanged meshes showing old pixels.
            // Retained whole-window frames still avoid idle/exposure rasterization.
            renderer: EguiSoftwareRender::new(ColorFieldOrder::Bgra).with_caching(false),
            pixels: Vec::new(),
            size: (0, 0),
            pending: Default::default(),
        }
    }
}
impl CpuFrame {
    /// Retain uploads/frees if a frame is suppressed by a hide/minimize transition.
    pub fn retain_textures(&mut self, delta: egui::TexturesDelta) {
        self.pending.append(delta);
    }
    pub fn rasterize(
        &mut self,
        width: usize,
        height: usize,
        primitives: &[egui::ClippedPrimitive],
        pixels_per_point: f32,
    ) -> Result<()> {
        let len = width
            .checked_mul(height)
            .filter(|n| *n <= MAX_FRAME_PIXELS)
            .ok_or_else(|| anyhow::anyhow!("Window framebuffer exceeds 16 megapixels"))?;
        ensure!(
            width > 0 && height > 0 && pixels_per_point.is_finite() && pixels_per_point > 0.0,
            "Invalid framebuffer dimensions or display scale"
        );
        self.pixels.resize(len, [0; 4]);
        // BGRA paper background, before compositing egui's premultiplied colors.
        self.pixels.fill([251, 250, 248, 255]);
        self.renderer.render(
            &mut BufferMutRef::new(&mut self.pixels, width, height),
            primitives,
            &self.pending,
            pixels_per_point,
        );
        self.pending.clear();
        self.size = (width, height);
        Ok(())
    }
    pub fn size(&self) -> (usize, usize) {
        self.size
    }
    /// softbuffer's native format is 0x00RRGGBB. No GPU conversion or upload.
    pub fn copy_to(&self, destination: &mut [u32]) -> Result<()> {
        ensure!(
            destination.len() == self.pixels.len(),
            "Framebuffer size changed before presentation"
        );
        for (output, pixel) in destination.iter_mut().zip(&self.pixels) {
            *output = u32::from_le_bytes(*pixel) & 0x00ff_ffff;
        }
        Ok(())
    }
}

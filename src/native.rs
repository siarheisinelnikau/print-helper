//! Windows-only egui host. Rasterization is CPU-only; softbuffer presents via GDI.
use anyhow::{Context as _, Result};
use print_helper::software::{
    BackgroundWake, CpuFrame, DesktopApp, MAX_FRAME_PIXELS, RepaintSchedule,
};
use serde::Serialize;
use std::{num::NonZeroU32, path::PathBuf, sync::Arc, time::Instant};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    platform::windows::{IconExtWindows, WindowAttributesExtWindows},
    window::{Icon, Window, WindowId},
};

#[derive(Debug)]
enum Wake {
    Background,
    Repaint { at: Instant, pass: u64 },
}

#[derive(Default, Serialize)]
struct Diagnostics {
    backend: &'static str,
    ui_frames: u64,
    raster_frames: u64,
    presentations: u64,
    exposure_presentations: u64,
    suppressed_redraws: u64,
    background_wakeups: u64,
    max_ui_us: u64,
    max_raster_us: u64,
    max_present_us: u64,
}

struct NativeWindow {
    // Drop native drawing resources before their HWND owner.
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    _display: softbuffer::Context<Arc<Window>>,
    input: egui_winit::State,
    window: Arc<Window>,
    info: egui::ViewportInfo,
    occluded: bool,
}

type Factory = Box<dyn FnOnce(&egui::Context, BackgroundWake) -> Box<dyn DesktopApp>>;
struct Host {
    ctx: egui::Context,
    factory: Option<Factory>,
    app: Option<Box<dyn DesktopApp>>,
    background_wake: BackgroundWake,
    native: Option<NativeWindow>,
    schedule: RepaintSchedule,
    frame: CpuFrame,
    diagnostics: Diagnostics,
    failure: Option<anyhow::Error>,
    suspended: bool,
}

pub fn run(
    factory: impl FnOnce(&egui::Context, BackgroundWake) -> Box<dyn DesktopApp> + 'static,
    diagnostics_path: Option<PathBuf>,
) -> Result<()> {
    let event_loop = EventLoop::<Wake>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let ctx = egui::Context::default();
    ctx.set_embed_viewports(true);
    let proxy = event_loop.create_proxy();
    let worker_proxy = proxy.clone();
    let background_wake = BackgroundWake::new(move || {
        let _ = worker_proxy.send_event(Wake::Background);
    });
    ctx.set_request_repaint_callback(move |request| {
        if request.viewport_id == egui::ViewportId::ROOT
            && let Some(at) = Instant::now().checked_add(request.delay)
        {
            let _ = proxy.send_event(Wake::Repaint {
                at,
                pass: request.current_cumulative_pass_nr,
            });
        }
    });
    let mut host = Host {
        ctx,
        factory: Some(Box::new(factory)),
        app: None,
        background_wake,
        native: None,
        schedule: Default::default(),
        frame: Default::default(),
        diagnostics: Diagnostics {
            backend: "egui_software_backend 0.0.2 / softbuffer 0.4.8 / Win32 GDI",
            ..Default::default()
        },
        failure: None,
        suspended: false,
    };
    let result = event_loop.run_app(&mut host);
    host.shutdown();
    if let Some(path) = diagnostics_path {
        print_helper::output::save_new(&path, &serde_json::to_vec_pretty(&host.diagnostics)?)
            .context("Cannot save rendering diagnostics; choose a new filename")?;
    }
    result?;
    if let Some(error) = host.failure {
        return Err(error);
    }
    Ok(())
}

impl Host {
    fn shutdown(&mut self) {
        self.ctx.set_request_repaint_callback(|_| {});
        self.app.take();
        self.native.take();
    }
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.failure = Some(error);
        self.shutdown();
        event_loop.exit();
    }
    fn sync_visibility(&mut self) {
        let drawable = !self.suspended
            && self.native.as_ref().is_some_and(|n| {
                let size = n.window.inner_size();
                !n.occluded
                    && n.window.is_visible() != Some(false)
                    && n.window.is_minimized() != Some(true)
                    && size.width > 0
                    && size.height > 0
                    && (size.width as usize)
                        .checked_mul(size.height as usize)
                        .is_some_and(|pixels| pixels <= MAX_FRAME_PIXELS)
            });
        self.schedule.set_drawable(drawable);
    }
    fn background(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(app) = &mut self.app {
            app.background(&self.ctx);
            if app.should_exit() {
                self.shutdown();
                event_loop.exit();
            }
        }
    }
    fn close(&mut self, event_loop: &ActiveEventLoop) {
        let close = self
            .app
            .as_mut()
            .is_none_or(|app| app.request_close(&self.ctx));
        if close {
            self.shutdown();
            event_loop.exit();
        } else if let Some(native) = &mut self.native {
            // A close decision must remain accessible even if requested while minimized.
            native.window.set_minimized(false);
            native.window.set_visible(true);
            native.occluded = false;
            self.sync_visibility();
            self.schedule.invalidate();
        }
    }
    fn present(&mut self, exposure: bool) -> Result<()> {
        let native = self.native.as_mut().context("Window is unavailable")?;
        let (width, height) = self.frame.size();
        let width = NonZeroU32::new(width as u32).context("Empty frame")?;
        let height = NonZeroU32::new(height as u32).context("Empty frame")?;
        let started = Instant::now();
        native
            .surface
            .resize(width, height)
            .map_err(|e| anyhow::anyhow!("GDI surface resize: {e}"))?;
        let mut buffer = native
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("GDI buffer: {e}"))?;
        self.frame.copy_to(&mut buffer)?;
        buffer
            .present()
            .map_err(|e| anyhow::anyhow!("GDI presentation: {e}"))?;
        self.diagnostics.presentations += 1;
        if self.diagnostics.presentations == 1 {
            print_helper::updater::cleanup_after_startup();
        }
        self.diagnostics.exposure_presentations += u64::from(exposure);
        self.diagnostics.max_present_us = self
            .diagnostics
            .max_present_us
            .max(started.elapsed().as_micros() as u64);
        Ok(())
    }
    fn redraw(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        self.background(event_loop);
        self.sync_visibility();
        if !self.schedule.drawable() {
            self.schedule.begin_redraw(Instant::now());
            self.diagnostics.suppressed_redraws += 1;
            return Ok(());
        }
        let size = self
            .native
            .as_ref()
            .context("Window is unavailable")?
            .window
            .inner_size();
        if self.frame.size() != (size.width as usize, size.height as usize) {
            self.schedule.invalidate();
        }
        if !self.schedule.begin_redraw(Instant::now()) {
            if self.frame.size() != (0, 0) {
                self.present(true)?;
            }
            return Ok(());
        }
        let native = self.native.as_mut().context("Window is unavailable")?;
        let size = native.window.inner_size();
        egui_winit::update_viewport_info(&mut native.info, &self.ctx, &native.window, false);
        let mut input = native.input.take_egui_input(&native.window);
        input
            .viewports
            .insert(egui::ViewportId::ROOT, native.info.clone());
        let app = self.app.as_mut().context("Application is unavailable")?;
        let started = Instant::now();
        let output = self.ctx.run(input, |ctx| app.update(ctx));
        self.diagnostics.ui_frames += 1;
        self.diagnostics.max_ui_us = self
            .diagnostics
            .max_ui_us
            .max(started.elapsed().as_micros() as u64);
        self.frame.retain_textures(output.textures_delta);
        native
            .input
            .handle_platform_output(&native.window, output.platform_output);
        let mut close = false;
        if let Some(root) = output.viewport_output.get(&egui::ViewportId::ROOT) {
            close = root.commands.contains(&egui::ViewportCommand::Close)
                && !root.commands.contains(&egui::ViewportCommand::CancelClose);
            let mut actions = Vec::new();
            egui_winit::process_viewport_commands(
                &self.ctx,
                &mut native.info,
                root.commands
                    .iter()
                    .filter(|cmd| {
                        !matches!(
                            cmd,
                            egui::ViewportCommand::Close | egui::ViewportCommand::CancelClose
                        )
                    })
                    .cloned(),
                &native.window,
                &mut actions,
            );
            for action in actions {
                match action {
                    egui_winit::ActionRequested::Cut => {
                        native.input.egui_input_mut().events.push(egui::Event::Cut)
                    }
                    egui_winit::ActionRequested::Copy => {
                        native.input.egui_input_mut().events.push(egui::Event::Copy)
                    }
                    egui_winit::ActionRequested::Paste => {
                        if let Some(text) = native.input.clipboard_text() {
                            native
                                .input
                                .egui_input_mut()
                                .events
                                .push(egui::Event::Paste(text));
                        }
                    }
                    // PrintHelper exposes no native screenshot command or secondary viewport.
                    egui_winit::ActionRequested::Screenshot(_) => {}
                }
                self.schedule.invalidate();
            }
            if let Some(at) = Instant::now().checked_add(root.repaint_delay) {
                self.schedule.request_at(at);
            }
        }
        if close {
            self.close(event_loop);
        }
        self.background(event_loop);
        self.sync_visibility();
        if !self.schedule.drawable() {
            return Ok(());
        }
        if self
            .native
            .as_ref()
            .is_some_and(|n| n.window.inner_size() != size)
        {
            self.schedule.invalidate();
            return Ok(());
        }
        let started = Instant::now();
        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        self.frame.rasterize(
            size.width as usize,
            size.height as usize,
            &primitives,
            output.pixels_per_point,
        )?;
        self.diagnostics.raster_frames += 1;
        self.diagnostics.max_raster_us = self
            .diagnostics
            .max_raster_us
            .max(started.elapsed().as_micros() as u64);
        self.present(false)
    }
}

impl ApplicationHandler<Wake> for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.suspended = false;
        let result = (|| -> Result<()> {
            if self.native.is_some() {
                self.sync_visibility();
                return Ok(());
            }
            let window = Arc::new(
                event_loop.create_window(
                    Window::default_attributes()
                        .with_title("PrintHelper")
                        .with_window_icon(Some(Icon::from_resource(
                            1,
                            Some(PhysicalSize::new(32, 32)),
                        )?))
                        .with_taskbar_icon(Some(Icon::from_resource(
                            1,
                            Some(PhysicalSize::new(256, 256)),
                        )?))
                        .with_inner_size(LogicalSize::new(1220.0, 850.0))
                        .with_min_inner_size(LogicalSize::new(960.0, 640.0))
                        .with_max_inner_size(PhysicalSize::new(4000, 4000)),
                )?,
            );
            let display = softbuffer::Context::new(window.clone())
                .map_err(|e| anyhow::anyhow!("GDI display initialization: {e}"))?;
            let surface = softbuffer::Surface::new(&display, window.clone())
                .map_err(|e| anyhow::anyhow!("GDI surface initialization: {e}"))?;
            let input = egui_winit::State::new(
                self.ctx.clone(),
                egui::ViewportId::ROOT,
                &*window,
                Some(window.scale_factor() as f32),
                window.theme(),
                Some(8192),
            );
            let mut info = egui::ViewportInfo::default();
            egui_winit::update_viewport_info(&mut info, &self.ctx, &window, true);
            self.native = Some(NativeWindow {
                surface,
                _display: display,
                window,
                input,
                info,
                occluded: false,
            });
            if let Some(factory) = self.factory.take() {
                self.app = Some(factory(&self.ctx, self.background_wake.clone()));
            }
            self.sync_visibility();
            self.schedule.invalidate();
            Ok(())
        })();
        if let Err(error) = result {
            self.fail(event_loop, error);
        }
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Wake) {
        self.diagnostics.background_wakeups += 1;
        match event {
            Wake::Background => {
                self.background_wake.acknowledge();
                self.background(event_loop);
                self.schedule.invalidate();
            }
            Wake::Repaint { at, pass } => {
                if pass.saturating_add(1) >= self.ctx.cumulative_pass_nr() {
                    self.schedule.request_at(at);
                }
            }
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self
            .native
            .as_ref()
            .is_none_or(|native| native.window.id() != id)
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => self.close(event_loop),
            WindowEvent::Destroyed => {
                self.shutdown();
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.redraw(event_loop) {
                    self.fail(event_loop, error);
                }
            }
            event => {
                let native = self.native.as_mut().expect("checked window");
                if let WindowEvent::Occluded(value) = &event {
                    native.occluded = *value;
                }
                let repaint = native.input.on_window_event(&native.window, &event).repaint;
                if repaint
                    || matches!(
                        event,
                        WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }
                    )
                {
                    self.schedule.invalidate();
                }
                self.sync_visibility();
            }
        }
    }
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.suspended = true;
        self.schedule.set_drawable(false);
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.sync_visibility();
        if self.schedule.queue_redraw(Instant::now())
            && let Some(native) = &self.native
        {
            native.window.request_redraw();
        }
        // An OS redraw request wakes Wait; no ControlFlow::Poll or idle timer.
        event_loop.set_control_flow(
            self.schedule
                .next_deadline()
                .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
        );
    }
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.shutdown();
    }
}

//! Runs the app in a winit window drawn on the CPU.
//!
//! egui's usual backends draw with OpenGL or wgpu. On Nvidia cards that can
//! make the driver treat the window like a game (overlay, G-Sync, game
//! profiles), and with G-Sync on for windowed apps the refresh rate follows
//! the app's redraws, which flickers. Drawing the small UI on the CPU and
//! handing finished frames to the OS (`softbuffer`) avoids the GPU path
//! entirely, so the app only redraws when something changes.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use serde::{Deserialize, Serialize};

use crate::taskbar::Taskbar;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle};
use winit::monitor::MonitorHandle;
use winit::window::{Window, WindowId};

/// Where the window was and how large, to open it there again.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    /// Position of the window's outer top-left corner, in physical pixels.
    pub x: i32,
    pub y: i32,
    /// Size of the window's inside, in points.
    pub width: f32,
    pub height: f32,
}

/// What the window needs from the app.
pub trait App {
    fn ui(&mut self, ui: &mut egui::Ui);

    /// Where to open the window, if it should open where it was.
    fn place(&self) -> Option<Place> {
        None
    }

    /// Called once before the window closes, with its place unless it is
    /// minimized, maximized or full screen.
    fn on_exit(&mut self, _place: Option<Place>) {}

    /// Whether the window opens hidden, as when hdrshot starts in the tray.
    fn starts_hidden(&self) -> bool {
        false
    }

    /// Whether closing the window only hides it, as hdrshot then keeps
    /// running in the tray.
    fn hides_on_close(&self) -> bool {
        false
    }

    /// Called when closing the window hid it, with its place as for
    /// [`App::on_exit`].
    fn on_hide(&mut self, _place: Option<Place>) {}

    /// Whether the app asks to quit, e.g. from the tray icon's menu.
    fn wants_quit(&self) -> bool {
        false
    }

    /// Files pasted with Ctrl+V (Cmd+V on macOS), e.g. copied in Explorer.
    /// egui only passes on pasted text, so the window reads files itself.
    fn paste_files(&mut self, _files: Vec<std::path::PathBuf>) {}

    /// Progress (0 to 1) to show on the taskbar button, or `None`.
    fn taskbar_progress(&self) -> Option<f32> {
        None
    }
}

/// Opens the window and runs until it is closed.
pub fn run<A: App>(
    viewport: egui::ViewportBuilder,
    make_app: impl FnOnce(&egui::Context) -> A,
) -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::<Repaint>::with_user_event().build()?;
    let ctx = egui::Context::default();
    // Glyphs the bundled fonts lack, e.g. CJK file names, come from the
    // system fonts, as in eframe.
    ctx.add_font_provider(Arc::new(
        egui_system_font_provider::SystemFontProvider::new(),
    ));
    let proxy = event_loop.create_proxy();
    ctx.set_request_repaint_callback(move |info| {
        // Fails only once the event loop is gone, when nothing needs drawing.
        let _ = proxy.send_event(Repaint(Instant::now() + info.delay));
    });
    let app = make_app(&ctx);
    let softbuffer = softbuffer::Context::new(event_loop.owned_display_handle())
        .expect("softbuffer supports every platform winit runs on");
    let mut runner = Runner {
        ctx,
        softbuffer,
        viewport,
        app,
        running: None,
        next_repaint: None,
        last_background_frame: None,
    };
    event_loop.run_app(&mut runner)
}

/// Asks for a redraw at the given time.
struct Repaint(Instant);

struct Runner<A> {
    ctx: egui::Context,
    softbuffer: softbuffer::Context<OwnedDisplayHandle>,
    viewport: egui::ViewportBuilder,
    app: A,
    running: Option<Running>,
    /// Earliest pending repaint request that lies in the future.
    next_repaint: Option<Instant>,
    /// When a frame last ran while the window was hidden or minimized.
    last_background_frame: Option<Instant>,
}

/// Least time between frames while the window is hidden or minimized, so
/// something animating, like a spinner, doesn't keep the CPU busy.
const BACKGROUND_FRAME: Duration = Duration::from_millis(100);

/// The window and everything tied to it.
///
/// Fields drop in declaration order: the surface and egui-winit's clipboard
/// must go before the window they use.
struct Running {
    surface: softbuffer::Surface<OwnedDisplayHandle, Arc<Window>>,
    egui_winit: egui_winit::State,
    renderer: EguiSoftwareRender,
    info: egui::ViewportInfo,
    modifiers: winit::keyboard::ModifiersState,
    taskbar: Taskbar,
    /// Taskbar progress last shown, in thousandths.
    progress: Option<u32>,
    /// Texture changes not yet handed to the renderer, kept when a frame
    /// can't be drawn so the next one still gets them.
    textures: egui::TexturesDelta,
    window: Arc<Window>,
}

impl<A: App> Runner<A> {
    fn open(&mut self, event_loop: &ActiveEventLoop) {
        let monitors: Vec<MonitorHandle> = event_loop.available_monitors().collect();
        let mut viewport = self.viewport.clone();
        if self.app.starts_hidden() {
            viewport = viewport.with_visible(false);
        }
        let mut position = None;
        let mut monitor = None;
        if let Some(place) = self.app.place() {
            viewport.inner_size = Some(egui::vec2(place.width, place.height));
            // Back where it was only if a monitor still shows its title bar.
            let grip = (place.x + 100, place.y + 10);
            monitor = monitors.iter().find(|m| shows(m, grip)).cloned();
            if monitor.is_some() {
                position = Some(PhysicalPosition::new(place.x, place.y));
            }
        }
        if let Some(monitor) = monitor
            .or_else(|| event_loop.primary_monitor())
            .or_else(|| monitors.first().cloned())
        {
            fit_on(&mut viewport, &monitor);
        }
        let mut attributes =
            egui_winit::create_winit_window_attributes(&self.ctx, viewport.clone());
        if let Some(position) = position {
            attributes = attributes.with_position(position);
        }
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(e) => {
                eprintln!("Couldn't open the window: {e}");
                event_loop.exit();
                return;
            }
        };
        egui_winit::apply_viewport_builder_to_window(&self.ctx, &window, &viewport);
        let surface = match softbuffer::Surface::new(&self.softbuffer, window.clone()) {
            Ok(surface) => surface,
            Err(e) => {
                eprintln!("Couldn't draw to the window: {e}");
                event_loop.exit();
                return;
            }
        };
        let egui_winit = egui_winit::State::new(
            self.ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            window.theme(),
            None,
        );
        let mut info = egui::ViewportInfo::default();
        egui_winit::update_viewport_info(&mut info, &self.ctx, &window, true);
        self.running = Some(Running {
            surface,
            egui_winit,
            renderer: EguiSoftwareRender::new(ColorFieldOrder::Bgra),
            info,
            modifiers: Default::default(),
            textures: Default::default(),
            taskbar: Taskbar::default(),
            progress: None,
            window,
        });
        self.wake();
    }

    fn schedule(&mut self, when: Instant) {
        if when <= Instant::now() {
            self.wake();
        } else if self.next_repaint.is_none_or(|next| when < next) {
            self.next_repaint = Some(when);
        }
    }

    /// Runs a frame soon. A hidden or minimized window may get no redraw
    /// events, but the app must keep running, e.g. to convert new images
    /// from the watch folder or follow a batch on the taskbar, so then the
    /// frame runs right away, or once [`BACKGROUND_FRAME`] has passed.
    fn wake(&mut self) {
        let Some(running) = &self.running else {
            return;
        };
        let window = &running.window;
        if window.is_visible() != Some(false) && window.is_minimized() != Some(true) {
            window.request_redraw();
            return;
        }
        let now = Instant::now();
        if let Some(next) = self.last_background_frame.map(|t| t + BACKGROUND_FRAME)
            && next > now
        {
            self.schedule(next);
            return;
        }
        self.last_background_frame = Some(now);
        self.redraw();
    }

    /// Runs the app for one frame and draws it. While hidden or minimized
    /// the app still runs, but nothing is drawn.
    fn redraw(&mut self) {
        let Some(running) = &mut self.running else {
            return;
        };

        egui_winit::update_viewport_info(&mut running.info, &self.ctx, &running.window, false);
        let mut input = running.egui_winit.take_egui_input(&running.window);
        input
            .viewports
            .insert(egui::ViewportId::ROOT, running.info.clone());
        let app = &mut self.app;
        let mut output = self.ctx.run_ui(input, |ui| app.ui(ui));
        running
            .egui_winit
            .handle_platform_output(&running.window, std::mem::take(&mut output.platform_output));

        let repaint_delay =
            output
                .viewport_output
                .remove(&egui::ViewportId::ROOT)
                .map(|viewport| {
                    let mut actions = Vec::new();
                    egui_winit::process_viewport_commands(
                        &self.ctx,
                        &mut running.info,
                        viewport.commands,
                        &running.window,
                        &mut actions,
                    );
                    viewport.repaint_delay
                });
        running
            .textures
            .append(std::mem::take(&mut output.textures_delta));

        let progress = self
            .app
            .taskbar_progress()
            .map(|p| (p.clamp(0.0, 1.0) * 1000.0) as u32);
        if progress != running.progress {
            running.progress = progress;
            running
                .taskbar
                .set_progress(&running.window, progress.map(|p| p as f32 / 1000.0));
        }

        let size = running.window.inner_size();
        if running.window.is_visible() != Some(false)
            && let (Some(width), Some(height)) =
                (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        {
            let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
            running.draw(width, height, &primitives, output.pixels_per_point);
        }

        if let Some(delay) = repaint_delay
            && let Some(when) = Instant::now().checked_add(delay)
        {
            self.schedule(when);
        }
    }
}

impl<A: App> Runner<A> {
    fn quit(&mut self, event_loop: &ActiveEventLoop) {
        let place = self.running.as_ref().and_then(|r| place(&r.window));
        // Hidden first, since the app may still finish writing a file.
        if let Some(running) = &self.running {
            running.window.set_visible(false);
        }
        self.app.on_exit(place);
        event_loop.exit();
    }
}

impl Running {
    fn draw(
        &mut self,
        width: NonZeroU32,
        height: NonZeroU32,
        primitives: &[egui::ClippedPrimitive],
        pixels_per_point: f32,
    ) {
        if let Err(e) = self.surface.resize(width, height) {
            eprintln!("Couldn't resize the window buffer: {e}");
            return;
        }
        let mut buffer = match self.surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(e) => {
                eprintln!("Couldn't get the window buffer: {e}");
                return;
            }
        };
        self.renderer.render(
            &mut BufferMutRef::new(
                bytemuck::cast_slice_mut(&mut buffer),
                width.get() as usize,
                height.get() as usize,
            ),
            primitives,
            &self.textures,
            pixels_per_point,
        );
        self.textures.clear();
        if let Err(e) = buffer.present() {
            eprintln!("Couldn't show the frame: {e}");
        }
    }
}

impl<A: App> ApplicationHandler<Repaint> for Runner<A> {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            self.next_repaint = None;
            self.wake();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_none() {
            self.open(event_loop);
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, Repaint(when): Repaint) {
        self.schedule(when);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(running) = &mut self.running else {
            return;
        };
        match event {
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::CloseRequested if self.app.hides_on_close() => {
                let place = place(&running.window);
                running.window.set_visible(false);
                self.app.on_hide(place);
            }
            WindowEvent::CloseRequested => self.quit(event_loop),
            event => {
                if let WindowEvent::ModifiersChanged(modifiers) = &event {
                    running.modifiers = modifiers.state();
                }
                if is_paste(&event, running.modifiers)
                    && let Ok(files) =
                        arboard::Clipboard::new().and_then(|mut c| c.get().file_list())
                    && !files.is_empty()
                {
                    self.app.paste_files(files);
                    running.window.request_redraw();
                }
                if running
                    .egui_winit
                    .on_window_event(&running.window, &event)
                    .repaint
                {
                    running.window.request_redraw();
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.wants_quit() && !event_loop.exiting() {
            self.quit(event_loop);
        }
        event_loop.set_control_flow(match self.next_repaint {
            Some(when) => ControlFlow::WaitUntil(when),
            None => ControlFlow::Wait,
        });
    }
}

/// Whether `monitor` shows the physical pixel `point`.
fn shows(monitor: &MonitorHandle, point: (i32, i32)) -> bool {
    let (pos, size) = (monitor.position(), monitor.size());
    (pos.x..pos.x + size.width as i32).contains(&point.0)
        && (pos.y..pos.y + size.height as i32).contains(&point.1)
}

/// Shrinks the window to fit on `monitor`, leaving room for the title bar
/// and the taskbar, but not below its smallest size.
fn fit_on(viewport: &mut egui::ViewportBuilder, monitor: &MonitorHandle) {
    let Some(size) = viewport.inner_size.as_mut() else {
        return;
    };
    let screen = monitor.size().to_logical::<f32>(monitor.scale_factor());
    size.x = size.x.min(screen.width - 40.0);
    size.y = size.y.min(screen.height - 100.0);
    if let Some(min) = viewport.min_inner_size {
        *size = size.max(min);
    }
}

/// The window's place, unless it is minimized, maximized or full screen.
fn place(window: &Window) -> Option<Place> {
    if window.is_minimized() == Some(true) || window.is_maximized() || window.fullscreen().is_some()
    {
        return None;
    }
    let position = window.outer_position().ok()?;
    let size = window.inner_size().to_logical::<f32>(window.scale_factor());
    Some(Place {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

fn is_paste(event: &WindowEvent, modifiers: winit::keyboard::ModifiersState) -> bool {
    use winit::keyboard::{Key, NamedKey};
    let WindowEvent::KeyboardInput { event, .. } = event else {
        return false;
    };
    let command = if cfg!(target_os = "macos") {
        modifiers.super_key()
    } else {
        modifiers.control_key()
    };
    event.state.is_pressed()
        && !event.repeat
        && match &event.logical_key {
            Key::Character(c) => command && c.eq_ignore_ascii_case("v"),
            Key::Named(NamedKey::Paste) => true,
            _ => false,
        }
}

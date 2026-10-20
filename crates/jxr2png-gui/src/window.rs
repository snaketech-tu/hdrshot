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
use std::time::Instant;

use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle};
use winit::window::{Window, WindowId};

/// What the window needs from the app.
pub trait App {
    fn ui(&mut self, ui: &mut egui::Ui);

    /// Called once before the window closes.
    fn on_exit(&mut self) {}

    /// Files pasted with Ctrl+V (Cmd+V on macOS), e.g. copied in Explorer.
    /// egui only passes on pasted text, so the window reads files itself.
    fn paste_files(&mut self, _files: Vec<std::path::PathBuf>) {}
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
}

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
    /// Texture changes not yet handed to the renderer, kept when a frame
    /// can't be drawn so the next one still gets them.
    textures: egui::TexturesDelta,
    window: Arc<Window>,
}

impl<A: App> Runner<A> {
    fn open(&mut self, event_loop: &ActiveEventLoop) {
        let attributes =
            egui_winit::create_winit_window_attributes(&self.ctx, self.viewport.clone());
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(e) => {
                eprintln!("Couldn't open the window: {e}");
                event_loop.exit();
                return;
            }
        };
        egui_winit::apply_viewport_builder_to_window(&self.ctx, &window, &self.viewport);
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
        window.request_redraw();
        self.running = Some(Running {
            surface,
            egui_winit,
            renderer: EguiSoftwareRender::new(ColorFieldOrder::Bgra),
            info,
            modifiers: Default::default(),
            textures: Default::default(),
            window,
        });
    }

    fn schedule(&mut self, when: Instant) {
        if when <= Instant::now() {
            if let Some(running) = &self.running {
                running.window.request_redraw();
            }
        } else if self.next_repaint.is_none_or(|next| when < next) {
            self.next_repaint = Some(when);
        }
    }

    fn redraw(&mut self) {
        let Some(running) = &mut self.running else {
            return;
        };
        let size = running.window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            // Minimized: nothing to draw.
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
        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        if let Err(e) = running.surface.resize(width, height) {
            eprintln!("Couldn't resize the window buffer: {e}");
            return;
        }
        let mut buffer = match running.surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(e) => {
                eprintln!("Couldn't get the window buffer: {e}");
                return;
            }
        };
        running.renderer.render(
            &mut BufferMutRef::new(
                bytemuck::cast_slice_mut(&mut buffer),
                width.get() as usize,
                height.get() as usize,
            ),
            &primitives,
            &running.textures,
            output.pixels_per_point,
        );
        running.textures.clear();
        if let Err(e) = buffer.present() {
            eprintln!("Couldn't show the frame: {e}");
        }

        if let Some(delay) = repaint_delay
            && let Some(when) = Instant::now().checked_add(delay)
        {
            self.schedule(when);
        }
    }
}

impl<A: App> ApplicationHandler<Repaint> for Runner<A> {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause
            && let Some(running) = &self.running
        {
            running.window.request_redraw();
            self.next_repaint = None;
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
            WindowEvent::CloseRequested => {
                self.app.on_exit();
                running.window.set_visible(false);
                event_loop.exit();
            }
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
        event_loop.set_control_flow(match self.next_repaint {
            Some(when) => ControlFlow::WaitUntil(when),
            None => ControlFlow::Wait,
        });
    }
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

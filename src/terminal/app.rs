use anyhow::Result;
use std::sync::{Arc, Mutex};
use std::io::Write;
use std::time::Instant;
use winit::{
    event::{WindowEvent},
    event_loop::{EventLoop, ActiveEventLoop},
    window::WindowAttributes,
    dpi::{LogicalSize},
};
use wgpu::{
    Device, DeviceDescriptor, Features, Instance, Limits, Queue, SurfaceConfiguration,
    TextureFormat, TextureUsages, PresentMode,
};

use crate::terminal::{
    config::{ATLAS_SIZE},
    gpu::GpuResources,
    input::handle_input,
    render::render_frame,
    texture::GlyphAtlas,
    window::TerminalWindow,
    Terminal,
    TerminalState,
};

pub struct TerminalApp {
    pub window: Option<TerminalWindow>,
    pub instance: Instance,
    pub config: SurfaceConfiguration,
    pub device: Device,
    pub queue: Queue,
    pub state: TerminalState,
    pub input_writer: Arc<Mutex<dyn Write + Send>>,
    pub _child_process: Arc<Mutex<Box<dyn portable_pty::Child + Send>>>, // Keep child process alive
}

impl TerminalApp {
    pub fn new(
        window_attributes: WindowAttributes,
        instance: Instance,
        config: SurfaceConfiguration,
        device: Device,
        queue: Queue,
        state: TerminalState,
        input_writer: Arc<Mutex<dyn Write + Send>>,
        child_process: Arc<Mutex<Box<dyn portable_pty::Child + Send>>>,
    ) -> Self {
        Self {
            window: None,
            instance,
            config,
            device,
            queue,
            state,
            input_writer,
            _child_process: child_process,
        }
    }

    pub fn run() -> Result<()> {
        pollster::block_on(async {
            let event_loop = EventLoop::new()?;
            let window_attributes = WindowAttributes::default()
                .with_title("Nebula")
                .with_inner_size(LogicalSize::new(1600.0, 900.0));

            let instance = wgpu::Instance::default();
            let adapter = instance
                .enumerate_adapters(wgpu::Backends::all())
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("Failed to find suitable GPU adapter"))?;

            let (device, queue) = adapter
                .request_device(
                    &DeviceDescriptor {
                        label: None,
                        required_features: Features::empty(),
                        required_limits: Limits::default(),
                        ..Default::default()
                    }
                )
                .await?;

            let config = SurfaceConfiguration {
                usage: TextureUsages::RENDER_ATTACHMENT,
                format: TextureFormat::Bgra8UnormSrgb,
                width: 1600,
                height: 900,
                present_mode: PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };

            let glyph_atlas = GlyphAtlas::new(&device, ATLAS_SIZE);
            let gpu_resources = GpuResources::new(
                            &device, 
                            &config,
                            glyph_atlas.bind_group_layout()
                        );

            let terminal = Terminal::new();
            let (input_writer, child_process) = terminal.spawn_pty()?;
            let start_time = Instant::now();
            let last_frame_time = start_time;

            let state = TerminalState {
                font_system: terminal.font_system,
                buffer: terminal.buffer,
                text_content: terminal.text_content,
                glyph_atlas,
                swash_cache: cosmic_text::SwashCache::new(),
                gpu_resources,
                start_time,
                last_frame_time,
                focused: true,
                dirty: true,
                cursor_x: 16.0, // After "$ " (2 chars * 8px)
                cursor_y: 20.0,  // First line
                cursor_visible: true,
                last_text: String::from("Nebula\n$ "), // Track last text for changes
            };

            let mut app = TerminalApp::new(
                window_attributes,
                instance,
                config,
                device,
                queue,
                state,
                input_writer,
                child_process,
            );

            event_loop.run_app(&mut app)?;
            Ok(())
        })
    }
}

impl winit::application::ApplicationHandler for TerminalApp {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: winit::event::StartCause) {
        if let Some(window) = &self.window {
            window.window.request_redraw();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            match TerminalWindow::new(
                event_loop,
                WindowAttributes::default()
                    .with_title("Nebula")
                    .with_inner_size(LogicalSize::new(1600.0, 900.0)),
                &self.instance,
            ) {
                Ok(window) => {
                    window.configure_surface(&self.device, &self.config);
                    self.window = Some(window);
                }
                Err(e) => eprintln!("Failed to create window: {}", e),
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = &self.window else { return };
        if window.window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::Resized(size) => {
                window.handle_resize(&self.device, &mut self.config, size);
                
                if let Ok(mut buffer_lock) = self.state.buffer.lock() {
                    if let Ok(mut fs) = self.state.font_system.lock() {
                        buffer_lock.set_size(
                            &mut fs, 
                            Some(size.width as f32), 
                            Some(size.height as f32)
                        );
                    }
                }
                window.window.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if self.state.focused {
                    if let Ok(mut writer) = self.input_writer.lock() {
                        let _ = handle_input(&event, &mut *writer, &mut self.state);
                        // State marked dirty in handle_input
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                render_frame(
                    &self.device,
                    &self.queue,                                                         
                    &self.config,
                    window,
                    &mut self.state
                );
            }
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Focused(f) => {
                self.state.focused = f;
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let mut text_changed = false;
        if let Ok(text) = self.state.text_content.lock() {
            if *text != self.state.last_text {
                self.state.last_text = text.clone();
                text_changed = true;
            }
        }
        
        if text_changed {
            self.state.dirty = true;
        }
        
        if self.state.dirty {
            if let Some(window) = &self.window {
                window.window.request_redraw();
            }
        }
    }
}
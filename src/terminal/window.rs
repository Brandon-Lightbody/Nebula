// src/terminal/window.rs
use std::sync::Arc;
use winit::{
    dpi::LogicalSize,
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes, WindowLevel},
};
use wgpu::{Instance, Surface, SurfaceConfiguration};

pub struct TerminalWindow {
    pub window: Arc<Window>,
    pub surface: Surface<'static>,
}

impl TerminalWindow {
    pub fn new(
        event_loop: &ActiveEventLoop,
        attributes: WindowAttributes,
        instance: &Instance,
    ) -> anyhow::Result<Self> {
        let window = Arc::new(event_loop.create_window(attributes)?);
        let surface = instance.create_surface(window.clone())?;
        
        Ok(Self { window, surface })
    }

    pub fn configure_surface(
        &self,
        device: &wgpu::Device,
        config: &SurfaceConfiguration,
    ) {
        self.surface.configure(device, config);
    }

    pub fn handle_resize(
        &self,
        device: &wgpu::Device,
        config: &mut SurfaceConfiguration,
        new_size: winit::dpi::PhysicalSize<u32>,
    ) {
        config.width = new_size.width;
        config.height = new_size.height;
        self.surface.configure(device, config);
        self.window.request_redraw();
    }
}
// src/terminal/mod.rs
pub mod app;
pub mod config;
pub mod gpu;
pub mod input;
pub mod render;
pub mod terminal;
pub mod texture;
pub mod window;

pub use gpu::GpuResources;
pub use terminal::Terminal;
pub use texture::GlyphAtlas;

use cosmic_text::{FontSystem, SwashCache};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub use cosmic_text::Buffer;

pub struct TerminalState {
    pub font_system: Arc<Mutex<FontSystem>>,
    pub buffer: Arc<Mutex<Buffer>>,
    pub text_content: Arc<Mutex<String>>,
    pub glyph_atlas: GlyphAtlas,
    pub swash_cache: SwashCache,
    pub gpu_resources: GpuResources,
    pub start_time: Instant,
    pub last_frame_time: Instant,
    pub focused: bool,
    pub dirty: bool,
    pub cursor_x: f32,
    pub cursor_y: f32,
    pub cursor_visible: bool,
    pub last_text: String,
}

pub fn run() -> Result<(), anyhow::Error> {
    app::TerminalApp::run()
}
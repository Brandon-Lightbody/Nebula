use wgpu::{Device, Queue, SurfaceConfiguration};
use crate::terminal::{
    TerminalState,
    window::TerminalWindow,
    texture::GlyphKey,
    config::{ATLAS_SIZE, FONT_SIZE, LINE_HEIGHT},
};
use std::time::Instant;
use wgpu::util::DeviceExt;
use bytemuck;
use cosmic_text::CacheKey;
use cosmic_text::CacheKeyFlags;

pub fn render_frame(
    device: &Device,
    queue: &Queue,
    config: &SurfaceConfiguration,
    window: &TerminalWindow,
    state: &mut TerminalState,
) {
    let now = Instant::now();
    let _delta = now.duration_since(state.last_frame_time).as_secs_f32();
    state.last_frame_time = now;

    // Get current surface texture
    let output = match window.surface.get_current_texture() {
        Ok(frame) => frame,
        Err(_) => {
            eprintln!("Surface texture error, reconfiguring surface");
            window.configure_surface(device, config);
            window.window.request_redraw();
            return;
        }
    };

    let view = output
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    
    // Declare cursor variables here so they're available for debugging
    let mut rendered_cursor = false;
    let mut cursor_x = 0.0;
    let mut cursor_y = 0.0;
    
    let (vertex_buffer, vertex_count) = if let Ok(mut buffer_lock) = state.buffer.lock() {
        if let Ok(mut fs) = state.font_system.lock() {
            // Shape the text buffer
            buffer_lock.shape_until_scroll(&mut fs, true);
            let mut verts: Vec<[f32; 4]> = Vec::new();

            let (screen_width, screen_height) =
                (config.width as f32, config.height as f32);
            
            let mut glyph_count = 0;
            let mut skipped_glyphs = 0;
            
            for run in buffer_lock.layout_runs() {
                for glyph in run.glyphs {
                    // Skip zero-width glyphs (like space, control characters)
                    if glyph.w == 0.0 {
                        skipped_glyphs += 1;
                        continue;
                    }

                    // Create glyph key
                    let key = GlyphKey {
                        font_id: glyph.font_id,
                        glyph_id: glyph.glyph_id,
                        font_size: glyph.font_size as u16,
                    };

                    // Create cache key for swash
                    let cache_key = CacheKey::new(
                        glyph.font_id,
                        glyph.glyph_id,
                        glyph.font_size,
                        (0.0, 0.0),
                        cosmic_text::CacheKeyFlags::empty(),
                    );

                    // Get the swash image
                    if let Some(image) = state.swash_cache.lock().unwrap().get_image(&mut fs, cache_key.0) {
                        // Skip zero-sized images
                        if image.placement.width == 0 || image.placement.height == 0 {
                            skipped_glyphs += 1;
                            continue;
                        }
                        
                        // Add to atlas or get existing
                        match state.glyph_atlas.add_glyph(queue, key, &image) {
                            Ok((x, y, w, h)) => {
                                glyph_count += 1;
                                
                                // Calculate texture coordinates
                                let atlas_x = x as f32 / ATLAS_SIZE as f32;
                                let atlas_y = y as f32 / ATLAS_SIZE as f32;
                                let atlas_w = w as f32 / ATLAS_SIZE as f32;
                                let atlas_h = h as f32 / ATLAS_SIZE as f32;

                                // Calculate screen position
                                let screen_x = glyph.x;
                                let screen_y = run.line_y + glyph.y - image.placement.top as f32;

                                // Convert to normalized device coordinates
                                let left = (screen_x / screen_width) * 2.0 - 1.0;
                                let right = ((screen_x + w as f32) / screen_width) * 2.0 - 1.0;
                                let top = 1.0 - (screen_y / screen_height) * 2.0;
                                let bottom = 1.0 - ((screen_y + h as f32) / screen_height) * 2.0;

                                // Create two triangles (6 vertices) for the glyph quad
                                verts.push([left, top, atlas_x, atlas_y]);
                                verts.push([right, top, atlas_x + atlas_w, atlas_y]);
                                verts.push([left, bottom, atlas_x, atlas_y + atlas_h]);
                                
                                verts.push([right, top, atlas_x + atlas_w, atlas_y]);
                                verts.push([right, bottom, atlas_x + atlas_w, atlas_y + atlas_h]);
                                verts.push([left, bottom, atlas_x, atlas_y + atlas_h]);
                            }
                            Err(e) => {
                                eprintln!("Glyph atlas error: {}", e);
                                skipped_glyphs += 1;
                            }
                        }
                    } else {
                        skipped_glyphs += 1;
                    }
                }
            }

            // Get cursor position from state
            cursor_x = *state.cursor_x.lock().unwrap();
            cursor_y = *state.cursor_y.lock().unwrap();
            let metrics = buffer_lock.metrics();

            // Render cursor if visible and blinking
            if state.cursor_visible && state.cursor_blink {
                rendered_cursor = true;
                let cursor_width = FONT_SIZE;
                let cursor_height = LINE_HEIGHT;
                
                // Convert to normalized device coordinates
                let left = (cursor_x / screen_width) * 2.0 - 1.0;
                let right = ((cursor_x + cursor_width) / screen_width) * 2.0 - 1.0;
                let top = 1.0 - (cursor_y / screen_height) * 2.0;
                let bottom = 1.0 - ((cursor_y + cursor_height) / screen_height) * 2.0;
                
                // Create two triangles (6 vertices) for the cursor quad
                // Using special texture coordinates (-1, -1) to indicate cursor
                verts.push([left, top, -1.0, -1.0]);
                verts.push([right, top, -1.0, -1.0]);
                verts.push([left, bottom, -1.0, -1.0]);
                verts.push([right, top, -1.0, -1.0]);
                verts.push([right, bottom, -1.0, -1.0]);
                verts.push([left, bottom, -1.0, -1.0]);
            }

            // Debug information
            if state.local_dirty {
                println!(
                    "Rendering frame: {} glyphs, {} skipped, {} vertices, cursor: {}x{} at ({}, {})",
                    glyph_count,
                    skipped_glyphs,
                    verts.len(),
                    FONT_SIZE,
                    LINE_HEIGHT,
                    cursor_x,
                    cursor_y
                );
            }

            // Create vertex buffer if we have vertices
            if !verts.is_empty() {
                let vertex_buf = device.create_buffer_init(
                    &wgpu::util::BufferInitDescriptor {
                        label: Some("Glyph Vertices"),
                        contents: bytemuck::cast_slice(&verts),
                        usage: wgpu::BufferUsages::VERTEX,
                    },
                );
                (Some(vertex_buf), verts.len() as u32)
            } else {
                (None, 0)
            }
        } else {
            eprintln!("Failed to lock font system");
            (None, 0)
        }
    } else {
        eprintln!("Failed to lock text buffer");
        (None, 0)
    };

    // Create command encoder
    let mut encoder = device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });
    
    // Begin render pass
    {
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });

        // Set pipeline and bindings
        rpass.set_pipeline(&state.gpu_resources.pipeline);
        rpass.set_bind_group(0, state.glyph_atlas.bind_group(), &[]);

        // Draw vertices if available
        if let Some(ref vertex_buffer) = vertex_buffer {
            rpass.set_vertex_buffer(0, vertex_buffer.slice(..));
            rpass.draw(0..vertex_count, 0..1);
        } else if state.local_dirty {
            eprintln!("No vertices to draw");
        }
    }

    // Submit commands and present
    queue.submit(Some(encoder.finish()));
    output.present();
    
    // Reset dirty flag
    state.local_dirty = false;
}
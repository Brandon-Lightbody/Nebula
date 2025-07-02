use wgpu::{Device, Queue, SurfaceConfiguration};
use crate::terminal::{
    TerminalState,
    window::TerminalWindow,
    texture::GlyphKey,
    config::ATLAS_SIZE,
};
use std::time::Instant;
use wgpu::util::DeviceExt;
use bytemuck;

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

    let output = match window.surface.get_current_texture() {
        Ok(frame) => frame,
        Err(_) => {
            window.configure_surface(device, config);
            window.window.request_redraw();
            return;
        }
    };

    let view = output
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    
    let (vertex_buffer, vertex_count) = if let Ok(mut buffer_lock) = state.buffer.lock() {
        if let Ok(mut fs) = state.font_system.lock() {
            buffer_lock.shape_until_scroll(&mut fs, true);
            let mut verts: Vec<[f32; 4]> = Vec::new();

            let (screen_width, screen_height) =
                (config.width as f32, config.height as f32);
            
            for run in buffer_lock.layout_runs() {
                for glyph in run.glyphs {
                    let key = GlyphKey {
                        font_id: glyph.font_id,
                        glyph_id: glyph.glyph_id as u16,
                        font_size: glyph.font_size as u16,
                    };

                    let cache_key = cosmic_text::CacheKey::new(
                        glyph.font_id,
                        glyph.glyph_id,
                        glyph.font_size,
                        (0.0, 0.0),
                        cosmic_text::CacheKeyFlags::empty(),
                    );

                    if let Some(image) = state.swash_cache.get_image(&mut fs, cache_key.0) {
                        match state.glyph_atlas.add_glyph(
                            queue,
                            key,
                            &image,
                        ) {
                            Ok((x, y, w, h)) => {
                                let atlas_x = x as f32 / ATLAS_SIZE as f32;
                                let atlas_y = y as f32 / ATLAS_SIZE as f32;
                                let atlas_w = w as f32 / ATLAS_SIZE as f32;
                                let atlas_h = h as f32 / ATLAS_SIZE as f32;

                                let screen_x = glyph.x;
                                let screen_y = run.line_y + glyph.y - image.placement.top as f32;

                                let left = (screen_x / screen_width) * 2.0 - 1.0;
                                let right = ((screen_x + w as f32) / screen_width) * 2.0 - 1.0;
                                let top = 1.0 - (screen_y / screen_height) * 2.0;
                                let bottom = 1.0 - ((screen_y + h as f32) / screen_height) * 2.0;

                                verts.push([left, top, atlas_x, atlas_y]);
                                verts.push([right, top, atlas_x + atlas_w, atlas_y]);
                                verts.push([left, bottom, atlas_x, atlas_y + atlas_h]);
                                
                                verts.push([right, top, atlas_x + atlas_w, atlas_y]);
                                verts.push([right, bottom, atlas_x + atlas_w, atlas_y + atlas_h]);
                                verts.push([left, bottom, atlas_x, atlas_y + atlas_h]);
                            }
                            Err(e) => eprintln!("Glyph atlas error: {}", e),
                        }
                    }
                }
            }

            // Get cursor position from state
            let cursor_x = *state.cursor_x.lock().unwrap();
            let cursor_y = *state.cursor_y.lock().unwrap();

            // Render cursor
            let elapsed = state.start_time.elapsed().as_secs_f32();
            let cursor_blink = (elapsed * 2.0).sin() > 0.0;
            if state.cursor_visible && cursor_blink {
                let cursor_width = 8.0;
                let cursor_height = 20.0;
                
                let left = (cursor_x / screen_width) * 2.0 - 1.0;
                let right = ((cursor_x + cursor_width) / screen_width) * 2.0 - 1.0;
                let top = 1.0 - (cursor_y / screen_height) * 2.0;
                let bottom = 1.0 - ((cursor_y + cursor_height) / screen_height) * 2.0;
                
                verts.push([left, top, -1.0, -1.0]);
                verts.push([right, top, -1.0, -1.0]);
                verts.push([left, bottom, -1.0, -1.0]);
                verts.push([right, top, -1.0, -1.0]);
                verts.push([right, bottom, -1.0, -1.0]);
                verts.push([left, bottom, -1.0, -1.0]);
            }

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
            (None, 0)
        }
    } else {
        (None, 0)
    };

    let mut encoder = device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });
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

        rpass.set_pipeline(&state.gpu_resources.pipeline);
        rpass.set_bind_group(0, state.glyph_atlas.bind_group(), &[]);

        if let Some(ref vertex_buffer) = vertex_buffer {
            rpass.set_vertex_buffer(0, vertex_buffer.slice(..));
            rpass.draw(0..vertex_count, 0..1);
        }
    }

    queue.submit(Some(encoder.finish()));
    output.present();
    
    state.local_dirty = false;
}
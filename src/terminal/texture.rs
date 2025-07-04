// src/terminal/texture.rs
use anyhow::{Result, anyhow};
use cosmic_text::SwashImage;
use std::collections::HashMap;
use wgpu::{
    BindGroup, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingResource, BindingType, Device, Extent3d, ImageCopyTexture, ImageDataLayout, Queue,
    Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
    TextureViewDescriptor, TextureViewDimension,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub font_id: cosmic_text::fontdb::ID,
    pub glyph_id: u16,
    pub font_size: u16,
}

pub struct GlyphAtlas {
    texture: Texture,
    view: TextureView,
    sampler: Sampler,
    bind_group: BindGroup,
    bind_group_layout: BindGroupLayout,
    cache: HashMap<GlyphKey, (u32, u32, u32, u32)>,
    current_x: u32,
    current_y: u32,
    row_height: u32,
    atlas_size: u32,
}

impl GlyphAtlas {
    pub fn new(device: &Device, atlas_size: u32) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("Glyph Atlas"),
            size: Extent3d {
                width: atlas_size,
                height: atlas_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&TextureViewDescriptor::default());
        
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("Glyph Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Glyph Atlas Bind Group Layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Glyph Atlas Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&sampler),
                },
            ],
        });

        GlyphAtlas {
            texture,
            view,
            sampler,
            bind_group,
            bind_group_layout,
            cache: HashMap::new(),
            current_x: 0,
            current_y: 0,
            row_height: 0,
            atlas_size,
        }
    }

    pub fn bind_group_layout(&self) -> &BindGroupLayout {
        &self.bind_group_layout
    }

    pub fn bind_group(&self) -> &BindGroup {
        &self.bind_group
    }

    pub fn add_glyph(
        &mut self,
        queue: &Queue,
        key: GlyphKey,
        image: &SwashImage,
    ) -> Result<(u32, u32, u32, u32)> {
        if let Some(rect) = self.cache.get(&key) {
            return Ok(*rect);
        }

        let width = image.placement.width;
        let height = image.placement.height;

        // Skip zero-sized glyphs
        if width == 0 || height == 0 {
            return Err(anyhow!("Zero-sized glyph"));
        }

        if self.current_x + width > self.atlas_size {
            self.current_x = 0;
            self.current_y += self.row_height;
            self.row_height = 0;
        }

        if self.current_y + height > self.atlas_size {
            return Err(anyhow!("Glyph atlas out of space"));
        }

        if height > self.row_height {
            self.row_height = height;
        }

        let mut rgba_data = Vec::with_capacity((width * height * 4) as usize);
        for &alpha in image.data.iter() {
            rgba_data.extend_from_slice(&[255, 255, 255, alpha]);
        }

        queue.write_texture(
            ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: self.current_x,
                    y: self.current_y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &rgba_data,
            ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let rect = (self.current_x, self.current_y, width, height);
        self.cache.insert(key, rect);
        self.current_x += width;

        Ok(rect)
    }
}
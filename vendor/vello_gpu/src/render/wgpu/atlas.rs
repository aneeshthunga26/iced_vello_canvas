// Copyright 2025 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Encoder-ordered bitmap uploads and atlas clears.

/// Copies padded RGBA bytes in command order, preserving earlier pending atlas users.
pub(crate) fn upload(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    bytes: &[u8],
    size: [u32; 2],
    offset: [u32; 2],
) {
    let [width, height] = size;
    if width == 0 || height == 0 {
        return;
    }
    let stride = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Ordered atlas upload"),
        size: u64::from(stride) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: true,
    });
    {
        let mut mapped = buffer.slice(..).get_mapped_range_mut();
        for y in 0..height as usize {
            let n = width as usize * 4;
            mapped[y * stride as usize..y * stride as usize + n]
                .copy_from_slice(&bytes[y * n..(y + 1) * n]);
        }
    }
    buffer.unmap();
    encoder.copy_buffer_to_texture(
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(height),
            },
        },
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: offset[0],
                y: offset[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

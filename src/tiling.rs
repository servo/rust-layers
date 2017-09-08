// Copyright 2014 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use geometry::{DevicePixel, LayerPixel};
use layers::{BufferRequest, ContentAge, LayerBuffer};
use platform::surface::NativeDisplay;
use texturegl::Texture;
use util::project_rect_to_screen;

use euclid::length::Length;
use euclid::{Matrix4D, Point2D, TypedPoint2D};
use euclid::rect::{Rect, TypedRect};
use euclid::size::{Size2D, TypedSize2D};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::mem;

pub struct Tile {
    /// The buffer displayed by this tile.
    buffer: Option<Box<LayerBuffer>>,

    /// The content age of any pending buffer request to avoid re-requesting
    /// a buffer while waiting for it to come back from rendering.
    content_age_of_pending_buffer: Option<ContentAge>,

    /// A handle to the GPU texture.
    pub texture: Texture,

    /// The tile boundaries in the parent layer coordinates.
    pub bounds: Option<TypedRect<f32, LayerPixel>>,
}

impl Tile {
    fn new() -> Tile {
        Tile {
            buffer: None,
            texture: Texture::zero(),
            content_age_of_pending_buffer: None,
            bounds: None,
        }
    }

    fn should_use_new_buffer(&self, new_buffer: &Box<LayerBuffer>) -> bool {
        match self.buffer {
            Some(ref buffer) => new_buffer.content_age >= buffer.content_age,
            None => true,
        }
    }

    fn replace_buffer(&mut self, buffer: Box<LayerBuffer>) -> Option<Box<LayerBuffer>> {
        if !self.should_use_new_buffer(&buffer) {
            warn!("Layer received an old buffer.");
            return Some(buffer);
        }

        let old_buffer = self.buffer.take();
        self.buffer = Some(buffer);
        self.texture = Texture::zero(); // The old texture is bound to the old buffer.
        self.content_age_of_pending_buffer = None;
        old_buffer
    }

    fn create_texture(&mut self, display: &NativeDisplay) {
        if let Some(ref buffer) = self.buffer {
            // If we already have a texture it should still be valid.
            if !self.texture.is_zero() {
                return;
            }

            // Make a new texture and bind the LayerBuffer's surface to it.
            self.texture = Texture::new_with_buffer(buffer);
            debug!(
                "Tile: binding to native surface {}",
                buffer.native_surface.get_id() as isize
            );
            buffer.native_surface.bind_to_texture(
                display,
                &self.texture,
            );

            // Set the layer's rect.
            self.bounds = Some(TypedRect::from_untyped(&buffer.rect));
        }
    }

    fn should_request_buffer(&self, content_age: ContentAge) -> bool {
        // Don't resend a request if our buffer's content age matches the current content age.
        if let Some(ref buffer) = self.buffer {
            if buffer.content_age >= content_age {
                return false;
            }
        }

        // Don't resend a request, if we already have one pending.
        match self.content_age_of_pending_buffer {
            Some(pending_content_age) => pending_content_age != content_age,
            None => true,
        }
    }
}

pub struct TileGrid {
    pub tiles: HashMap<Point2D<usize>, Tile>,

    /// The size of tiles in this grid in device pixels.
    tile_size: Length<usize, DevicePixel>,

    // Buffers that are currently unused.
    unused_buffers: Vec<Box<LayerBuffer>>,
}

pub fn rect_uint_as_rect_f32(rect: Rect<usize>) -> Rect<f32> {
    TypedRect::new(
        Point2D::new(rect.origin.x as f32, rect.origin.y as f32),
        Size2D::new(rect.size.width as f32, rect.size.height as f32),
    )
}

impl TileGrid {
    pub fn new(tile_size: usize) -> TileGrid {
        TileGrid {
            tiles: HashMap::new(),
            tile_size: Length::new(tile_size),
            unused_buffers: Vec::new(),
        }
    }

    pub fn get_rect_for_tile_index(
        &self,
        tile_index: Point2D<usize>,
        current_layer_size: TypedSize2D<f32, DevicePixel>,
    ) -> TypedRect<usize, DevicePixel> {

        let origin: TypedPoint2D<usize, DevicePixel> = TypedPoint2D::new(
            self.tile_size.get() * tile_index.x,
            self.tile_size.get() * tile_index.y,
        );

        // Don't let tiles extend beyond the layer boundaries.
        let tile_size = self.tile_size.get() as f32;
        let size = Size2D::new(
            tile_size.min(current_layer_size.width - origin.x as f32),
            tile_size.min(current_layer_size.height - origin.y as f32),
        );

        // Round up to texture pixels.
        let size = TypedSize2D::new(size.width.ceil() as usize, size.height.ceil() as usize);

        TypedRect::new(origin, size)
    }

    pub fn take_unused_buffers(&mut self) -> Vec<Box<LayerBuffer>> {
        let mut unused_buffers = Vec::new();
        mem::swap(&mut unused_buffers, &mut self.unused_buffers);
        unused_buffers
    }

    pub fn add_unused_buffer(&mut self, buffer: Option<Box<LayerBuffer>>) {
        if let Some(buffer) = buffer {
            self.unused_buffers.push(buffer);
        }
    }

    pub fn tile_intersects_rect(
        &self,
        tile_index: &Point2D<usize>,
        test_rect: &Rect<f32>,
        current_layer_size: TypedSize2D<f32, DevicePixel>,
        layer_world_origin: &Point2D<f32>,
        layer_transform: &Matrix4D<f32>,
    ) -> bool {
        let tile_rect = self.get_rect_for_tile_index(*tile_index, current_layer_size);
        let tile_rect = tile_rect.to_f32().to_untyped().translate(
            layer_world_origin,
        );

        let screen_rect = project_rect_to_screen(&tile_rect, layer_transform);

        if let Some(screen_rect) = screen_rect {
            if screen_rect.rect.intersection(&test_rect).is_some() {
                return true;
            }
        }

        false
    }

    pub fn mark_tiles_outside_of_rect_as_unused(
        &mut self,
        rect: TypedRect<f32, DevicePixel>,
        layer_world_origin: &Point2D<f32>,
        layer_transform: &Matrix4D<f32>,
        current_layer_size: TypedSize2D<f32, DevicePixel>,
    ) {
        let mut tile_indexes_to_take = Vec::new();

        for tile_index in self.tiles.keys() {
            if !self.tile_intersects_rect(
                tile_index,
                &rect.to_untyped(),
                current_layer_size,
                layer_world_origin,
                layer_transform,
            )
            {
                tile_indexes_to_take.push(tile_index.clone());
            }
        }

        for tile_index in &tile_indexes_to_take {
            if let Some(ref mut tile) = self.tiles.remove(tile_index) {
                self.add_unused_buffer(tile.buffer.take());
            }
        }
    }

    pub fn get_buffer_request_for_tile(
        &mut self,
        tile_index: Point2D<usize>,
        current_layer_size: TypedSize2D<f32, DevicePixel>,
        current_content_age: ContentAge,
    ) -> Option<BufferRequest> {
        let tile_rect = self.get_rect_for_tile_index(tile_index, current_layer_size);
        let tile = match self.tiles.entry(tile_index) {
            Entry::Occupied(occupied) => occupied.into_mut(),
            Entry::Vacant(vacant) => vacant.insert(Tile::new()),
        };

        if tile_rect.is_empty() {
            return None;
        }

        if !tile.should_request_buffer(current_content_age) {
            return None;
        }

        tile.content_age_of_pending_buffer = Some(current_content_age);

        Some(BufferRequest::new(
            tile_rect.to_untyped(),
            tile_rect.to_f32().to_untyped(),
            current_content_age,
        ))
    }

    /// Returns buffer requests inside the given dirty rect, and simultaneously throws out tiles
    /// outside the given viewport rect.
    pub fn get_buffer_requests_in_rect(
        &mut self,
        dirty_rect: TypedRect<f32, DevicePixel>,
        viewport: TypedRect<f32, DevicePixel>,
        current_layer_size: TypedSize2D<f32, DevicePixel>,
        layer_world_origin: &Point2D<f32>,
        layer_transform: &Matrix4D<f32>,
        current_content_age: ContentAge,
    ) -> Vec<BufferRequest> {
        let mut buffer_requests = Vec::new();

        // Get the range of tiles that can fit into the current layer size.
        // Step through each, transform/clip them to 2d rect
        // Check if visible against rect

        let tile_size = self.tile_size.get() as f32;
        let x_tile_count = ((current_layer_size.to_untyped().width + tile_size - 1.0) /
                                tile_size) as usize;
        let y_tile_count = ((current_layer_size.to_untyped().height + tile_size - 1.0) /
                                tile_size) as usize;

        for x in 0..x_tile_count {
            for y in 0..y_tile_count {
                let tile_index = Point2D::new(x, y);
                if self.tile_intersects_rect(
                    &tile_index,
                    &dirty_rect.to_untyped(),
                    current_layer_size,
                    layer_world_origin,
                    layer_transform,
                )
                {
                    if let Some(buffer) = self.get_buffer_request_for_tile(
                        tile_index,
                        current_layer_size,
                        current_content_age,
                    )
                    {
                        buffer_requests.push(buffer);
                    }
                }
            }
        }

        self.mark_tiles_outside_of_rect_as_unused(
            viewport,
            layer_world_origin,
            layer_transform,
            current_layer_size,
        );

        buffer_requests
    }

    pub fn get_tile_index_for_point(&self, point: Point2D<usize>) -> Point2D<usize> {
        assert!(point.x % self.tile_size.get() == 0);
        assert!(point.y % self.tile_size.get() == 0);
        Point2D::new(
            (point.x / self.tile_size.get()) as usize,
            (point.y / self.tile_size.get()) as usize,
        )
    }

    pub fn add_buffer(&mut self, buffer: Box<LayerBuffer>) {
        let index = self.get_tile_index_for_point(buffer.screen_pos.origin.clone());
        if !self.tiles.contains_key(&index) {
            warn!("Received buffer for non-existent tile!");
            self.add_unused_buffer(Some(buffer));
            return;
        }

        let replaced_buffer = self.tiles.get_mut(&index).unwrap().replace_buffer(buffer);
        self.add_unused_buffer(replaced_buffer);
    }

    pub fn do_for_all_tiles<F>(&self, mut f: F)
    where
        F: FnMut(&Tile),
    {
        for tile in self.tiles.values() {
            f(tile);
        }
    }

    pub fn collect_buffers(&mut self) -> Vec<Box<LayerBuffer>> {
        let mut collected_buffers = self.take_unused_buffers();
        collected_buffers.extend(self.tiles.drain().flat_map(
            |(_, mut tile)| tile.buffer.take(),
        ));
        collected_buffers
    }

    pub fn create_textures(&mut self, display: &NativeDisplay) {
        for (_, ref mut tile) in &mut self.tiles {
            tile.create_texture(display);
        }
    }

    /// Calculate the amount of memory used by all the tiles in the
    /// tile grid. The memory may be allocated on the heap or in GPU memory.
    pub fn get_memory_usage(&self) -> usize {
        self.tiles
            .values()
            .map(|ref tile| {
                // We cannot use Option::map_or here because rust will
                // complain about moving out of borrowed content.
                match tile.buffer {
                    Some(ref buffer) => buffer.get_mem(),
                    None => 0,
                }
            })
            .sum()
    }
}

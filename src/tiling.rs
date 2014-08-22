// Copyright 2014 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use geometry::DevicePixel;
use layers::{BufferRequest, ContentAge, LayerBuffer};
use platform::surface::{NativeCompositingGraphicsContext, NativeSurfaceMethods};
use texturegl::Texture;

use geom::matrix::{Matrix4, identity};
use geom::point::Point2D;
use geom::rect::Rect;
use geom::size::{Size2D, TypedSize2D};
use std::collections::hashmap::HashMap;
use std::iter::range_inclusive;
use std::mem;
use std::num::Zero;

pub struct Tile {
    /// The buffer displayed by this tile.
    buffer: Option<Box<LayerBuffer>>,

    /// The content age of any pending buffer request to avoid re-requesting
    /// a buffer while waiting for it to come back from rendering.
    content_age_of_pending_buffer: Option<ContentAge>,

    /// A handle to the GPU texture.
    pub texture: Texture,

    /// The transformation applied to this tiles texture.
    pub transform: Matrix4<f32>,
}

impl Tile {
    fn new() -> Tile {
        Tile {
            buffer: None,
            texture: Zero::zero(),
            transform: identity(),
            content_age_of_pending_buffer: None,
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
        self.texture = Zero::zero(); // The old texture is bound to the old buffer.
        self.content_age_of_pending_buffer = None;
        return old_buffer;
    }

    fn create_texture(&mut self, graphics_context: &NativeCompositingGraphicsContext) {
        match self.buffer {
            Some(ref buffer) => {
                let size = Size2D(buffer.screen_pos.size.width as int,
                                  buffer.screen_pos.size.height as int);

                // If we already have a texture it should still be valid.
                if !self.texture.is_zero() {
                    return;
                }

                // Make a new texture and bind the LayerBuffer's surface to it.
                self.texture = Texture::new_with_buffer(buffer);
                debug!("Tile: binding to native surface {:d}",
                       buffer.native_surface.get_id() as int);
                buffer.native_surface.bind_to_texture(graphics_context, &self.texture, size);

                // Set the layer's transform.
                let rect = buffer.rect;
                let transform = identity().translate(rect.origin.x, rect.origin.y, 0.0);
                self.transform = transform.scale(rect.size.width, rect.size.height, 1.0);
            },
            None => {},
        }
    }

    fn should_request_buffer(&self, content_age: ContentAge) -> bool {
        // Don't resend a request if our buffer's content age matches the current content age.
        match self.buffer {
            Some(ref buffer) => {
                if buffer.content_age >= content_age {
                    return false;
                }
            }
            None => {}
        }

        // Don't resend a request, if we already have one pending.
        match self.content_age_of_pending_buffer {
            Some(pending_content_age) => pending_content_age != content_age,
            None => true,
        }
    }
}

pub struct TileGrid {
    pub tiles: HashMap<Point2D<uint>, Tile>,

    // The size of tiles in this grid in device pixels.
    tile_size: uint,

    // Buffers that are currently unused.
    unused_buffers: Vec<Box<LayerBuffer>>,
}

pub fn rect_uint_as_rect_f32(rect: Rect<uint>) -> Rect<f32> {
    Rect(Point2D(rect.origin.x as f32, rect.origin.y as f32),
         Size2D(rect.size.width as f32, rect.size.height as f32))
}

impl TileGrid {
    pub fn new(tile_size: uint) -> TileGrid {
        TileGrid {
            tiles: HashMap::new(),
            tile_size: tile_size,
            unused_buffers: Vec::new(),
        }
    }

    pub fn get_tile_index_range_for_rect(&self, rect: Rect<f32>) -> (Point2D<uint>, Point2D<uint>) {
        (Point2D((rect.origin.x / self.tile_size as f32) as uint,
                 (rect.origin.y / self.tile_size as f32) as uint),
         Point2D(((rect.origin.x + rect.size.width) / self.tile_size as f32) as uint,
                 ((rect.origin.y + rect.size.height) / self.tile_size as f32) as uint))
    }

    pub fn get_rect_for_tile_index(&self, tile_index: Point2D<uint>) -> Rect<uint> {
        Rect(Point2D(self.tile_size * tile_index.x, self.tile_size * tile_index.y),
             Size2D(self.tile_size, self.tile_size))
    }

    pub fn take_unused_buffers(&mut self) -> Vec<Box<LayerBuffer>> {
        let mut unused_buffers = Vec::new();
        mem::swap(&mut unused_buffers, &mut self.unused_buffers);
        return unused_buffers;
    }

    pub fn add_unused_buffer(&mut self, buffer: Option<Box<LayerBuffer>>) {
        match buffer {
            Some(buffer) => self.unused_buffers.push(buffer),
            None => {},
        }
    }

    pub fn mark_tiles_outside_of_rect_as_unused(&mut self, rect: Rect<f32>) {
        let mut tile_indexes_to_take = Vec::new();
        for tile_index in self.tiles.keys() {
            if !rect_uint_as_rect_f32(self.get_rect_for_tile_index(*tile_index)).intersects(&rect) {
                tile_indexes_to_take.push(tile_index.clone());
            }
        }

        for tile_index in tile_indexes_to_take.iter() {
            match self.tiles.pop(tile_index) {
                Some(ref mut tile) => self.add_unused_buffer(tile.buffer.take()),
                None => {},
            }
        }
    }

    pub fn get_buffer_request_for_tile(&mut self,
                                       tile_index: Point2D<uint>,
                                       current_content_age: ContentAge)
                                       -> Option<BufferRequest> {
        let tile_rect = self.get_rect_for_tile_index(tile_index);
        let tile_screen_rect = rect_uint_as_rect_f32(tile_rect);

        let tile = self.tiles.find_or_insert_with(tile_index, |_| Tile::new());
        if !tile.should_request_buffer(current_content_age) {
            return None;
        }

        tile.content_age_of_pending_buffer = Some(current_content_age);
        return Some(BufferRequest::new(tile_rect, tile_screen_rect, current_content_age));
    }

    pub fn get_buffer_requests_in_rect(&mut self,
                                       screen_rect: Rect<f32>,
                                       current_content_age: ContentAge)
                                       -> Vec<BufferRequest> {
        let mut buffer_requests = Vec::new();
        let rect_in_layer_pixels = screen_rect;
        let (top_left_index, bottom_right_index) =
            self.get_tile_index_range_for_rect(rect_in_layer_pixels);

        for x in range_inclusive(top_left_index.x, bottom_right_index.x) {
            for y in range_inclusive(top_left_index.y, bottom_right_index.y) {
                match self.get_buffer_request_for_tile(Point2D(x, y), current_content_age) {
                    Some(buffer) => buffer_requests.push(buffer),
                    None => {},
                }
            }
        }

        self.mark_tiles_outside_of_rect_as_unused(rect_in_layer_pixels);
        return buffer_requests;
    }

    pub fn get_tile_index_for_point(&self, point: Point2D<uint>) -> Point2D<uint> {
        assert!(point.x % self.tile_size == 0);
        assert!(point.y % self.tile_size == 0);
        Point2D((point.x / self.tile_size) as uint,
                (point.y / self.tile_size) as uint)
    }

    pub fn add_buffer(&mut self, buffer: Box<LayerBuffer>) {
        let index = self.get_tile_index_for_point(buffer.screen_pos.origin.clone());
        if !self.tiles.contains_key(&index) {
            warn!("Received buffer for non-existent tile!");
            self.add_unused_buffer(Some(buffer));
            return;
        }

        let replaced_buffer = self.tiles.get_mut(&index).replace_buffer(buffer);
        self.add_unused_buffer(replaced_buffer);
    }

    pub fn do_for_all_tiles(&self, f: |&Tile|) {
        for tile in self.tiles.values() {
            f(tile);
        }
    }

    pub fn collect_buffers(&mut self) -> Vec<Box<LayerBuffer>> {
        let mut collected_buffers = Vec::new();

        collected_buffers.push_all_move(self.take_unused_buffers());

        // We need to replace the HashMap since it cannot be used again after move_iter().
        let mut tile_map = HashMap::new();
        mem::swap(&mut tile_map, &mut self.tiles);

        for (_, mut tile) in tile_map.move_iter() {
            match tile.buffer.take() {
                Some(buffer) => collected_buffers.push(buffer),
                None => {},
            }
        }

        return collected_buffers;
    }

    pub fn create_textures(&mut self, graphics_context: &NativeCompositingGraphicsContext) {
        for (_, ref mut tile) in self.tiles.mut_iter() {
            tile.create_texture(graphics_context);
        }
    }
}

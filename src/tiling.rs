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
use platform::surface::NativeCompositingGraphicsContext;
use texturegl::Texture;

use geom::length::Length;
use geom::matrix::Matrix4;
use geom::point::Point2D;
use geom::rect::{Rect, TypedRect};
use geom::size::{Size2D, TypedSize2D};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::iter::range_inclusive;
use std::mem;

pub struct Tile {
    /// The buffer displayed by this tile.
    buffer: Option<Box<LayerBuffer>>,

    /// The content age of any pending buffer request to avoid re-requesting
    /// a buffer while waiting for it to come back from rendering.
    content_age_of_pending_buffer: Option<ContentAge>,

    /// A handle to the GPU texture.
    pub texture: Texture,

    /// The transformation applied to this tiles texture.
    pub transform: Matrix4,

    /// The tile boundaries in the parent layer coordinates.
    pub bounds: Option<TypedRect<LayerPixel,f32>>,
}

impl Tile {
    fn new() -> Tile {
        Tile {
            buffer: None,
            texture: Texture::zero(),
            transform: Matrix4::identity(),
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
        return old_buffer;
    }

    fn create_texture(&mut self, graphics_context: &NativeCompositingGraphicsContext) {
        match self.buffer {
            Some(ref buffer) => {
                let size = Size2D::new(buffer.screen_pos.size.width as isize,
                                       buffer.screen_pos.size.height as isize);

                // If we already have a texture it should still be valid.
                if !self.texture.is_zero() {
                    return;
                }

                // Make a new texture and bind the LayerBuffer's surface to it.
                self.texture = Texture::new_with_buffer(buffer);
                debug!("Tile: binding to native surface {}",
                       buffer.native_surface.get_id() as isize);
                buffer.native_surface.bind_to_texture(graphics_context, &self.texture, size);

                // Set the layer's transform.
                let rect = buffer.rect;
                let transform = Matrix4::identity().translate(rect.origin.x, rect.origin.y, 0.0);
                self.transform = transform.scale(rect.size.width, rect.size.height, 1.0);
                self.bounds = Some(Rect::from_untyped(&rect));
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
    pub tiles: HashMap<Point2D<usize>, Tile>,

    /// The size of tiles in this grid in device pixels.
    tile_size: Length<DevicePixel, usize>,

    // Buffers that are currently unused.
    unused_buffers: Vec<Box<LayerBuffer>>,
}

pub fn rect_uint_as_rect_f32(rect: Rect<usize>) -> Rect<f32> {
    Rect::new(Point2D::new(rect.origin.x as f32, rect.origin.y as f32),
              Size2D::new(rect.size.width as f32, rect.size.height as f32))
}

impl TileGrid {
    pub fn new(tile_size: usize) -> TileGrid {
        TileGrid {
            tiles: HashMap::new(),
            tile_size: Length::new(tile_size),
            unused_buffers: Vec::new(),
        }
    }

    pub fn get_tile_index_range_for_rect(&self,
                                         rect: TypedRect<DevicePixel, f32>)
                                         -> (Point2D<usize>, Point2D<usize>) {
        let rect = rect.to_untyped();

        // NB: Even in the case of an empty rect, the semantics of Rust floating-point-to-integer
        // casts mean this will corrently round to zero.
        (Point2D::new((rect.origin.x / self.tile_size.get() as f32) as usize,
                      (rect.origin.y / self.tile_size.get() as f32) as usize),
         Point2D::new(((rect.origin.x + rect.size.width - 1.0) / self.tile_size.get() as f32) as usize,
                      ((rect.origin.y + rect.size.height - 1.0) / self.tile_size.get() as f32) as usize))
    }

    pub fn get_rect_for_tile_index(&self,
                                   tile_index: Point2D<usize>,
                                   current_layer_size: TypedSize2D<DevicePixel, f32>)
                                   -> TypedRect<DevicePixel, usize> {

        let origin = Point2D::new(self.tile_size.get() * tile_index.x,
                             self.tile_size.get() * tile_index.y);

        // Don't let tiles extend beyond the layer boundaries.
        let tile_size = self.tile_size.get() as f32;
        let size = Size2D::new(tile_size.min(current_layer_size.width.get() - origin.x as f32),
                          tile_size.min(current_layer_size.height.get() - origin.y as f32));

        // Round up to texture pixels.
        let size = Size2D::new(size.width.ceil() as usize, size.height.ceil() as usize);

        Rect::from_untyped(&Rect::new(origin, size))
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

    pub fn mark_tiles_outside_of_rect_as_unused(&mut self,
                                                rect: TypedRect<DevicePixel, f32>,
                                                current_layer_size: TypedSize2D<DevicePixel, f32>) {
        let mut tile_indexes_to_take = Vec::new();
        for tile_index in self.tiles.keys() {
            let tile_rect = self.get_rect_for_tile_index(*tile_index, current_layer_size);
            if !tile_rect.as_f32().intersects(&rect) {
                tile_indexes_to_take.push(tile_index.clone());
            }
        }

        for tile_index in tile_indexes_to_take.iter() {
            match self.tiles.remove(tile_index) {
                Some(ref mut tile) => self.add_unused_buffer(tile.buffer.take()),
                None => {},
            }
        }
    }

    pub fn get_buffer_request_for_tile(&mut self,
                                       tile_index: Point2D<usize>,
                                       current_layer_size: TypedSize2D<DevicePixel, f32>,
                                       current_content_age: ContentAge)
                                       -> Option<BufferRequest> {
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

        return Some(BufferRequest::new(tile_rect.to_untyped(),
                                       tile_rect.as_f32().to_untyped(),
                                       current_content_age));
    }

    /// Returns buffer requests inside the given dirty rect, and simultaneously throws out tiles
    /// outside the given viewport rect.
    pub fn get_buffer_requests_in_rect(&mut self,
                                       rect_in_layer: TypedRect<DevicePixel, f32>,
                                       viewport_in_layer: TypedRect<DevicePixel, f32>,
                                       current_layer_size: TypedSize2D<DevicePixel, f32>,
                                       current_content_age: ContentAge)
                                       -> Vec<BufferRequest> {
        let mut buffer_requests = Vec::new();
        let (top_left_index, bottom_right_index) =
            self.get_tile_index_range_for_rect(rect_in_layer);

        for x in range_inclusive(top_left_index.x, bottom_right_index.x) {
            for y in range_inclusive(top_left_index.y, bottom_right_index.y) {
                match self.get_buffer_request_for_tile(Point2D::new(x, y),
                                                       current_layer_size,
                                                       current_content_age) {
                    Some(buffer) => buffer_requests.push(buffer),
                    None => {},
                }
            }
        }

        self.mark_tiles_outside_of_rect_as_unused(viewport_in_layer, current_layer_size);
        return buffer_requests;
    }

    pub fn get_tile_index_for_point(&self, point: Point2D<usize>) -> Point2D<usize> {
        assert!(point.x % self.tile_size.get() == 0);
        assert!(point.y % self.tile_size.get() == 0);
        Point2D::new((point.x / self.tile_size.get()) as usize,
                     (point.y / self.tile_size.get()) as usize)
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

    pub fn do_for_all_tiles<F: Fn(&Tile)>(&self, f: F) {
        for tile in self.tiles.values() {
            f(tile);
        }
    }

    pub fn collect_buffers(&mut self) -> Vec<Box<LayerBuffer>> {
        let mut collected_buffers = Vec::new();

        collected_buffers.extend(self.take_unused_buffers().into_iter());

        // We need to replace the HashMap since it cannot be used again after move_iter().
        let mut tile_map = HashMap::new();
        mem::swap(&mut tile_map, &mut self.tiles);

        for (_, mut tile) in tile_map.into_iter() {
            match tile.buffer.take() {
                Some(buffer) => collected_buffers.push(buffer),
                None => {},
            }
        }

        return collected_buffers;
    }

    pub fn create_textures(&mut self, graphics_context: &NativeCompositingGraphicsContext) {
        for (_, ref mut tile) in self.tiles.iter_mut() {
            tile.create_texture(graphics_context);
        }
    }
}

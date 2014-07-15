// Copyright 2014 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use geom::point::Point2D;
use geom::size::Size2D;
use geom::rect::Rect;
use layers::BufferRequest;
use layers::LayerBuffer;
use std::collections::hashmap::HashMap;
use std::iter::range_inclusive;
use std::mem;

pub struct Tile {
    buffer: Option<Box<LayerBuffer>>,
}

impl Tile {
    fn new() -> Tile {
        Tile {
            buffer: None,
        }
    }

    fn replace_buffer(&mut self, buffer: Box<LayerBuffer>) -> Option<Box<LayerBuffer>> {
        let old_buffer = self.buffer.take();
        self.buffer = Some(buffer);
        return old_buffer;
    }
}

pub struct TileGrid {
    pub tiles: HashMap<Point2D<uint>, Tile>,

    // The size of tiles in this grid in device pixels.
    tile_size: uint,

    // Buffers that are currently unused.
    unused_buffers: Vec<Box<LayerBuffer>>,

    // Whether or not there are pending buffer requests.
    waiting_on_buffers : bool,

    // Once we know that we are waiting for buffers, track any later buffer requests.
    // FIXME: Replace with a per-tile state which better tracks epoch transitions.
    pending_buffer_request: Option<(Rect<f32>, f32)>,
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
            waiting_on_buffers: false,
            pending_buffer_request: None,
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

    pub fn get_buffer_requests_in_rect(&mut self, screen_rect: Rect<f32>, scale: f32) -> Vec<BufferRequest> {
        if self.waiting_on_buffers {
            self.pending_buffer_request = Some((screen_rect, scale));
            return Vec::new();
        }

        let mut buffer_requests = Vec::new();
        let rect_in_layer_pixels = screen_rect * scale;
        let (top_left_index, bottom_right_index) =
            self.get_tile_index_range_for_rect(rect_in_layer_pixels);

        for x in range_inclusive(top_left_index.x, bottom_right_index.x) {
            for y in range_inclusive(top_left_index.y, bottom_right_index.y) {
                let tile_rect = self.get_rect_for_tile_index(Point2D(x, y));
                let tile_screen_rect = rect_uint_as_rect_f32(tile_rect) / scale;
                buffer_requests.push(BufferRequest::new(tile_rect, tile_screen_rect));
            }
        }

        self.mark_tiles_outside_of_rect_as_unused(rect_in_layer_pixels);
        self.waiting_on_buffers = !buffer_requests.is_empty();
        return buffer_requests;
    }

    pub fn get_tile_index_for_point(&self, point: Point2D<uint>) -> Point2D<uint> {
        assert!(point.x % self.tile_size == 0);
        assert!(point.y % self.tile_size == 0);
        Point2D((point.x / self.tile_size) as uint,
                (point.y / self.tile_size) as uint)
    }

    pub fn add_buffer(&mut self, buffer: Box<LayerBuffer>) {
        self.waiting_on_buffers = false;
        let index = self.get_tile_index_for_point(buffer.screen_pos.origin.clone());
        let replaced_buffer =
            self.tiles.find_or_insert_with(index, |_| Tile::new()).replace_buffer(buffer);
        self.add_unused_buffer(replaced_buffer);
    }

    pub fn do_for_all_buffers(&self, f: |&Box<LayerBuffer>|) {
        for tile in self.tiles.values() {
            match tile.buffer {
                Some(ref buffer) => f(buffer),
                None => {},
            }
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

    pub fn flush_pending_buffer_requests(&mut self) -> (Vec<BufferRequest>, f32) {
        match self.pending_buffer_request.take() {
            Some((rect, scale)) => (self.get_buffer_requests_in_rect(rect, scale), scale),
            None => (Vec::new(), 0.0),
        }
    }

    pub fn contents_changed(&mut self) {
        self.pending_buffer_request = None;
        self.waiting_on_buffers = false;
    }
}

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

pub struct TileGrid {
    pub tiles: HashMap<Point2D<uint>, Box<LayerBuffer>>,

    // The size of tiles in this grid in device pixels.
    tile_size: uint,

    // Tiles that are currently unused or outside the last-known visible rectangle.
    unused_tiles: Vec<Box<LayerBuffer>>,

    // Whether or not there are pending buffer requests.
    waiting_on_tiles : bool,

    // Once we know that we are waiting for tiles, track any later buffer requests.
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
            unused_tiles: Vec::new(),
            waiting_on_tiles: false,
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

    pub fn take_unused_tiles(&mut self) -> Vec<Box<LayerBuffer>> {
        let mut unused_tiles = Vec::new();
        mem::swap(&mut unused_tiles, &mut self.unused_tiles);
        return unused_tiles;
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
                Some(tile) => self.unused_tiles.push(tile),
                None => {},
            }
        }
    }

    pub fn get_buffer_requests_in_rect(&mut self, screen_rect: Rect<f32>, scale: f32) -> Vec<BufferRequest> {
        if self.waiting_on_tiles {
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
        self.waiting_on_tiles = !buffer_requests.is_empty();
        return buffer_requests;
    }

    pub fn get_tile_index_for_point(&self, point: Point2D<uint>) -> Point2D<uint> {
        assert!(point.x % self.tile_size == 0);
        assert!(point.y % self.tile_size == 0);
        Point2D((point.x / self.tile_size) as uint,
                (point.y / self.tile_size) as uint)
    }

    pub fn add_tile(&mut self, tile: Box<LayerBuffer>) {
        self.waiting_on_tiles = false;
        let index = self.get_tile_index_for_point(tile.screen_pos.origin.clone());
        match self.tiles.swap(index, tile) {
            Some(tile) => self.unused_tiles.push(tile),
            None => {},
        }
    }

    pub fn do_for_all_tiles(&self, f: |&Box<LayerBuffer>|) {
        for tile in self.tiles.values() {
            f(tile);
        }
    }

    pub fn collect_tiles(&mut self) -> Vec<Box<LayerBuffer>> {
        let mut collected_tiles = Vec::new();

        collected_tiles.push_all_move(self.take_unused_tiles());

        // We need to replace the HashMap since it cannot be used again after move_iter().
        let mut tile_map = HashMap::new();
        mem::swap(&mut tile_map, &mut self.tiles);

        for (_, tile) in tile_map.move_iter() {
            collected_tiles.push(tile);
        }

        return collected_tiles;
    }

    pub fn flush_pending_buffer_requests(&mut self) -> (Vec<BufferRequest>, f32) {
        match self.pending_buffer_request.take() {
            Some((rect, scale)) => (self.get_buffer_requests_in_rect(rect, scale), scale),
            None => (Vec::new(), 0.0),
        }
    }

    pub fn contents_changed(&mut self) {
        self.pending_buffer_request = None;
        self.waiting_on_tiles = false;
    }
}

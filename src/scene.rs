// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use color::Color;
use geom::matrix::Matrix4;
use geom::point::Point2D;
use geom::rect::{Rect, TypedRect};
use geom::size::TypedSize2D;
use geometry::DevicePixel;
use layers::{BufferRequest, Layer, LayerBuffer};
use std::mem;
use std::num::Zero;
use std::rc::Rc;

pub struct Scene<T> {
    pub root: Option<Rc<Layer<T>>>,
    pub viewport: TypedRect<DevicePixel, f32>,
    pub background_color: Color,
    pub unused_buffers: Vec<Box<LayerBuffer>>,

    /// The scene scale, to allow for zooming and high-resolution painting.
    pub scale: f32,
}

impl<T> Scene<T> {
    pub fn new(viewport: TypedRect<DevicePixel, f32>) -> Scene<T> {
        Scene {
            root: None,
            viewport: viewport,
            background_color: Color {
                r: 0.38f32,
                g: 0.36f32,
                b: 0.36f32,
                a: 1.0f32
            },
            unused_buffers: Vec::new(),
            scale: 1.,
        }
    }

    pub fn get_buffer_requests_for_layer(&mut self,
                                         layer: Rc<Layer<T>>,
                                         layers_and_requests: &mut Vec<(Rc<Layer<T>>,
                                                                        Vec<BufferRequest>)>,
                                         rect_in_window: TypedRect<DevicePixel, f32>) {
        let content_offset = *layer.content_offset.borrow() * self.scale;
        let content_offset = Point2D::from_untyped(&content_offset);

        // The rectangle passed in is in the coordinate system of our parent, so we
        // need to intersect with our boundaries and convert it to our coordinate system.
        let layer_bounds = layer.bounds.borrow().clone();
        let layer_rect = Rect(Point2D(rect_in_window.origin.x - content_offset.x,
                                      rect_in_window.origin.y - content_offset.y),
                              rect_in_window.size);

        match layer_rect.intersection(&layer_bounds) {
            Some(mut intersected_rect) => {
                // Child layers act as if they are rendered at (0,0), so we
                // subtract the layer's (x,y) coords in its containing page
                // to make the child_rect appear in coordinates local to it.
                intersected_rect.origin = intersected_rect.origin.sub(&layer_bounds.origin);

                let requests = layer.get_buffer_requests(intersected_rect);
                if !requests.is_empty() {
                    layers_and_requests.push((layer.clone(), requests));
                }

                self.unused_buffers.push_all_move(layer.collect_unused_buffers());

                for kid in layer.children().iter() {
                    self.get_buffer_requests_for_layer(kid.clone(),
                                                       layers_and_requests,
                                                       rect_in_window);
                }
            }
            None => {},
        }
    }

    pub fn get_buffer_requests(&mut self,
                               requests: &mut Vec<(Rc<Layer<T>>, Vec<BufferRequest>)>,
                               window_rect: TypedRect<DevicePixel, f32>) {
        let root_layer = match self.root {
            Some(ref root_layer) => root_layer.clone(),
            None => return,
        };

        self.get_buffer_requests_for_layer(root_layer.clone(), requests, window_rect);
    }

    pub fn collect_unused_buffers(&mut self) -> Vec<Box<LayerBuffer>> {
        let mut unused_buffers = Vec::new();
        mem::swap(&mut unused_buffers, &mut self.unused_buffers);
        return unused_buffers;
    }

    pub fn mark_layer_contents_as_changed_recursively_for_layer(&self, layer: Rc<Layer<T>>) {
        layer.contents_changed();
        for kid in layer.children().iter() {
            self.mark_layer_contents_as_changed_recursively_for_layer(kid.clone());
        }
    }

    pub fn mark_layer_contents_as_changed_recursively(&self) {
        let root_layer = match self.root {
            Some(ref root_layer) => root_layer.clone(),
            None => return,
        };
        self.mark_layer_contents_as_changed_recursively_for_layer(root_layer);
    }

    pub fn set_root_layer_size(&self, new_size: TypedSize2D<DevicePixel, f32>) {
        match self.root {
            Some(ref root_layer) => *root_layer.bounds.borrow_mut() = Rect(Zero::zero(), new_size),
            None => {},
        }
    }
}


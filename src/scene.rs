// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use geom::rect::{Rect, TypedRect};
use geom::scale_factor::ScaleFactor;
use geom::size::TypedSize2D;
use geometry::{DevicePixel, LayerPixel};
use layers::{BufferRequest, Layer, LayerBuffer};
use std::mem;
use std::num::Zero;
use std::rc::Rc;

pub struct Scene<T> {
    pub root: Option<Rc<Layer<T>>>,
    pub viewport: TypedRect<DevicePixel, f32>,
    pub unused_buffers: Vec<Box<LayerBuffer>>,

    /// The scene scale, to allow for zooming and high-resolution painting.
    pub scale: ScaleFactor<LayerPixel, DevicePixel, f32>,
}

impl<T> Scene<T> {
    pub fn new(viewport: TypedRect<DevicePixel, f32>) -> Scene<T> {
        Scene {
            root: None,
            viewport: viewport,
            unused_buffers: Vec::new(),
            scale: ScaleFactor(1.0),
        }
    }

    pub fn get_buffer_requests_for_layer(&mut self,
                                         layer: Rc<Layer<T>>,
                                         layers_and_requests: &mut Vec<(Rc<Layer<T>>,
                                                                        Vec<BufferRequest>)>,
                                         dirty_rect: TypedRect<LayerPixel, f32>) {
        // We need to consider the intersection of the dirty rect with the final position
        // of the layer onscreen. The layer will be translated by the content rect, so we
        // simply do the reverse to the dirty rect.
        let layer_bounds = *layer.bounds.borrow();
        let content_offset = *layer.content_offset.borrow();
        let dirty_rect_adjusted_for_content_offset = dirty_rect.translate(&-content_offset);

        match dirty_rect_adjusted_for_content_offset.intersection(&layer_bounds) {
            Some(mut intersected_rect) => {
                // Layer::get_buffer_requests_for_layer expects a rectangle in coordinates relative
                // to this layer's origin, so move our intersected rect into the coordinate space
                // of this layer.
                intersected_rect = intersected_rect.translate(&-layer_bounds.origin);
                let requests = layer.get_buffer_requests(intersected_rect, self.scale);
                if !requests.is_empty() {
                    layers_and_requests.push((layer.clone(), requests));
                }

                self.unused_buffers.extend(layer.collect_unused_buffers().into_iter());

            }
            None => {},
        }

        // If this layer masks its children, we don't need to ask for tiles outside the
        // boundaries of this layer. We can simply re-use the intersection rectangle
        // from above, but we must undo the content_offset translation, since children
        // will re-apply it.
        let mut dirty_rect_in_children = dirty_rect;
        if *layer.masks_to_bounds.borrow() {
            // FIXME: Likely because of rust bug rust-lang/rust#16822, caching the intersected
            // rect and reusing it causes a crash in rustc. When that bug is resolved this code
            // should simply reuse a cached version of the intersection.
            dirty_rect_in_children =
                match dirty_rect_adjusted_for_content_offset.intersection(&layer_bounds) {
                    Some(intersected_rect) => intersected_rect.translate(&content_offset),
                    None => Rect::zero(),
                };
        }

        if dirty_rect_in_children.is_empty() {
            return;
        }

        dirty_rect_in_children = dirty_rect_in_children.translate(&-layer_bounds.origin);
        for kid in layer.children().iter() {
            self.get_buffer_requests_for_layer(kid.clone(),
                                               layers_and_requests,
                                               dirty_rect_in_children);
        }
    }

    pub fn get_buffer_requests(&mut self,
                               requests: &mut Vec<(Rc<Layer<T>>, Vec<BufferRequest>)>,
                               window_rect: TypedRect<DevicePixel, f32>) {
        let root_layer = match self.root {
            Some(ref root_layer) => root_layer.clone(),
            None => return,
        };

        let scale = self.scale; // Necessary to avoid a double borrow.
        self.get_buffer_requests_for_layer(root_layer.clone(),
                                           requests,
                                           window_rect / scale);
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
            Some(ref root_layer) => {
                *root_layer.bounds.borrow_mut() = Rect(Zero::zero(), new_size / self.scale);
            },
            None => {},
        }
    }
}


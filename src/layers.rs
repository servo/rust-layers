// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use color::Color;
use geometry::{DevicePixel, LayerPixel};
use tiling::{Tile, TileGrid};

use euclid::matrix::Matrix4;
use euclid::scale_factor::ScaleFactor;
use euclid::size::{Size2D, TypedSize2D};
use euclid::point::{Point2D, TypedPoint2D};
use euclid::rect::{Rect, TypedRect};
use platform::surface::{NativeDisplay, NativeSurface};
use std::cell::{RefCell, RefMut};
use std::rc::Rc;
use util::{project_rect_to_screen, ScreenRect};

#[derive(Clone, Copy, PartialEq, PartialOrd)]
#[cfg_attr(feature = "plugins", derive(HeapSizeOf))]
pub struct ContentAge {
    age: usize,
}

impl ContentAge {
    pub fn new() -> ContentAge {
        ContentAge {
            age: 0,
        }
    }

    pub fn next(&mut self) {
        self.age += 1;
    }
}

#[cfg_attr(feature = "plugins", derive(HeapSizeOf))]
pub struct TransformState {
    /// Final, concatenated transform + perspective matrix for this layer
    pub final_transform: Matrix4,

    /// If this is none, the rect was clipped and is not visible at all!
    pub screen_rect: Option<ScreenRect>,

    /// Rectangle in global coordinates, but not transformed.
    pub world_rect: Rect<f32>,

    /// True if this layer has a non-identity transform
    pub has_transform: bool,
}

impl TransformState {
    fn new() -> TransformState {
        TransformState {
            final_transform: Matrix4::identity(),
            screen_rect: None,
            world_rect: Rect::zero(),
            has_transform: false,
        }
    }
}

pub struct Layer<T> {
    pub children: RefCell<Vec<Rc<Layer<T>>>>,
    pub transform: RefCell<Matrix4>,
    pub perspective: RefCell<Matrix4>,
    pub tile_size: usize,
    pub extra_data: RefCell<T>,
    tile_grid: RefCell<TileGrid>,

    /// The boundaries of this layer in the coordinate system of the parent layer.
    pub bounds: RefCell<TypedRect<LayerPixel, f32>>,

    /// A monotonically increasing counter that keeps track of the current content age.
    pub content_age: RefCell<ContentAge>,

    /// The content offset for this layer in unscaled layer pixels.
    pub content_offset: RefCell<TypedPoint2D<LayerPixel, f32>>,

    /// Whether this layer clips its children to its boundaries.
    pub masks_to_bounds: RefCell<bool>,

    /// The background color for this layer.
    pub background_color: RefCell<Color>,

    /// The opacity of this layer, from 0.0 (fully transparent) to 1.0 (fully opaque).
    pub opacity: RefCell<f32>,

    /// Whether this stacking context creates a new 3d rendering context.
    pub establishes_3d_context: bool,

    /// Collection of state related to transforms for this layer.
    pub transform_state: RefCell<TransformState>,
}

impl<T> Layer<T> {
    pub fn new(bounds: TypedRect<LayerPixel, f32>,
               tile_size: usize,
               background_color: Color,
               opacity: f32,
               establishes_3d_context: bool,
               data: T)
               -> Layer<T> {
        Layer {
            children: RefCell::new(vec!()),
            transform: RefCell::new(Matrix4::identity()),
            perspective: RefCell::new(Matrix4::identity()),
            bounds: RefCell::new(bounds),
            tile_size: tile_size,
            extra_data: RefCell::new(data),
            tile_grid: RefCell::new(TileGrid::new(tile_size)),
            content_age: RefCell::new(ContentAge::new()),
            masks_to_bounds: RefCell::new(false),
            content_offset: RefCell::new(Point2D::zero()),
            background_color: RefCell::new(background_color),
            opacity: RefCell::new(opacity),
            establishes_3d_context: establishes_3d_context,
            transform_state: RefCell::new(TransformState::new()),
        }
    }

    pub fn children(&self) -> RefMut<Vec<Rc<Layer<T>>>> {
        self.children.borrow_mut()
    }

    pub fn add_child(&self, new_child: Rc<Layer<T>>) {
        self.children().push(new_child);
    }

    pub fn remove_child_at_index(&self, index: usize) {
        self.children().remove(index);
    }

    /// Returns buffer requests inside the given dirty rect, and simultaneously throws out tiles
    /// outside the given viewport rect.
    pub fn get_buffer_requests(&self,
                               rect_in_layer: TypedRect<LayerPixel, f32>,
                               viewport_in_layer: TypedRect<LayerPixel, f32>,
                               scale: ScaleFactor<LayerPixel, DevicePixel, f32>)
                               -> Vec<BufferRequest> {
        let mut tile_grid = self.tile_grid.borrow_mut();
        tile_grid.get_buffer_requests_in_rect(rect_in_layer * scale,
                                              viewport_in_layer * scale,
                                              self.bounds.borrow().size * scale,
                                              &(self.transform_state.borrow().world_rect.origin *
                                                scale.get()),
                                              &self.transform_state.borrow().final_transform,
                                              *self.content_age.borrow())
    }

    pub fn resize(&self, new_size: TypedSize2D<LayerPixel, f32>) {
        self.bounds.borrow_mut().size = new_size;
    }

    pub fn add_buffer(&self, tile: Box<LayerBuffer>) {
        self.tile_grid.borrow_mut().add_buffer(tile);
    }

    pub fn collect_unused_buffers(&self) -> Vec<Box<LayerBuffer>> {
        self.tile_grid.borrow_mut().take_unused_buffers()
    }

    pub fn collect_buffers(&self) -> Vec<Box<LayerBuffer>> {
        self.tile_grid.borrow_mut().collect_buffers()
    }

    pub fn contents_changed(&self) {
        self.content_age.borrow_mut().next();
    }

    pub fn create_textures(&self, display: &NativeDisplay) {
        self.tile_grid.borrow_mut().create_textures(display);
    }

    pub fn do_for_all_tiles<F: FnMut(&Tile)>(&self, f: F) {
        self.tile_grid.borrow().do_for_all_tiles(f);
    }

    pub fn update_transform_state(&self,
                                  parent_transform: &Matrix4,
                                  parent_perspective: &Matrix4,
                                  parent_origin: &Point2D<f32>) {
        let mut ts = self.transform_state.borrow_mut();
        let rect_without_scroll = self.bounds.borrow()
                                             .to_untyped()
                                             .translate(parent_origin);

        ts.world_rect = rect_without_scroll.translate(&self.content_offset.borrow().to_untyped());

        let x0 = ts.world_rect.origin.x;
        let y0 = ts.world_rect.origin.y;

        // Build world space transform
        let local_transform = Matrix4::identity().translate(x0, y0, 0.0)
                                                 .mul(&*self.transform.borrow())
                                                 .translate(-x0, -y0, 0.0);

        ts.final_transform = parent_perspective.mul(&local_transform).mul(&parent_transform);
        ts.screen_rect = project_rect_to_screen(&ts.world_rect, &ts.final_transform);

        // TODO(gw): This is quite bogus. It's a hack to allow the paint task
        // to avoid "optimizing" 3d layers with an incorrect clip rect.
        // We should probably make the display list optimizer work with transforms!
        // This layer is part of a 3d context if its concatenated transform
        // is not identity, since 2d transforms don't get layers.
        ts.has_transform = ts.final_transform != Matrix4::identity();

        // Build world space perspective transform
        let perspective_transform = Matrix4::identity().translate(x0, y0, 0.0)
                                                       .mul(&*self.perspective.borrow())
                                                       .translate(-x0, -y0, 0.0);

        for child in self.children().iter() {
            child.update_transform_state(&ts.final_transform,
                                         &perspective_transform,
                                         &rect_without_scroll.origin);
        }
    }

    /// Calculate the amount of memory used by this layer and all its children.
    /// The memory may be allocated on the heap or in GPU memory.
    pub fn get_memory_usage(&self) -> usize {
        let size_of_children : usize = self.children().iter().map(|ref child| -> usize {
            child.get_memory_usage()
        }).sum();
        size_of_children + self.tile_grid.borrow().get_memory_usage()
    }
}

/// A request from the compositor to the renderer for tiles that need to be (re)displayed.
pub struct BufferRequest {
    /// The rect in pixels that will be drawn to the screen
    pub screen_rect: Rect<usize>,

    /// The rect in page coordinates that this tile represents
    pub page_rect: Rect<f32>,

    /// The content age of that this BufferRequest corresponds to.
    pub content_age: ContentAge,

    /// A cached NativeSurface that can be used to avoid allocating a new one.
    pub native_surface: Option<NativeSurface>,
}

impl BufferRequest {
    pub fn new(screen_rect: Rect<usize>, page_rect: Rect<f32>, content_age: ContentAge)
               -> BufferRequest {
        BufferRequest {
            screen_rect: screen_rect,
            page_rect: page_rect,
            content_age: content_age,
            native_surface: None,
        }
    }
}

pub struct LayerBuffer {
    /// The native surface which can be shared between threads or processes. On Mac this is an
    /// `IOSurface`; on Linux this is an X Pixmap; on Android this is an `EGLImageKHR`.
    pub native_surface: NativeSurface,

    /// The rect in the containing RenderLayer that this represents.
    pub rect: Rect<f32>,

    /// The rect in pixels that will be drawn to the screen.
    pub screen_pos: Rect<usize>,

    /// The scale at which this tile is rendered
    pub resolution: f32,

    /// Whether or not this buffer was painted with the CPU rasterization.
    pub painted_with_cpu: bool,

    /// The content age of that this buffer request corresponds to.
    pub content_age: ContentAge,
}

impl LayerBuffer {
    /// Returns the amount of memory used by the tile
    pub fn get_mem(&self) -> usize {
        self.native_surface.get_memory_usage()
    }

    /// Returns true if the tile is displayable at the given scale
    pub fn is_valid(&self, scale: f32) -> bool {
        (self.resolution - scale).abs() < 1.0e-6
    }

    /// Returns the Size2D of the tile
    pub fn get_size_2d(&self) -> Size2D<usize> {
        self.screen_pos.size
    }

    /// Marks the layer buffer as not leaking. See comments on
    /// `NativeSurfaceMethods::mark_wont_leak` for how this is used.
    pub fn mark_wont_leak(&mut self) {
        self.native_surface.mark_wont_leak()
    }

    /// Destroys the layer buffer. Painting task only.
    pub fn destroy(self, display: &NativeDisplay) {
        let mut this = self;
        this.native_surface.destroy(display)
    }
}

/// A set of layer buffers. This is an atomic unit used to switch between the front and back
/// buffers.
pub struct LayerBufferSet {
    pub buffers: Vec<Box<LayerBuffer>>
}

impl LayerBufferSet {
    /// Notes all buffer surfaces will leak if not destroyed via a call to `destroy`.
    pub fn mark_will_leak(&mut self) {
        for buffer in &mut self.buffers {
            buffer.native_surface.mark_will_leak()
        }
    }
}

// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use geometry::DevicePixel;
use tiling::{Tile, TileGrid};

use geom::matrix::{Matrix4, identity};
use geom::size::{Size2D, TypedSize2D};
use geom::point::Point2D;
use geom::rect::{Rect, TypedRect};
use platform::surface::{NativeSurfaceMethods, NativeSurface};
use platform::surface::{NativeCompositingGraphicsContext, NativePaintingGraphicsContext};
use std::cell::{RefCell, RefMut};
use std::rc::Rc;

#[deriving(Clone, PartialEq, PartialOrd)]
pub struct ContentAge {
    age: uint,
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

pub struct Layer<T> {
    pub children: RefCell<Vec<Rc<Layer<T>>>>,
    pub transform: RefCell<Matrix4<f32>>,
    pub tile_size: uint,
    pub extra_data: RefCell<T>,
    tile_grid: RefCell<TileGrid>,

    /// The boundaries of this layer in the coordinate system of the parent layer.
    pub bounds: RefCell<TypedRect<DevicePixel, f32>>,

    /// A monotonically increasing counter that keeps track of the current content age.
    pub content_age: RefCell<ContentAge>,

    /// The content offset for this layer in device pixels.
    pub content_offset: RefCell<Point2D<f32>>,
}

impl<T> Layer<T> {
    pub fn new(bounds: TypedRect<DevicePixel, f32>, tile_size: uint, data: T) -> Layer<T> {
        Layer {
            children: RefCell::new(vec!()),
            transform: RefCell::new(identity()),
            bounds: RefCell::new(bounds),
            tile_size: tile_size,
            extra_data: RefCell::new(data),
            tile_grid: RefCell::new(TileGrid::new(tile_size)),
            content_age: RefCell::new(ContentAge::new()),
            content_offset: RefCell::new(Point2D(0f32, 0f32)),
        }
    }

    pub fn children<'a>(&'a self) -> RefMut<'a,Vec<Rc<Layer<T>>>> {
        self.children.borrow_mut()
    }

    pub fn add_child(&self, new_child: Rc<Layer<T>>) {
        self.children().push(new_child);
    }

    pub fn get_buffer_requests(&self, rect_in_layer: Rect<f32>) -> Vec<BufferRequest> {
        let mut tile_grid = self.tile_grid.borrow_mut();
        return tile_grid.get_buffer_requests_in_rect(rect_in_layer, *self.content_age.borrow());
    }

    pub fn resize(&self, new_size: TypedSize2D<DevicePixel, f32>) {
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

    pub fn create_textures(&self, graphics_context: &NativeCompositingGraphicsContext) {
        self.tile_grid.borrow_mut().create_textures(graphics_context);
    }

    pub fn do_for_all_tiles(&self, f: |&Tile|) {
        self.tile_grid.borrow().do_for_all_tiles(f);
    }
}

/// A request from the compositor to the renderer for tiles that need to be (re)displayed.
#[deriving(Clone)]
pub struct BufferRequest {
    // The rect in pixels that will be drawn to the screen
    pub screen_rect: Rect<uint>,

    // The rect in page coordinates that this tile represents
    pub page_rect: Rect<f32>,

    /// The content age of that this BufferRequest corresponds to.
    pub content_age: ContentAge,
}

impl BufferRequest {
    pub fn new(screen_rect: Rect<uint>,
               page_rect: Rect<f32>,
               content_age: ContentAge)
               -> BufferRequest {
        BufferRequest {
            screen_rect: screen_rect,
            page_rect: page_rect,
            content_age: content_age,
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
    pub screen_pos: Rect<uint>,

    /// The scale at which this tile is rendered
    pub resolution: f32,

    /// NB: stride is in pixels, like OpenGL GL_UNPACK_ROW_LENGTH.
    pub stride: uint,

    /// Whether or not this buffer was painted with the CPU rasterization.
    pub painted_with_cpu: bool,

    /// The content age of that this buffer request corresponds to.
    pub content_age: ContentAge,
}

impl LayerBuffer {
    /// Returns the amount of memory used by the tile
    pub fn get_mem(&self) -> uint {
        // This works for now, but in the future we may want a better heuristic
        self.screen_pos.size.width * self.screen_pos.size.height
    }

    /// Returns true if the tile is displayable at the given scale
    pub fn is_valid(&self, scale: f32) -> bool {
        (self.resolution - scale).abs() < 1.0e-6
    }

    /// Returns the Size2D of the tile
    pub fn get_size_2d(&self) -> Size2D<uint> {
        self.screen_pos.size
    }

    /// Marks the layer buffer as not leaking. See comments on
    /// `NativeSurfaceMethods::mark_wont_leak` for how this is used.
    pub fn mark_wont_leak(&mut self) {
        self.native_surface.mark_wont_leak()
    }

    /// Destroys the layer buffer. Painting task only.
    pub fn destroy(self, graphics_context: &NativePaintingGraphicsContext) {
        let mut this = self;
        this.native_surface.destroy(graphics_context)
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
        for buffer in self.buffers.mut_iter() {
            buffer.native_surface.mark_will_leak()
        }
    }
}

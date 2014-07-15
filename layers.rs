// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use texturegl::Texture;
use tiling::TileGrid;

use geom::matrix::{Matrix4, identity};
use geom::size::Size2D;
use geom::rect::Rect;
use platform::surface::{NativePaintingGraphicsContext, NativeSurfaceMethods, NativeSurface};
use std::cell::{RefCell, RefMut};
use std::rc::Rc;

pub enum Format {
    ARGB32Format,
    RGB24Format
}

pub struct Layer<T> {
    pub children: RefCell<Vec<Rc<Layer<T>>>>,
    pub tiles: RefCell<Vec<Rc<TextureLayer>>>,
    pub transform: RefCell<Matrix4<f32>>,
    pub bounds: RefCell<Rect<f32>>,
    tile_size: uint,
    pub extra_data: RefCell<T>,
    tile_grid: RefCell<TileGrid>,
}

impl<T> Layer<T> {
    pub fn new(bounds: Rect<f32>, tile_size: uint, data: T) -> Layer<T> {
        Layer {
            children: RefCell::new(vec!()),
            tiles: RefCell::new(vec!()),
            transform: RefCell::new(identity()),
            bounds: RefCell::new(bounds),
            tile_size: tile_size,
            extra_data: RefCell::new(data),
            tile_grid: RefCell::new(TileGrid::new(tile_size)),
        }
    }

    pub fn children<'a>(&'a self) -> RefMut<'a,Vec<Rc<Layer<T>>>> {
        self.children.borrow_mut()
    }

    pub fn add_child(this: Rc<Layer<T>>, new_child: Rc<Layer<T>>) {
        this.children().push(new_child);
    }

    pub fn tile_size(this: Rc<Layer<T>>) -> uint {
        this.tile_size
    }

    pub fn get_tile_rects_page(this: Rc<Layer<T>>, window: Rect<f32>, scale: f32) -> (Vec<BufferRequest>, Vec<Box<LayerBuffer>>) {
        let mut tile_grid = this.tile_grid.borrow_mut();
        (tile_grid.get_buffer_requests_in_rect(window, scale), tile_grid.take_unused_tiles())
    }

    pub fn resize(this: Rc<Layer<T>>, new_size: Size2D<f32>) {
        this.bounds.borrow_mut().size = new_size;
    }

    pub fn do_for_all_tiles(this: Rc<Layer<T>>, f: |&Box<LayerBuffer>|) {
        this.tile_grid.borrow().do_for_all_tiles(f);
    }

    pub fn add_tile_pixel(this: Rc<Layer<T>>, tile: Box<LayerBuffer>) {
        this.tile_grid.borrow_mut().add_tile(tile);
    }

    pub fn collect_unused_tiles(this: Rc<Layer<T>>) -> Vec<Box<LayerBuffer>> {
        this.tile_grid.borrow_mut().take_unused_tiles()
    }

    pub fn collect_tiles(this: Rc<Layer<T>>) -> Vec<Box<LayerBuffer>> {
        this.tile_grid.borrow_mut().collect_tiles()
    }

    pub fn flush_pending_buffer_requests(&self) -> (Vec<BufferRequest>, f32) {
        self.tile_grid.borrow_mut().flush_pending_buffer_requests()
    }

    pub fn contents_changed(&self) {
        self.tile_grid.borrow_mut().contents_changed()
    }
}

/// Whether a texture should be flipped.
#[deriving(PartialEq)]
pub enum Flip {
    /// The texture should not be flipped.
    NoFlip,
    /// The texture should be flipped vertically.
    VerticalFlip,
}

pub struct TextureLayer {
    /// A handle to the GPU texture.
    pub texture: Texture,
    /// The size of the texture in pixels.
    size: Size2D<uint>,
    /// Whether this texture is flipped vertically.
    pub flip: Flip,

    pub transform: Matrix4<f32>,
}

impl TextureLayer {
    pub fn new(texture: Texture, size: Size2D<uint>, flip: Flip, transform: Matrix4<f32>) -> TextureLayer {
        TextureLayer {
            texture: texture,
            size: size,
            flip: flip,
            transform: transform,
        }
    }
}

/// A request from the compositor to the renderer for tiles that need to be (re)displayed.
#[deriving(Clone)]
pub struct BufferRequest {
    // The rect in pixels that will be drawn to the screen
    pub screen_rect: Rect<uint>,

    // The rect in page coordinates that this tile represents
    pub page_rect: Rect<f32>,
}

impl BufferRequest {
    pub fn new(screen_rect: Rect<uint>, page_rect: Rect<f32>) -> BufferRequest {
        BufferRequest {
            screen_rect: screen_rect,
            page_rect: page_rect,
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

/// The interface used by the BufferMap to get info about layer buffers.
pub trait Tile {
    /// Returns the amount of memory used by the tile
    fn get_mem(&self) -> uint;

    /// Returns true if the tile is displayable at the given scale
    fn is_valid(&self, f32) -> bool;

    /// Returns the Size2D of the tile
    fn get_size_2d(&self) -> Size2D<uint>;

    /// Marks the layer buffer as not leaking. See comments on
    /// `NativeSurfaceMethods::mark_wont_leak` for how this is used.
    fn mark_wont_leak(&mut self);

    /// Destroys the layer buffer. Painting task only.
    fn destroy(self, graphics_context: &NativePaintingGraphicsContext);
}

impl Tile for Box<LayerBuffer> {
    fn get_mem(&self) -> uint {
        // This works for now, but in the future we may want a better heuristic
        self.screen_pos.size.width * self.screen_pos.size.height
    }
    fn is_valid(&self, scale: f32) -> bool {
        (self.resolution - scale).abs() < 1.0e-6
    }
    fn get_size_2d(&self) -> Size2D<uint> {
        self.screen_pos.size
    }
    fn mark_wont_leak(&mut self) {
        self.native_surface.mark_wont_leak()
    }
    fn destroy(self, graphics_context: &NativePaintingGraphicsContext) {
        let mut this = self;
        this.native_surface.destroy(graphics_context)
    }
}



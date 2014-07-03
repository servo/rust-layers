// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use texturegl::Texture;

use geom::matrix::{Matrix4, identity};
use geom::size::Size2D;
use geom::rect::Rect;
use geom::point::Point2D;
use platform::surface::{NativeSurfaceMethods, NativeSurface};
use std::cell::{RefCell, RefMut};
use std::num::Zero;
use std::rc::Rc;
use quadtree::{Quadtree, NodeStatus, Normal};

/// The amount of memory usage allowed per layer.
static MAX_TILE_MEMORY_PER_LAYER: uint = 10000000;

pub enum Format {
    ARGB32Format,
    RGB24Format
}

pub struct Layer<T> {
    pub children: RefCell<Vec<Rc<Layer<T>>>>,
    pub tiles: RefCell<Vec<Rc<TextureLayer>>>,
    pub quadtree: RefCell<Quadtree>,
    pub transform: RefCell<Matrix4<f32>>,
    pub origin: RefCell<Point2D<f32>>,
    tile_size: uint,
    pub extra_data: RefCell<T>,
}

impl<T> Layer<T> {
    pub fn new(page_size: Size2D<f32>, tile_size: uint, data: T) -> Layer<T> {
        Layer {
            children: RefCell::new(vec!()),
            tiles: RefCell::new(vec!()),
            quadtree: RefCell::new(Quadtree::new(Size2D(page_size.width as uint, page_size.height as uint),
                                                 tile_size, Some(MAX_TILE_MEMORY_PER_LAYER))),
            transform: RefCell::new(identity()),
            origin: RefCell::new(Zero::zero()),
            tile_size: tile_size,
            extra_data: RefCell::new(data),
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
        this.quadtree.borrow_mut().get_tile_rects_page(window, scale)
    }

    pub fn set_status_page(this: Rc<Layer<T>>, rect: Rect<f32>, status: NodeStatus, include_border: bool) {
        this.quadtree.borrow_mut().set_status_page(rect, Normal, false); // Rect is unhidden
    }

    pub fn resize(this: Rc<Layer<T>>, new_size: Size2D<f32>) -> Vec<Box<LayerBuffer>> {
        this.quadtree.borrow_mut().resize(new_size.width as uint, new_size.height as uint)
    }

    pub fn do_for_all_tiles(this: Rc<Layer<T>>, f: |&Box<LayerBuffer>|) {
        this.quadtree.borrow_mut().do_for_all_tiles(f);
    }

    pub fn add_tile_pixel(this: Rc<Layer<T>>, tile: Box<LayerBuffer>) -> Vec<Box<LayerBuffer>> {
        this.quadtree.borrow_mut().add_tile_pixel(tile.screen_pos.origin.x,
                                                          tile.screen_pos.origin.y,
                                                          tile.resolution, tile)
    }

    pub fn collect_tiles(this: Rc<Layer<T>>) -> Vec<Box<LayerBuffer>> {
        this.quadtree.borrow_mut().collect_tiles()
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

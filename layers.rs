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

pub struct CommonLayer<T> {
    pub parent: Option<Rc<ContainerLayer<T>>>,
    pub prev_sibling: Option<Rc<ContainerLayer<T>>>,
    pub next_sibling: Option<Rc<ContainerLayer<T>>>,

    pub transform: Matrix4<f32>,
    pub origin: Point2D<f32>,
}

impl<T> CommonLayer<T> {
    // FIXME: Workaround for cross-crate bug regarding mutability of class fields
    pub fn set_transform(&mut self, new_transform: Matrix4<f32>) {
        self.transform = new_transform;
    }

    pub fn new() -> CommonLayer<T> {
        CommonLayer {
            parent: None,
            prev_sibling: None,
            next_sibling: None,
            transform: identity(),
            origin: Zero::zero(),
        }
    }
}

pub struct ContainerLayer<T> {
    pub common: RefCell<CommonLayer<T>>,
    pub first_child: RefCell<Option<Rc<ContainerLayer<T>>>>,
    pub last_child: RefCell<Option<Rc<ContainerLayer<T>>>>,
    pub tiles: RefCell<Vec<Rc<TextureLayer>>>,
    pub quadtree: RefCell<Quadtree>,

    tile_size: uint,
    pub extra_data: RefCell<T>,
}

pub struct ChildIterator<T> {
    current: Option<Rc<ContainerLayer<T>>>,
}

impl<T> Iterator<Rc<ContainerLayer<T>>> for ChildIterator<T> {
    fn next(&mut self) -> Option<Rc<ContainerLayer<T>>> {
        let (new_current, result) =
            match self.current {
                None => (None, None),
                Some(ref child) => {
                    (child.common().next_sibling.clone(), Some(child.clone()))
                }
            };
        self.current = new_current;
        result
    }
}

impl<T> ContainerLayer<T> {
    pub fn new(page_size: Option<Size2D<f32>>, tile_size: uint, data: T) -> ContainerLayer<T> {
        ContainerLayer {
            common: RefCell::new(CommonLayer::new()),
            first_child: RefCell::new(None),
            last_child: RefCell::new(None),
            quadtree: match page_size {
                None => {
                    RefCell::new(Quadtree::new(Size2D(tile_size, tile_size),
                                                   tile_size,
                                                   Some(MAX_TILE_MEMORY_PER_LAYER)))
                }
                Some(page_size) => {
                    RefCell::new(Quadtree::new(Size2D(page_size.width as uint, page_size.height as uint),
                                                   tile_size,
                                                   Some(MAX_TILE_MEMORY_PER_LAYER)))
                }
            },
            tiles: RefCell::new(vec!()),
            tile_size: tile_size,
            extra_data: RefCell::new(data),
        }
    }

    pub fn children(&self) -> ChildIterator<T> {
        ChildIterator {
            current: self.first_child.borrow().clone(),
        }
    }

    pub fn common<'a>(&'a self) -> RefMut<'a,CommonLayer<T>> {
        self.common.borrow_mut()
    }

    /// Adds a child to the beginning of the list.
    /// Only works when the child is disconnected from the layer tree.
    pub fn add_child_start(this: Rc<ContainerLayer<T>>, new_child: Rc<ContainerLayer<T>>) {
        let mut new_child_common = new_child.common();
        assert!(new_child_common.parent.is_none());
        assert!(new_child_common.prev_sibling.is_none());
        assert!(new_child_common.next_sibling.is_none());

        new_child_common.parent = Some(this.clone());

        match *this.first_child.borrow() {
            None => {}
            Some(ref first_child) => {
                let mut first_child_common = first_child.common();
                assert!(first_child_common.prev_sibling.is_none());
                first_child_common.prev_sibling = Some(new_child.clone());
                new_child_common.next_sibling = Some(first_child.clone());
            }
        }

        *this.first_child.borrow_mut() = Some(new_child.clone());

        let should_set = this.last_child.borrow().is_none();
        if should_set {
            *this.last_child.borrow_mut() = Some(new_child.clone());
        }
    }

    /// Adds a child to the end of the list.
    /// Only works when the child is disconnected from the layer tree.
    pub fn add_child_end(this: Rc<ContainerLayer<T>>, new_child: Rc<ContainerLayer<T>>) {
        let mut new_child_common = new_child.common();
        assert!(new_child_common.parent.is_none());
        assert!(new_child_common.prev_sibling.is_none());
        assert!(new_child_common.next_sibling.is_none());

        new_child_common.parent = Some(this.clone());


        match *this.last_child.borrow() {
            None => {}
            Some(ref last_child) => {
                let mut last_child_common = last_child.common();
                assert!(last_child_common.next_sibling.is_none());
                last_child_common.next_sibling = Some(new_child.clone());
                new_child_common.prev_sibling = Some(last_child.clone());
            }
        }

        *this.last_child.borrow_mut() = Some(new_child.clone());

        let mut child = this.first_child.borrow_mut();
        match *child {
            Some(_) => {},
            None => *child = Some(new_child.clone()),
        }
    }

    pub fn remove_child(this: Rc<ContainerLayer<T>>, child: Rc<ContainerLayer<T>>) {
        let mut child_common = child.common();
        assert!(child_common.parent.is_some());
        match child_common.parent {
            Some(ref container) => {
                assert!(container.deref() as *ContainerLayer<T> ==
                        this.deref() as *ContainerLayer<T>);
            },
            _ => fail!("Invalid parent of child in layer tree"),
        }

        let previous_sibling = child_common.prev_sibling.clone();
        match child_common.next_sibling {
            None => { // this is the last child
                *this.last_child.borrow_mut() = previous_sibling;
            },
            Some(ref sibling) => {
                sibling.common().prev_sibling = previous_sibling;
            }
        }

        let next_sibling = child_common.next_sibling.clone();
        match child_common.prev_sibling {
            None => { // this is the first child
                *this.first_child.borrow_mut() = next_sibling;
            },
            Some(ref sibling) => {
                sibling.common().next_sibling = next_sibling;
            }
        }
    }

    pub fn remove_all_children(&self) {
        *self.first_child.borrow_mut() = None;
        *self.last_child.borrow_mut() = None;
    }

    pub fn tile_size(this: Rc<ContainerLayer<T>>) -> uint {
        this.tile_size
    }

    pub fn get_tile_rects_page(this: Rc<ContainerLayer<T>>, window: Rect<f32>, scale: f32) -> (Vec<BufferRequest>, Vec<Box<LayerBuffer>>) {
        this.quadtree.borrow_mut().get_tile_rects_page(window, scale)
    }

    pub fn set_status_page(this: Rc<ContainerLayer<T>>, rect: Rect<f32>, status: NodeStatus, include_border: bool) {
        this.quadtree.borrow_mut().set_status_page(rect, Normal, false); // Rect is unhidden
    }

    pub fn resize(this: Rc<ContainerLayer<T>>, new_size: Size2D<f32>) -> Vec<Box<LayerBuffer>> {
        this.quadtree.borrow_mut().resize(new_size.width as uint, new_size.height as uint)
    }

    pub fn do_for_all_tiles(this: Rc<ContainerLayer<T>>, f: |&Box<LayerBuffer>|) {
        this.quadtree.borrow_mut().do_for_all_tiles(f);
    }

    pub fn add_tile_pixel(this: Rc<ContainerLayer<T>>, tile: Box<LayerBuffer>) -> Vec<Box<LayerBuffer>> {
        this.quadtree.borrow_mut().add_tile_pixel(tile.screen_pos.origin.x,
                                                          tile.screen_pos.origin.y,
                                                          tile.resolution, tile)
    }

    pub fn collect_tiles(this: Rc<ContainerLayer<T>>) -> Vec<Box<LayerBuffer>> {
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

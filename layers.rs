// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use geom::matrix::{Matrix4, identity};
use geom::size::Size2D;
use geom::rect::Rect;
use opengles::gl2::{GLuint, delete_textures};
use std::managed::mut_ptr_eq;

pub enum Format {
    ARGB32Format,
    RGB24Format
}

pub enum Layer {
    ContainerLayerKind(@mut ContainerLayer),
    TextureLayerKind(@mut TextureLayer),
    ImageLayerKind(@mut ImageLayer),
    TiledImageLayerKind(@mut TiledImageLayer)
}

impl Layer {
    pub fn with_common<T>(&self, f: &fn(&mut CommonLayer) -> T) -> T {
        match *self {
            ContainerLayerKind(container_layer) => f(&mut container_layer.common),
            TextureLayerKind(texture_layer) => f(&mut texture_layer.common),
            ImageLayerKind(image_layer) => f(&mut image_layer.common),
            TiledImageLayerKind(tiled_image_layer) => f(&mut tiled_image_layer.common)
        }
    }
}

pub struct CommonLayer {
    parent: Option<Layer>,
    prev_sibling: Option<Layer>,
    next_sibling: Option<Layer>,

    transform: Matrix4<f32>,
}

impl CommonLayer {
    // FIXME: Workaround for cross-crate bug regarding mutability of class fields
    pub fn set_transform(&mut self, new_transform: Matrix4<f32>) {
        self.transform = new_transform;
    }
}

pub fn CommonLayer() -> CommonLayer {
    CommonLayer {
        parent: None,
        prev_sibling: None,
        next_sibling: None,
        transform: identity(),
    }
}


pub struct ContainerLayer {
    common: CommonLayer,
    first_child: Option<Layer>,
    last_child: Option<Layer>,
    scissor: Option<Rect<f32>>,
}


pub fn ContainerLayer() -> ContainerLayer {
    ContainerLayer {
        common: CommonLayer(),
        first_child: None,
        last_child: None,
        scissor: None,
    }
}

struct ChildIterator {
    priv current: Option<Layer>,
}

impl Iterator<Layer> for ChildIterator {
    fn next(&mut self) -> Option<Layer> {
        match self.current {
            None => None,
            Some(child) => {
                self.current = child.with_common(|x| x.next_sibling);
                Some(child)
            }
        }
    }
}

impl ContainerLayer {
    pub fn children(&self) -> ChildIterator {
        ChildIterator { current: self.first_child }
    }

    /// Adds a child to the beginning of the list.
    /// Only works when the child is disconnected from the layer tree.
    pub fn add_child_start(@mut self, new_child: Layer) {
        do new_child.with_common |new_child_common| {
            assert!(new_child_common.parent.is_none());
            assert!(new_child_common.prev_sibling.is_none());
            assert!(new_child_common.next_sibling.is_none());

            new_child_common.parent = Some(ContainerLayerKind(self));

            match self.first_child {
                None => {}
                Some(first_child) => {
                    do first_child.with_common |first_child_common| {
                        assert!(first_child_common.prev_sibling.is_none());
                        first_child_common.prev_sibling = Some(new_child);
                        new_child_common.next_sibling = Some(first_child);
                    }
                }
            }

            self.first_child = Some(new_child);

            match self.last_child {
                None => self.last_child = Some(new_child),
                Some(_) => {}
            }
        }
    }

    /// Adds a child to the end of the list.
    /// Only works when the child is disconnected from the layer tree.
    pub fn add_child_end(@mut self, new_child: Layer) {
        do new_child.with_common |new_child_common| {
            assert!(new_child_common.parent.is_none());
            assert!(new_child_common.prev_sibling.is_none());
            assert!(new_child_common.next_sibling.is_none());

            new_child_common.parent = Some(ContainerLayerKind(self));

            match self.last_child {
                None => {}
                Some(last_child) => {
                    do last_child.with_common |last_child_common| {
                        assert!(last_child_common.next_sibling.is_none());
                        last_child_common.next_sibling = Some(new_child);
                        new_child_common.prev_sibling = Some(last_child);
                    }
                }
            }

            self.last_child = Some(new_child);

            match self.first_child {
                None => self.first_child = Some(new_child),
                Some(_) => {}
            }
        }
    }
    
    pub fn remove_child(@mut self, child: Layer) {
        do child.with_common |child_common| {
            assert!(child_common.parent.is_some());
            match child_common.parent.unwrap() {
                ContainerLayerKind(ref container) => {
                    assert!(mut_ptr_eq(*container, self));
                },
                _ => fail!(~"Invalid parent of child in layer tree"),
            }

            match child_common.next_sibling {
                None => { // this is the last child
                    self.last_child = child_common.prev_sibling;
                },
                Some(ref sibling) => {
                    do sibling.with_common |sibling_common| {
                        sibling_common.prev_sibling = child_common.prev_sibling;
                    }
                }
            }
            match child_common.prev_sibling {
                None => { // this is the first child
                    self.first_child = child_common.next_sibling;
                },
                Some(ref sibling) => {
                    do sibling.with_common |sibling_common| {
                        sibling_common.next_sibling = child_common.next_sibling;
                    }
                }
            }           
        }
    }
}

pub trait TextureManager {
    fn get_texture(&self) -> GLuint;
}

pub struct TextureLayer {
    common: CommonLayer,
    manager: @TextureManager,
    size: Size2D<uint>
}

impl TextureLayer {
    pub fn new(manager: @TextureManager, size: Size2D<uint>) -> TextureLayer {
        TextureLayer {
            common: CommonLayer(),
            manager: manager,
            size: size,
        }
    }
}

pub type WithDataFn<'self> = &'self fn(&'self [u8]);

pub trait ImageData {
    fn size(&self) -> Size2D<uint>;

    // NB: stride is in pixels, like OpenGL GL_UNPACK_ROW_LENGTH.
    fn stride(&self) -> uint;

    fn format(&self) -> Format;
    fn with_data(&self, WithDataFn);
}

pub struct Image {
    data: @ImageData,
    texture: Option<GLuint>,
}

#[unsafe_destructor]
impl Drop for Image {
    fn drop(&self) {
        match self.texture.clone() {
            None => {
                // Nothing to do.
            }
            Some(texture) => {
                delete_textures(&[texture]);
            }
        }
    }
}

impl Image {
    pub fn new(data: @ImageData) -> Image {
        Image { data: data, texture: None }
    }
}

/// Basic image data is a simple image data store that just owns the pixel data in memory.
pub struct BasicImageData {
    size: Size2D<uint>,
    stride: uint,
    format: Format,
    data: ~[u8]
}

impl BasicImageData {
    pub fn new(size: Size2D<uint>, stride: uint, format: Format, data: ~[u8]) ->
            BasicImageData {
        BasicImageData {
            size: size,
            stride: stride,
            format: format,
            data: data
        }
    }
}

impl ImageData for BasicImageData {
    fn size(&self) -> Size2D<uint> { self.size }
    fn stride(&self) -> uint { self.stride }
    fn format(&self) -> Format { self.format }
    fn with_data(&self, f: WithDataFn) { f(self.data) }
}

pub struct ImageLayer {
    common: CommonLayer,
    image: @mut Image,
}

impl ImageLayer {
    // FIXME: Workaround for cross-crate bug
    pub fn set_image(&mut self, new_image: @mut Image) {
        self.image = new_image;
    }
}

pub fn ImageLayer(image: @mut Image) -> ImageLayer {
    ImageLayer {
        common : CommonLayer(),
        image : image,
    }
}

pub struct TiledImageLayer {
    common: CommonLayer,
    tiles: @mut ~[@mut Image],
    tiles_across: uint,
}

pub fn TiledImageLayer(in_tiles: &[@mut Image], tiles_across: uint) -> TiledImageLayer {
    let tiles = @mut ~[];
    for tile in in_tiles.iter() {
        tiles.push(*tile);
    }

    TiledImageLayer {
        common: CommonLayer(),
        tiles: tiles,
        tiles_across: tiles_across
    }
}


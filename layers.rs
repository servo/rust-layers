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
use opengles::gl2::{GLuint, delete_textures};
use core::managed::mut_ptr_eq;

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
    fn with_common<T>(&self, f: &fn(&mut CommonLayer) -> T) -> T {
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

pub impl CommonLayer {
    // FIXME: Workaround for cross-crate bug regarding mutability of class fields
    fn set_transform(&mut self, new_transform: Matrix4<f32>) {
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
}


pub fn ContainerLayer() -> ContainerLayer {
    ContainerLayer {
        common: CommonLayer(),
        first_child: None,
        last_child: None,
    }
}

pub impl ContainerLayer {
    fn each_child(&self, f: &fn(Layer) -> bool) -> bool {
        let mut child_opt = self.first_child;
        while !child_opt.is_none() {
            let child = child_opt.get();
            if !f(child) {
                break
            }
            child_opt = child.with_common(|x| x.next_sibling);
        }
        true
    }

    /// Only works when the child is disconnected from the layer tree.
    fn add_child(@mut self, new_child: Layer) {
        do new_child.with_common |new_child_common| {
            assert!(new_child_common.parent.is_none());
            assert!(new_child_common.prev_sibling.is_none());
            assert!(new_child_common.next_sibling.is_none());

            new_child_common.parent = Some(ContainerLayerKind(self));

            match self.first_child {
                None => {}
                Some(copy first_child) => {
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
    
    fn remove_child(@mut self, child: Layer) {
        do child.with_common |child_common| {
            assert!(child_common.parent.is_some());
            match child_common.parent.get() {
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

pub struct TextureLayer {
    common: CommonLayer,
    texture: Texture,
    size: Size2D<uint>,
}

impl TextureLayer {
    pub fn new(texture: Texture, size: Size2D<uint>) -> TextureLayer {
        TextureLayer {
            common: CommonLayer(),
            texture: texture,
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
    texture: Option<Texture>,
}

pub impl Image {
    fn new(data: @ImageData) -> Image {
        Image {
            data: data,
            texture: None,
        }
    }
}

/// Basic image data is a simple image data store that just owns the pixel data in memory.
pub struct BasicImageData {
    size: Size2D<uint>,
    stride: uint,
    format: Format,
    data: ~[u8]
}

pub impl BasicImageData {
    fn new(size: Size2D<uint>, stride: uint, format: Format, data: ~[u8]) ->
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

pub impl ImageLayer {
    // FIXME: Workaround for cross-crate bug
    fn set_image(&mut self, new_image: @mut Image) {
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
    for in_tiles.each |tile| {
        tiles.push(*tile);
    }

    TiledImageLayer {
        common: CommonLayer(),
        tiles: tiles,
        tiles_across: tiles_across
    }
}


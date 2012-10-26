use geom::matrix::{Matrix4, identity};
use geom::size::Size2D;
use opengles::gl2::{GLuint, delete_textures};

use std::cmp::FuzzyEq;
use dvec::DVec;

pub enum Format {
    ARGB32Format,
    RGB24Format
}

pub enum Layer {
    ContainerLayerKind(@ContainerLayer),
    ImageLayerKind(@ImageLayer),
    TiledImageLayerKind(@TiledImageLayer)
}

impl Layer {
    pure fn with_common<T>(&self, f: &fn(&mut CommonLayer) -> T) -> T {
        match *self {
            ContainerLayerKind(container_layer) => f(&mut container_layer.common),
            ImageLayerKind(image_layer) => f(&mut image_layer.common),
            TiledImageLayerKind(tiled_image_layer) => f(&mut tiled_image_layer.common)
        }
    }
}

pub struct CommonLayer {
    mut parent: Option<Layer>,
    mut prev_sibling: Option<Layer>,
    mut next_sibling: Option<Layer>,

    mut transform: Matrix4<f32>,
}

impl CommonLayer {
    // FIXME: Workaround for cross-crate bug regarding mutability of class fields
    fn set_transform(new_transform: Matrix4<f32>) {
        self.transform = new_transform;
    }
}

pub fn CommonLayer() -> CommonLayer {
    CommonLayer {
        parent: None,
        prev_sibling: None,
        next_sibling: None,
        transform: identity(0.0f32),
    }
}


pub struct ContainerLayer {
    mut common: CommonLayer,
    mut first_child: Option<Layer>,
    mut last_child: Option<Layer>,
}


pub fn ContainerLayer() -> ContainerLayer {
    ContainerLayer {
        common: CommonLayer(),
        first_child: None,
        last_child: None,
    }
}

impl ContainerLayer {
    fn each_child(&const self, f: &fn(Layer) -> bool) {
        let mut child_opt = self.first_child;
        while !child_opt.is_none() {
            let child = child_opt.get();
            if !f(child) { break; }
            child_opt = child.with_common(|x| x.next_sibling);
        }
    }

    /// Only works when the child is disconnected from the layer tree.
    fn add_child(&const self, new_child: Layer) {
        do new_child.with_common |new_child_common| {
            assert new_child_common.parent.is_none();
            assert new_child_common.prev_sibling.is_none();
            assert new_child_common.next_sibling.is_none();

            match self.first_child {
                None => self.first_child = Some(new_child),
                Some(copy first_child) => {
                    do first_child.with_common |first_child_common| {
                        assert first_child_common.prev_sibling.is_none();
                        first_child_common.next_sibling = Some(new_child);
                        new_child_common.prev_sibling = Some(first_child);
                    }
                }
            }

            match self.last_child {
                None => self.last_child = Some(new_child),
                Some(_) => {}
            }
        }
    }
}

pub type WithDataFn = &fn(&[u8]);

pub trait ImageData {
    fn size() -> Size2D<uint>;

    // NB: stride is in pixels, like OpenGL GL_UNPACK_ROW_LENGTH.
    fn stride() -> uint;

    fn format() -> Format;
    fn with_data(WithDataFn);
}

pub struct Image {
    data: @ImageData,
    mut texture: Option<GLuint>,

    drop {
        match copy self.texture {
            None => {
                // Nothing to do.
            }
            Some(texture) => {
                delete_textures(&[texture]);
            }
        }
    }
}

pub impl Image {
    static fn new(data: @ImageData) -> Image {
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

pub impl BasicImageData {
    static fn new(size: Size2D<uint>, stride: uint, format: Format, data: ~[u8]) ->
            BasicImageData {
        BasicImageData {
            size: size,
            stride: stride,
            format: format,
            data: move data
        }
    }
}

pub impl BasicImageData : ImageData {
    fn size() -> Size2D<uint> { self.size }
    fn stride() -> uint { self.stride }
    fn format() -> Format { self.format }
    fn with_data(f: WithDataFn) { f(self.data) }
}

pub struct ImageLayer {
    mut common: CommonLayer,
    mut image: @layers::Image,
}

impl ImageLayer {
    // FIXME: Workaround for cross-crate bug
    fn set_image(new_image: @layers::Image) {
        self.image = new_image;
    }
}

pub fn ImageLayer(image: @layers::Image) -> ImageLayer {
    ImageLayer {
        common : CommonLayer(),
        image : image,
    }
}

pub struct TiledImageLayer {
    mut common: CommonLayer,
    tiles: DVec<@layers::Image>,
    mut tiles_across: uint,
}

pub fn TiledImageLayer(in_tiles: &[@layers::Image], tiles_across: uint) -> TiledImageLayer {
    let tiles = DVec();
    for in_tiles.each |tile| {
        tiles.push(*tile);
    }

    TiledImageLayer {
        common: CommonLayer(),
        tiles: tiles,
        tiles_across: tiles_across
    }
}


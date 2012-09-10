import geom::matrix::{Matrix4, identity};
import opengles::gl2::{GLuint, delete_textures};

import std::cmp::FuzzyEq;
import dvec::DVec;

enum Format {
    ARGB32Format,
    RGB24Format
}

enum Layer {
    ContainerLayerKind(@ContainerLayer),
    ImageLayerKind(@ImageLayer),
    TiledImageLayerKind(@TiledImageLayer)
}

struct CommonLayer {
    mut parent: Option<Layer>,
    mut prev_sibling: Option<Layer>,
    mut next_sibling: Option<Layer>,

    mut transform: Matrix4<f32>,

    // FIXME: Workaround for cross-crate bug regarding mutability of class fields
    fn set_transform(new_transform: Matrix4<f32>) {
        self.transform = new_transform;
    }
}


fn CommonLayer() -> CommonLayer {
    CommonLayer {
        parent : None,
        prev_sibling : None,
        next_sibling : None,
        transform : identity(0.0f32),
    }
}


struct ContainerLayer {
    mut common: CommonLayer,
    mut first_child: Option<Layer>,
    mut last_child: Option<Layer>,
}


fn ContainerLayer() -> ContainerLayer {
    ContainerLayer {
        common : CommonLayer(),
        first_child : None,
        last_child : None,
    }
}


struct Image {
    width: uint,
    height: uint,
    format: Format,
    data: ~[u8],
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


fn Image(width: uint, height: uint, format: Format, +data: ~[u8]) -> Image {
    Image {
        width : width,
        height : height,
        format : format,
        data : data,
        texture : None,
    }
}

struct ImageLayer {
    mut common: CommonLayer,
    mut image: @layers::Image,

    // FIXME: Workaround for cross-crate bug
    fn set_image(new_image: @layers::Image) {
        self.image = new_image;
    }
}


fn ImageLayer(image: @layers::Image) -> ImageLayer {
    ImageLayer {
        common : CommonLayer(),
        image : image,
    }
}

struct TiledImageLayer {
    mut common: CommonLayer,
    tiles: DVec<@layers::Image>,
    mut tiles_across: uint,
}

fn TiledImageLayer(in_tiles: &[@layers::Image], tiles_across: uint) -> TiledImageLayer {
    let tiles = DVec();
    for in_tiles.each |tile| {
        tiles.push(tile);
    }

    TiledImageLayer {
        common: CommonLayer(),
        tiles: tiles,
        tiles_across: tiles_across
    }
}


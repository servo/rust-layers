import geom::matrix::{Matrix4, identity};
import opengles::gl2::GLuint;

import std::cmp::fuzzy_eq;

enum Format {
    RGB24Format
}

enum Layer {
    ContainerLayerKind(@ContainerLayer),
    ImageLayerKind(@ImageLayer)
}

class CommonLayer {
    let mut parent: option<Layer>;
    let mut prev_sibling: option<Layer>;
    let mut next_sibling: option<Layer>;

    let mut transform: Matrix4<f32>;

    new() {
        self.parent = none;
        self.prev_sibling = none;
        self.next_sibling = none;

        self.transform = identity(0.0f32);
    }

    // FIXME: Workaround for cross-crate bug regarding mutability of class fields
    fn set_transform(new_transform: Matrix4<f32>) {
        self.transform = new_transform;
    }
}

class ContainerLayer {
    let mut common: CommonLayer;
    let mut first_child: option<Layer>;
    let mut last_child: option<Layer>;

    new() {
        self.common = CommonLayer();
        self.first_child = none;
        self.last_child = none;
    }
}

class Image {
    let width: uint;
    let height: uint;
    let format: Format;
    let data: [u8]/~;

    let mut texture: option<GLuint>;

    new(width: uint, height: uint, format: Format, +data: [u8]/~) {
        self.width = width;
        self.height = height;
        self.format = format;
        self.data = data;

        self.texture = none;
    }
}

class ImageLayer {
    let mut common: CommonLayer;
    let mut image: @layers::Image;

    new(image: @layers::Image) {
        self.common = CommonLayer();
        self.image = image;
    }

    // FIXME: Workaround for cross-crate bug
    fn set_image(new_image: @layers::Image) {
        self.image = new_image;
    }
}


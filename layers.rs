import azure::AzDrawTargetRef;
import geom::matrix::{Matrix4, identity};
import geom::size::Size2D;

import f32::num;
import std::cmp::fuzzy_eq;

enum Format {
    RGB24Format
}

enum Layer {
    ContainerLayerKind(@ContainerLayer),
    AzureLayerKind(@AzureLayer)
}

class CommonLayer {
    let mut parent: option<Layer>;
    let mut prev_sibling: option<Layer>;
    let mut next_sibling: option<Layer>;

    let mut size: Size2D<f32>;
    let mut transform: Matrix4<f32>;

    new(size: Size2D<f32>) {
        self.parent = none;
        self.prev_sibling = none;
        self.next_sibling = none;

        self.size = size;
        self.transform = identity(0.0f32);
    }
}

class ContainerLayer {
    let mut common: CommonLayer;
    let mut first_child: option<Layer>;
    let mut last_child: option<Layer>;

    new(size: Size2D<f32>) {
        self.common = CommonLayer(size);
        self.first_child = none;
        self.last_child = none;
    }
}

class AzureLayer {
    let mut common: CommonLayer;
    let mut draw_target: AzDrawTargetRef;

    new(size: Size2D<f32>, draw_target: AzDrawTargetRef) {
        self.common = CommonLayer(size);
        self.draw_target = draw_target;
    }
}


use layers::Layer;
use geom::size::Size2D;
use geom::matrix::Matrix4;

pub struct Scene {
    root: Layer,
    size: Size2D<f32>,
    transform: Matrix4<f32>
}

pub fn Scene(root: Layer, size: Size2D<f32>, transform: Matrix4<f32>) -> Scene {
    Scene {
        root: root,
        size: size,
        transform: transform
    }
}

impl Scene {
    // FIXME: Workaround for cross-crate bug regarding mutability of class fields
    fn set_transform(&mut self, new_transform: Matrix4<f32>) {
        self.transform = new_transform;
    }
}


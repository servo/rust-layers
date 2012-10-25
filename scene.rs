use layers::Layer;
use geom::size::Size2D;

pub struct Scene {
    mut root: Layer,
    mut size: Size2D<f32>,
    mut transform: Matrix4<f32>
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
    fn set_transform(new_transform: Matrix4<f32>) {
        self.transform = new_transform;
    }
}


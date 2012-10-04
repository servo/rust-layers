use layers::Layer;
use geom::size::Size2D;

pub struct Scene {
    mut root: Layer,
    mut size: Size2D<f32>
}

pub fn Scene(root: Layer, size: Size2D<f32>) -> Scene {
    Scene {
        root : root,
        size : size
    }
}


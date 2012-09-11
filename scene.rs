use layers::Layer;
use geom::size::Size2D;

struct Scene {
    mut root: Layer,
    mut size: Size2D<f32>
}


fn Scene(root: Layer, size: Size2D<f32>) -> Scene {
    Scene {
        root : root,
        size : size
    }
}


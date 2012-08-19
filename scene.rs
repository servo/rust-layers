import layers::Layer;
import geom::size::Size2D;

struct Scene {
    let mut root: Layer;
    let mut size: Size2D<f32>;

    new(root: Layer, size: Size2D<f32>) {
        self.root = root;
        self.size = size;
    }
}


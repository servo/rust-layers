import layers::Layer;

class Scene {
    let mut root: Layer;

    new(root: Layer) {
        self.root = root;
    }
}


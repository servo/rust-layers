// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use texturegl::Texture;

use geom::matrix::{Matrix4, identity};
use geom::size::Size2D;
use geom::rect::Rect;
use std::managed::mut_ptr_eq;

pub enum Format {
    ARGB32Format,
    RGB24Format
}

pub enum Layer {
    ContainerLayerKind(@mut ContainerLayer),
    TextureLayerKind(@mut TextureLayer),
}

impl Layer {
    pub fn with_common<T>(&self, f: |&mut CommonLayer| -> T) -> T {
        match *self {
            ContainerLayerKind(container_layer) => f(&mut container_layer.common),
            TextureLayerKind(texture_layer) => f(&mut texture_layer.common),
        }
    }
}

pub struct CommonLayer {
    parent: Option<Layer>,
    prev_sibling: Option<Layer>,
    next_sibling: Option<Layer>,

    transform: Matrix4<f32>,
}

impl CommonLayer {
    // FIXME: Workaround for cross-crate bug regarding mutability of class fields
    pub fn set_transform(&mut self, new_transform: Matrix4<f32>) {
        self.transform = new_transform;
    }
}

pub fn CommonLayer() -> CommonLayer {
    CommonLayer {
        parent: None,
        prev_sibling: None,
        next_sibling: None,
        transform: identity(),
    }
}


pub struct ContainerLayer {
    common: CommonLayer,
    first_child: Option<Layer>,
    last_child: Option<Layer>,
    scissor: Option<Rect<f32>>,
}


pub fn ContainerLayer() -> ContainerLayer {
    ContainerLayer {
        common: CommonLayer(),
        first_child: None,
        last_child: None,
        scissor: None,
    }
}

struct ChildIterator {
    priv current: Option<Layer>,
}

impl Iterator<Layer> for ChildIterator {
    fn next(&mut self) -> Option<Layer> {
        match self.current {
            None => None,
            Some(child) => {
                self.current = child.with_common(|x| x.next_sibling);
                Some(child)
            }
        }
    }
}

impl ContainerLayer {
    pub fn children(&self) -> ChildIterator {
        ChildIterator { current: self.first_child }
    }

    /// Adds a child to the beginning of the list.
    /// Only works when the child is disconnected from the layer tree.
    pub fn add_child_start(@mut self, new_child: Layer) {
        new_child.with_common(|new_child_common| {
            assert!(new_child_common.parent.is_none());
            assert!(new_child_common.prev_sibling.is_none());
            assert!(new_child_common.next_sibling.is_none());

            new_child_common.parent = Some(ContainerLayerKind(self));

            match self.first_child {
                None => {}
                Some(first_child) => {
                    first_child.with_common(|first_child_common| {
                        assert!(first_child_common.prev_sibling.is_none());
                        first_child_common.prev_sibling = Some(new_child);
                        new_child_common.next_sibling = Some(first_child);
                    });
                }
            }

            self.first_child = Some(new_child);

            match self.last_child {
                None => self.last_child = Some(new_child),
                Some(_) => {}
            }
        });
    }

    /// Adds a child to the end of the list.
    /// Only works when the child is disconnected from the layer tree.
    pub fn add_child_end(@mut self, new_child: Layer) {
        new_child.with_common(|new_child_common| {
            assert!(new_child_common.parent.is_none());
            assert!(new_child_common.prev_sibling.is_none());
            assert!(new_child_common.next_sibling.is_none());

            new_child_common.parent = Some(ContainerLayerKind(self));

            match self.last_child {
                None => {}
                Some(last_child) => {
                    last_child.with_common(|last_child_common| {
                        assert!(last_child_common.next_sibling.is_none());
                        last_child_common.next_sibling = Some(new_child);
                        new_child_common.prev_sibling = Some(last_child);
                    });
                }
            }

            self.last_child = Some(new_child);

            match self.first_child {
                None => self.first_child = Some(new_child),
                Some(_) => {}
            }
        });
    }
    
    pub fn remove_child(@mut self, child: Layer) {
        child.with_common(|child_common| {
            assert!(child_common.parent.is_some());
            match child_common.parent.unwrap() {
                ContainerLayerKind(ref container) => {
                    assert!(mut_ptr_eq(*container, self));
                },
                _ => fail!(~"Invalid parent of child in layer tree"),
            }

            match child_common.next_sibling {
                None => { // this is the last child
                    self.last_child = child_common.prev_sibling;
                },
                Some(ref sibling) => {
                    sibling.with_common(|sibling_common| {
                        sibling_common.prev_sibling = child_common.prev_sibling;
                    });
                }
            }
            match child_common.prev_sibling {
                None => { // this is the first child
                    self.first_child = child_common.next_sibling;
                },
                Some(ref sibling) => {
                    sibling.with_common(|sibling_common| {
                        sibling_common.next_sibling = child_common.next_sibling;
                    });
                }
            }           
        });
    }
}

/// Whether a texture should be flipped.
#[deriving(Eq)]
pub enum Flip {
    /// The texture should not be flipped.
    NoFlip,
    /// The texture should be flipped vertically.
    VerticalFlip,
}

pub struct TextureLayer {
    common: CommonLayer,
    /// A handle to the GPU texture.
    texture: Texture,
    /// The size of the texture in pixels.
    size: Size2D<uint>,
    /// Whether this texture is flipped vertically.
    flip: Flip,
}

impl TextureLayer {
    pub fn new(texture: Texture, size: Size2D<uint>, flip: Flip) -> TextureLayer {
        TextureLayer {
            common: CommonLayer(),
            texture: texture,
            size: size,
            flip: flip,
        }
    }
}


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
use std::cell::RefCell;
use std::rc::Rc;

pub enum Format {
    ARGB32Format,
    RGB24Format
}

#[deriving(Clone)]
pub enum Layer {
    ContainerLayerKind(Rc<ContainerLayer>),
    TextureLayerKind(Rc<TextureLayer>),
}

impl Layer {
    pub fn with_common<T>(&self, f: |&mut CommonLayer| -> T) -> T {
        match *self {
            ContainerLayerKind(ref container_layer) => {
                f(&mut *container_layer.common.borrow_mut())
            },
            TextureLayerKind(ref texture_layer) => {
                f(&mut *texture_layer.common.borrow_mut())
            },
        }
    }
}

pub struct CommonLayer {
    pub parent: Option<Layer>,
    pub prev_sibling: Option<Layer>,
    pub next_sibling: Option<Layer>,

    pub transform: Matrix4<f32>,
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
    pub common: RefCell<CommonLayer>,
    pub first_child: RefCell<Option<Layer>>,
    pub last_child: RefCell<Option<Layer>>,
    pub scissor: RefCell<Option<Rect<f32>>>,
}


pub fn ContainerLayer() -> ContainerLayer {
    ContainerLayer {
        common: RefCell::new(CommonLayer()),
        first_child: RefCell::new(None),
        last_child: RefCell::new(None),
        scissor: RefCell::new(None),
    }
}

pub struct ChildIterator {
    current: Option<Layer>,
}

impl Iterator<Layer> for ChildIterator {
    fn next(&mut self) -> Option<Layer> {
        let (new_current, result) =
            match self.current {
                None => (None, None),
                Some(ref child) => {
                    (child.with_common(|x| x.next_sibling.clone()),
                     Some(child.clone()))
                }
            };
        self.current = new_current;
        result
    }
}

impl ContainerLayer {
    pub fn children(&self) -> ChildIterator {
        ChildIterator {
            current: self.first_child.borrow().clone(),
        }
    }

    /// Adds a child to the beginning of the list.
    /// Only works when the child is disconnected from the layer tree.
    pub fn add_child_start(pseudo_self: Rc<ContainerLayer>, new_child: Layer) {
        new_child.with_common(|new_child_common| {
            assert!(new_child_common.parent.is_none());
            assert!(new_child_common.prev_sibling.is_none());
            assert!(new_child_common.next_sibling.is_none());

            new_child_common.parent = Some(ContainerLayerKind(pseudo_self.clone()));

            match *pseudo_self.first_child.borrow() {
                None => {}
                Some(ref first_child) => {
                    first_child.with_common(|first_child_common| {
                        assert!(first_child_common.prev_sibling.is_none());
                        first_child_common.prev_sibling = Some(new_child.clone());
                        new_child_common.next_sibling = Some(first_child.clone());
                    });
                }
            }

            *pseudo_self.first_child.borrow_mut() = Some(new_child.clone());

            let should_set = pseudo_self.last_child.borrow().is_none();
            if should_set {
                *pseudo_self.last_child.borrow_mut() = Some(new_child.clone());
            }
        });
    }

    /// Adds a child to the end of the list.
    /// Only works when the child is disconnected from the layer tree.
    pub fn add_child_end(pseudo_self: Rc<ContainerLayer>, new_child: Layer) {
        new_child.with_common(|new_child_common| {
            assert!(new_child_common.parent.is_none());
            assert!(new_child_common.prev_sibling.is_none());
            assert!(new_child_common.next_sibling.is_none());

            new_child_common.parent = Some(ContainerLayerKind(pseudo_self.clone()));


            match *pseudo_self.last_child.borrow() {
                None => {}
                Some(ref last_child) => {
                    last_child.with_common(|last_child_common| {
                        assert!(last_child_common.next_sibling.is_none());
                        last_child_common.next_sibling = Some(new_child.clone());
                        new_child_common.prev_sibling = Some(last_child.clone());
                    });
                }
            }

            *pseudo_self.last_child.borrow_mut() = Some(new_child.clone());

            let mut child = pseudo_self.first_child.borrow_mut();
            match *child {
                Some(_) => {},
                None => *child = Some(new_child.clone()),
            }
        });
    }
    
    pub fn remove_child(pseudo_self: Rc<ContainerLayer>, child: Layer) {
        child.with_common(|child_common| {
            assert!(child_common.parent.is_some());
            match child_common.parent {
                Some(ContainerLayerKind(ref container)) => {
                    assert!(container.deref() as *ContainerLayer ==
                            pseudo_self.deref() as *ContainerLayer);
                },
                _ => fail!("Invalid parent of child in layer tree"),
            }

            match child_common.next_sibling {
                None => { // this is the last child
                    *pseudo_self.last_child.borrow_mut() = child_common.prev_sibling.clone();
                },
                Some(ref sibling) => {
                    sibling.with_common(|sibling_common| {
                        sibling_common.prev_sibling = child_common.prev_sibling.clone();
                    });
                }
            }
            match child_common.prev_sibling {
                None => { // this is the first child
                    *pseudo_self.first_child.borrow_mut() = child_common.next_sibling.clone();
                },
                Some(ref sibling) => {
                    sibling.with_common(|sibling_common| {
                        sibling_common.next_sibling = child_common.next_sibling.clone();
                    });
                }
            }           
        });
    }

    pub fn remove_all_children(&self) {
        *self.first_child.borrow_mut() = None;
        *self.last_child.borrow_mut() = None;
    }
}

/// Whether a texture should be flipped.
#[deriving(PartialEq)]
pub enum Flip {
    /// The texture should not be flipped.
    NoFlip,
    /// The texture should be flipped vertically.
    VerticalFlip,
}

pub struct TextureLayer {
    pub common: RefCell<CommonLayer>,
    /// A handle to the GPU texture.
    pub texture: Texture,
    /// The size of the texture in pixels.
    size: Size2D<uint>,
    /// Whether this texture is flipped vertically.
    pub flip: Flip,
}

impl TextureLayer {
    pub fn new(texture: Texture, size: Size2D<uint>, flip: Flip) -> TextureLayer {
        TextureLayer {
            common: RefCell::new(CommonLayer()),
            texture: texture,
            size: size,
            flip: flip,
        }
    }
}


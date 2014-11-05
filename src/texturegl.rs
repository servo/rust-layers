// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! OpenGL-specific implementation of texturing.

use layers::LayerBuffer;

use geom::size::Size2D;
use gleam::gl;
use gleam::gl::{GLenum, GLint, GLsizei, GLuint};
use std::num::Zero;
use libc::c_void;

#[cfg(not(target_os = "android"))]
use gleam::gl::BGRA;

#[cfg(target_os = "android")]
use gleam::gl::BGRA_EXT as BGRA;

pub enum Format {
    ARGB32Format,
    RGB24Format
}

pub enum FilterMode {
    Nearest,
    Linear
}

/// Image data used when uploading to a texture.
pub struct TextureImageData<'a> {
    size: Size2D<uint>,
    format: Format,
    data: &'a [u8],
}

/// The texture target.
pub enum TextureTarget {
    /// TEXTURE_2D.
    TextureTarget2D,
    /// TEXTURE_RECTANGLE_ARB, with the size included.
    TextureTargetRectangle,
}

impl TextureTarget {

    #[cfg(not(target_os = "android"))]
    fn as_gl_target(self) -> GLenum {
        match self {
            TextureTarget2D => gl::TEXTURE_2D,
            TextureTargetRectangle => gl::TEXTURE_RECTANGLE_ARB,
        }
    }

    #[cfg(target_os = "android")]
    fn as_gl_target(self) -> GLenum {
        match self {
            TextureTarget2D => gl::TEXTURE_2D,
            TextureTargetRectangle => panic!("android doesn't supported rectangle targets"),
        }
    }
}

/// A texture.
///
/// TODO: Include client storage here for `GL_CLIENT_STORAGE_APPLE`.
pub struct Texture {
    /// The OpenGL texture ID.
    id: GLuint,

    /// The texture target.
    pub target: TextureTarget,

    /// Whether this texture is weak. Weak textures will not be cleaned up by
    /// the destructor.
    weak: bool,

    // Whether or not this texture needs to be flipped upon display.
    pub flip: Flip,

    // The size of this texture in device pixels.
    pub size: Size2D<uint>
}

impl Drop for Texture {
    fn drop(&mut self) {
        if !self.weak {
            gl::delete_textures([ self.id ])
        }
    }
}

// This Trait is implemented because it is required
// for Zero, but we should never call it on textures.
impl Add<Texture, Texture> for Texture {
    fn add(&self, _: &Texture) -> Texture {
        panic!("Textures cannot be added.");
    }
}

impl Zero for Texture {
    fn zero() -> Texture {
        Texture {
            id: 0,
            target: TextureTarget2D,
            weak: true,
            flip: NoFlip,
            size: Size2D(0u, 0u),
        }
    }
    fn is_zero(&self) -> bool {
        self.id == 0
    }
}

/// Encapsulates a bound texture. This ensures that the texture is unbound
/// properly.
pub struct BoundTexture {
    pub target: TextureTarget
}

impl Drop for BoundTexture {
    fn drop(&mut self) {
        gl::bind_texture(self.target.as_gl_target(), 0);
    }
}

impl Texture {
    /// Creates a new blank texture.
    pub fn new(target: TextureTarget, size: Size2D<uint>) -> Texture {
        let this = Texture {
            id: gl::gen_textures(1)[0],
            target: target,
            weak: false,
            flip: NoFlip,
            size: size,
        };
        this.set_default_params();
        this
    }

    pub fn new_with_buffer(buffer: &Box<LayerBuffer>) -> Texture {
        let (flip, target) = Texture::texture_flip_and_target(buffer.painted_with_cpu);
        let mut texture = Texture::new(target, buffer.screen_pos.size);
        texture.flip = flip;
        return texture;
    }

    // Returns whether the layer should be vertically flipped.
    #[cfg(target_os="macos")]
    fn texture_flip_and_target(cpu_painting: bool) -> (Flip, TextureTarget) {
        let flip = if cpu_painting {
            NoFlip
        } else {
            VerticalFlip
        };

        (flip, TextureTargetRectangle)
    }

    #[cfg(target_os="android")]
    fn texture_flip_and_target(cpu_painting: bool) -> (Flip, TextureTarget) {
        let flip = if cpu_painting {
            NoFlip
        } else {
            VerticalFlip
        };

        (flip, TextureTarget2D)
    }

    #[cfg(target_os="linux")]
    fn texture_flip_and_target(_: bool) -> (Flip, TextureTarget) {
        (NoFlip, TextureTarget2D)
    }

    /// Returns the raw OpenGL texture underlying this texture.
    pub fn native_texture(&self) -> GLuint {
        self.id
    }

    /// Sets default parameters for this texture.
    fn set_default_params(&self) {
        let _bound_texture = self.bind();
        gl::tex_parameter_i(self.target.as_gl_target(), gl::TEXTURE_MAG_FILTER, gl::LINEAR as GLint);
        gl::tex_parameter_i(self.target.as_gl_target(), gl::TEXTURE_MIN_FILTER, gl::LINEAR as GLint);
        gl::tex_parameter_i(self.target.as_gl_target(), gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as GLint);
        gl::tex_parameter_i(self.target.as_gl_target(), gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as GLint);
    }

    /// Sets the filter mode for this texture.
    pub fn set_filter_mode(&self, mode: FilterMode) {
        let _bound_texture = self.bind();
        let gl_mode = match mode {
            Nearest => gl::NEAREST,
            Linear => gl::LINEAR,
        } as GLint;
        gl::tex_parameter_i(self.target.as_gl_target(), gl::TEXTURE_MAG_FILTER, gl_mode);
        gl::tex_parameter_i(self.target.as_gl_target(), gl::TEXTURE_MIN_FILTER, gl_mode);
    }

    /// Binds the texture to the current context.
    pub fn bind(&self) -> BoundTexture {
        gl::bind_texture(self.target.as_gl_target(), self.id);

        BoundTexture {
            target: self.target,
        }
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

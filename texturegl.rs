// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! OpenGL-specific implementation of texturing.

use layers::{ARGB32Format, Format, RGB24Format};

use geom::size::Size2D;
use opengles::gl2::{BGRA, CLAMP_TO_EDGE, GLenum, GLint, GLsizei, GLuint, LINEAR, RGB, RGBA};
use opengles::gl2::{TEXTURE_MAG_FILTER, TEXTURE_MIN_FILTER, TEXTURE_2D, TEXTURE_RECTANGLE_ARB};
use opengles::gl2::{TEXTURE_WRAP_S, TEXTURE_WRAP_T, UNSIGNED_BYTE, UNSIGNED_INT_8_8_8_8_REV};
use opengles::gl2;
use std::num::Zero;

/// Image data used when uploading to a texture.
pub struct TextureImageData<'a> {
    size: Size2D<uint>,
    stride: uint,
    format: Format,
    data: &'a [u8],
}

/// The texture target.
pub enum TextureTarget {
    /// TEXTURE_2D.
    TextureTarget2D,
    /// TEXTURE_RECTANGLE_ARB, with the size included.
    TextureTargetRectangle(Size2D<uint>),
}

impl TextureTarget {
    fn as_gl_target(self) -> GLenum {
        match self {
            TextureTarget2D => TEXTURE_2D,
            TextureTargetRectangle(_) => TEXTURE_RECTANGLE_ARB,
        }
    }
}

/// A texture.
///
/// TODO: Include client storage here for `GL_CLIENT_STORAGE_APPLE`.
pub struct Texture {
    /// The OpenGL texture ID.
    priv id: GLuint,
    /// The texture target.
    target: TextureTarget,
    /// Whether this texture is weak. Weak textures will not be cleaned up by
    /// the destructor.
    priv weak: bool,
}

impl Drop for Texture {
    fn drop(&mut self) {
        if !self.weak {
            gl2::delete_textures([ self.id ])
        }
    }
}

// This Trait is implemented because it is required
// for Zero, but we should never call it on textures.
impl Add<Texture, Texture> for Texture {
    fn add(&self, _: &Texture) -> Texture {
        fail!("Textures cannot be added.");
    }
}

impl Zero for Texture {
    fn zero() -> Texture {
        Texture {
            id: 0,
            target: TextureTarget2D,
            weak: true,
        }
    }
    fn is_zero(&self) -> bool {
        self.id == 0
    }
}

/// Encapsulates a bound texture. This ensures that the texture is unbound
/// properly.
struct BoundTexture {
    target: TextureTarget
}

impl Drop for BoundTexture {
    fn drop(&mut self) {
        gl2::bind_texture(self.target.as_gl_target(), 0)
    }
}

impl Texture {
    /// Creates a new blank texture.
    pub fn new(target: TextureTarget) -> Texture {
        let this = Texture {
            id: gl2::gen_textures(1)[0],
            target: target,
            weak: false,
        };
        this.set_default_params();
        this
    }

    /// Creates a texture from an existing OpenGL texture. The texture will be deleted when this
    /// `Texture` object goes out of scope.
    pub fn adopt_native_texture(native_texture_id: GLuint, target: TextureTarget) -> Texture {
        let this = Texture {
            id: native_texture_id,
            target: target,
            weak: false,
        };
        this
    }

    /// Creates a texture from an existing OpenGL texture. The texture will *not* be deleted when
    /// this `Texture` object goes out of scope.
    pub fn wrap_native_texture(native_texture_id: GLuint, target: TextureTarget) -> Texture {
        let this = Texture {
            id: native_texture_id,
            target: target,
            weak: true,
        };
        this.set_default_params();
        this
    }

    /// Returns the raw OpenGL texture underlying this texture.
    pub fn native_texture(&self) -> GLuint {
        self.id
    }

    /// Sets default parameters for this texture.
    fn set_default_params(&self) {
        let _bound_texture = self.bind();
        gl2::tex_parameter_i(self.target.as_gl_target(), TEXTURE_MAG_FILTER, LINEAR as GLint);
        gl2::tex_parameter_i(self.target.as_gl_target(), TEXTURE_MIN_FILTER, LINEAR as GLint);
        gl2::tex_parameter_i(self.target.as_gl_target(), TEXTURE_WRAP_S, CLAMP_TO_EDGE as GLint);
        gl2::tex_parameter_i(self.target.as_gl_target(), TEXTURE_WRAP_T, CLAMP_TO_EDGE as GLint);
    }

    /// Binds the texture to the current context.
    pub fn bind(&self) -> BoundTexture {
        gl2::bind_texture(self.target.as_gl_target(), self.id);

        BoundTexture {
            target: self.target,
        }
    }

    /// Uploads raw image data to the texture.
    pub fn upload_image<'a>(&self, texture_image_data: &TextureImageData<'a>) {
        let _bound_texture = self.bind();

        match texture_image_data.format {
            RGB24Format => {
                gl2::tex_image_2d(self.target.as_gl_target(),
                                  0,
                                  RGB as GLint,
                                  texture_image_data.size.width as GLsizei,
                                  texture_image_data.size.height as GLsizei,
                                  0,
                                  RGB,
                                  UNSIGNED_BYTE,
                                  Some(texture_image_data.data))
            }
            ARGB32Format => {
                gl2::tex_image_2d(self.target.as_gl_target(),
                                  0,
                                  RGBA as GLint,
                                  texture_image_data.size.width as GLsizei,
                                  texture_image_data.size.height as GLsizei,
                                  0,
                                  BGRA,
                                  UNSIGNED_INT_8_8_8_8_REV,
                                  Some(texture_image_data.data))
            }
        }
    }
}


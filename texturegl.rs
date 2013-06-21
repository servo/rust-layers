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

use core::num::Zero;
use geom::size::Size2D;
use opengles::gl2::{BGRA, CLAMP_TO_EDGE, GLenum, GLint, GLsizei, GLuint, LINEAR, RGB, RGBA};
use opengles::gl2::{TEXTURE_MAG_FILTER, TEXTURE_MIN_FILTER, TEXTURE_2D, TEXTURE_WRAP_S};
use opengles::gl2::{TEXTURE_WRAP_T, UNSIGNED_BYTE, UNSIGNED_INT_8_8_8_8_REV};
use opengles::gl2;

/// Image data used when uploading to a texture.
///
/// FIXME(pcwalton): This is annoyingly close to `BasicImageData`, except that it uses a reference
/// rather than an owning pointer.
pub struct TextureImageData<'self> {
    size: Size2D<uint>,
    stride: uint,
    format: Format,
    data: &'self [u8],
}

/// A texture.
///
/// TODO: Include client storage here for `GL_CLIENT_STORAGE_APPLE`.
pub struct Texture {
    priv id: GLuint,
    /// Whether this texture is weak. Weak textures will not be cleaned up by
    /// the destructor.
    priv weak: bool,
}

impl Drop for Texture {
    pub fn finalize(&self) {
        if !self.weak {
            gl2::delete_textures([ self.id ])
        }
    }
}

impl Zero for Texture {
    pub fn zero() -> Texture {
        Texture {
            id: 0,
            weak: true,
        }
    }
    pub fn is_zero(&self) -> bool {
        self.id == 0
    }
}

/// Encapsulates a bound texture. This ensures that the texture is unbound
/// properly.
struct BoundTexture {
    /// FIXME(pcwalton): Workaround for compiler bug.
    dummy: ()
}

impl Drop for BoundTexture {
    fn finalize(&self) {
        gl2::bind_texture(TEXTURE_2D, 0)
    }
}

impl Texture {
    /// Creates a new blank texture.
    pub fn new() -> Texture {
        let this = Texture {
            id: gl2::gen_textures(1)[0],
            weak: false,
        };
        this.set_default_params();
        this
    }

    /// Creates a texture from an existing OpenGL texture. The texture will be deleted when this
    /// `Texture` object goes out of scope.
    pub fn adopt_native_texture(native_texture_id: GLuint) -> Texture {
        let this = Texture {
            id: native_texture_id,
            weak: false,
        };
        this.set_default_params();
        this
    }

    /// Creates a texture from an existing OpenGL texture. The texture will *not* be deleted when
    /// this `Texture` object goes out of scope.
    pub fn wrap_native_texture(native_texture_id: GLuint) -> Texture {
        let this = Texture {
            id: native_texture_id,
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
        gl2::tex_parameter_i(TEXTURE_2D, TEXTURE_MAG_FILTER, LINEAR as GLint);
        gl2::tex_parameter_i(TEXTURE_2D, TEXTURE_MIN_FILTER, LINEAR as GLint);
        gl2::tex_parameter_i(TEXTURE_2D, TEXTURE_WRAP_S, CLAMP_TO_EDGE as GLint);
        gl2::tex_parameter_i(TEXTURE_2D, TEXTURE_WRAP_T, CLAMP_TO_EDGE as GLint);
    }

    /// Binds the texture to the current context.
    pub fn bind(&self) -> BoundTexture {
        gl2::bind_texture(TEXTURE_2D, self.id);

        BoundTexture {
            dummy: ()
        }
    }

    /// Uploads raw image data to the texture.
    pub fn upload_image<'a>(&self, texture_image_data: &TextureImageData<'a>) {
        let _bound_texture = self.bind();

        match texture_image_data.format {
            RGB24Format => {
                gl2::tex_image_2d(TEXTURE_2D,
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
                gl2::tex_image_2d(TEXTURE_2D,
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


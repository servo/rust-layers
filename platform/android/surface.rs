// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Implementation of cross-process surfaces for Android. This uses EGL surface.

use platform::surface::NativeSurfaceMethods;
use texturegl::Texture;

use geom::size::Size2D;
use opengles::gl2::{egl_image_target_texture2d_oes, TEXTURE_2D, glTexImage2D, BGRA, UNSIGNED_BYTE};
use egl::egl::EGLDisplay;
use egl::eglext::{EGLImageKHR, DestroyImageKHR};
use std::cast;
use std::mem;
use std::ptr;
use std::vec;
use std::libc::c_void;

/// FIXME(Aydin Kim) :Currently, native surface is consist of 2 types of hybrid image buffer. EGLImageKHR is used to GPU rendering and vector is used to CPU rendering. EGL extension seems not provide simple way to accessing its bitmap directly. In the future, we need to find out the way to integrate them.

pub struct NativeGraphicsMetadata {
    display : EGLDisplay,
}

pub struct NativePaintingGraphicsContext{
    display : EGLDisplay,
}

impl NativePaintingGraphicsContext {
    pub fn from_metadata(metadata: &NativeGraphicsMetadata) -> NativePaintingGraphicsContext {
        NativePaintingGraphicsContext {
            display : metadata.display,
        }
    }
}

impl Drop for NativePaintingGraphicsContext {
    fn drop(&mut self) {}
}

pub struct NativeCompositingGraphicsContext {
    contents: (),
}

impl NativeCompositingGraphicsContext {
    pub fn new() -> NativeCompositingGraphicsContext {
        NativeCompositingGraphicsContext {
            contents: (),
        }
    }
}

pub struct NativeSurface {
    image: Option<EGLImageKHR>,
    bitmap: *c_void,
    will_leak: bool,
}

impl NativeSurface {
    pub fn from_image_khr(image_khr: EGLImageKHR) -> NativeSurface {
        let mut _image: Option<EGLImageKHR> = None;
        if image_khr != ptr::null() {
            _image = Some(image_khr);
        }
        NativeSurface {
            image : _image,
            bitmap : ptr::null(),
            will_leak: true,
        }
    }
}

impl NativeSurfaceMethods for NativeSurface {
    /// This may only be called on the case of CPU rendering.
    fn new(_native_context: &NativePaintingGraphicsContext, size: Size2D<i32>, _stride: i32) -> NativeSurface {
        unsafe {
            let len = size.width * size.height * 4;
            let bitmap = vec::from_elem::<u8>(len as uint, 0);

            NativeSurface {
                image: None,
                bitmap: cast::transmute(bitmap),
                will_leak : true,
            }
        }
    }

    /// This may only be called on the compositor side.
    fn bind_to_texture(&self,
                       _native_context: &NativeCompositingGraphicsContext,
                       texture: &Texture,
                       _size: Size2D<int>) {
        let _bound = texture.bind();

        unsafe {
            match self.image {
                None => {
                    if self.bitmap != ptr::null() {
                        glTexImage2D(TEXTURE_2D, 0, BGRA as i32, _size.width as i32, _size.height as i32, 0, BGRA as u32, UNSIGNED_BYTE, self.bitmap);
                    }
                    else {
                        debug!("Cannot bind the buffer(CPU rendering), there is no bitmap");
                    }
                },
                Some(image_khr) => {
                    egl_image_target_texture2d_oes(TEXTURE_2D, image_khr);
                }
            }
        }
    }

    /// This may only be called on the painting side.
    fn upload(&self, _graphics_context: &NativePaintingGraphicsContext, data: &[u8]) {
        unsafe {
            if self.bitmap != ptr::null() {
                let dest:&mut [u8] = cast::transmute((self.bitmap, data.len()));
                dest.copy_memory(data);
            }
            else {
                debug!("Cannot upload the buffer(CPU rendering), there is no bitmap");
            }
        }
    }

    fn get_id(&self) -> int {
        match self.image {
            None => 0,
            Some(image_khr) => image_khr as int,
        }
    }

    fn destroy(&mut self, graphics_context: &NativePaintingGraphicsContext) {
        match self.image {
            None => {},
            Some(image_khr) => {
                DestroyImageKHR(graphics_context.display, image_khr);
                mem::replace(&mut self.image, None);
            }
        }
        self.mark_wont_leak()
    }

    fn mark_will_leak(&mut self) {
        self.will_leak = true
    }
    fn mark_wont_leak(&mut self) {
        self.will_leak = false
    }
}

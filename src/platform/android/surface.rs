// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Implementation of cross-process surfaces for Android. This uses EGL surface.

use texturegl::Texture;

use azure::AzSkiaGrGLSharedSurfaceRef;
use geom::size::Size2D;
use gleam::gl::{egl_image_target_texture2d_oes, TEXTURE_2D, TexImage2D, BGRA_EXT, UNSIGNED_BYTE};
use egl::egl::EGLDisplay;
use egl::eglext::{EGLImageKHR, DestroyImageKHR};
use libc::c_void;
use std::mem;
use std::ptr;
use std::slice::bytes::copy_memory;
use std::vec::Vec;

/// FIXME(Aydin Kim) :Currently, native surface is consist of 2 types of hybrid image buffer. EGLImageKHR is used to GPU rendering and vector is used to CPU rendering. EGL extension seems not provide simple way to accessing its bitmap directly. In the future, we need to find out the way to integrate them.

#[derive(Clone, Copy)]
pub struct NativeGraphicsMetadata {
    pub display: EGLDisplay,
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

#[derive(Copy)]
pub struct NativeCompositingGraphicsContext;

impl NativeCompositingGraphicsContext {
    pub fn new() -> NativeCompositingGraphicsContext {
        NativeCompositingGraphicsContext
    }
}

pub struct EGLImageNativeSurface {
    image: Option<EGLImageKHR>, // For GPU rendering
    bitmap: Option<Vec<u8>>, // For CPU rendering
    will_leak: bool,
}

impl EGLImageNativeSurface {
    pub fn from_image_khr(image_khr: EGLImageKHR) -> EGLImageNativeSurface {
        let mut _image: Option<EGLImageKHR> = None;
        if image_khr != ptr::null_mut() {
            _image = Some(image_khr);
        }
        EGLImageNativeSurface {
            image : _image,
            bitmap: None,
            will_leak: true,
        }
    }

    pub fn from_azure_surface(surface: AzSkiaGrGLSharedSurfaceRef) -> EGLImageNativeSurface {
        unsafe {
            EGLImageNativeSurface::from_image_khr(mem::transmute(surface))
        }
    }

    /// This may only be called on the case of CPU rendering.
    pub fn new(_: &NativePaintingGraphicsContext, size: Size2D<i32>, _stride: i32) -> EGLImageNativeSurface {
        let len = size.width * size.height * 4;
        let bitmap: Vec<u8> = Vec::from_elem(len as uint, 0 as u8);

        EGLImageNativeSurface {
            image: None,
            bitmap: Some(bitmap),
            will_leak : true,
        }
    }

    /// This may only be called on the compositor side.
    pub fn bind_to_texture(&self,
                           _: &NativeCompositingGraphicsContext,
                           texture: &Texture,
                           size: Size2D<int>) {
        let _bound = texture.bind();
        match self.image {
            None => match self.bitmap {
                Some(ref bitmap) => {
                    let data = bitmap.as_ptr() as *const c_void;
                    unsafe {
                        TexImage2D(TEXTURE_2D, 0, BGRA_EXT as i32, size.width as i32, size.height as i32,
                                   0, BGRA_EXT as u32, UNSIGNED_BYTE, data);
                    }
                }
                None => {
                    debug!("Cannot bind the buffer(CPU rendering), there is no bitmap");
                }
            },
            Some(image_khr) => {
                egl_image_target_texture2d_oes(TEXTURE_2D, image_khr as *const c_void);
            }
        }
    }

    /// This may only be called on the painting side.
    pub fn upload(&mut self, _: &NativePaintingGraphicsContext, data: &[u8]) {
        match self.bitmap {
            Some(ref mut bitmap) => {
                copy_memory(bitmap.as_mut_slice(), data);
            }
            None => {
                debug!("Cannot upload the buffer(CPU rendering), there is no bitmap");
            }
        }
    }

    pub fn get_id(&self) -> int {
        match self.image {
            None => 0,
            Some(image_khr) => image_khr as int,
        }
    }

    pub fn destroy(&mut self, graphics_context: &NativePaintingGraphicsContext) {
        match self.image {
            None => {},
            Some(image_khr) => {
                DestroyImageKHR(graphics_context.display, image_khr);
                mem::replace(&mut self.image, None);
            }
        }
        self.mark_wont_leak()
    }

    pub fn mark_will_leak(&mut self) {
        self.will_leak = true
    }

    pub fn mark_wont_leak(&mut self) {
        self.will_leak = false
    }
}

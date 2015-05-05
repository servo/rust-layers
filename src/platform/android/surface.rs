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

use geom::size::Size2D;
use gleam::gl::{egl_image_target_texture2d_oes, TEXTURE_2D, TexImage2D, BGRA_EXT, UNSIGNED_BYTE};
use egl::egl::EGLDisplay;
use egl::eglext::{EGLImageKHR, DestroyImageKHR};
use libc::c_void;
use skia::{SkiaSkNativeSharedGLContextRef, SkiaSkNativeSharedGLContextStealSurface};
use std::iter::repeat;
use std::mem;
use std::ptr;
use std::vec::Vec;

/// FIXME(Aydin Kim) :Currently, native surface is consist of 2 types of hybrid image buffer. EGLImageKHR is used to GPU rendering and vector is used to CPU rendering. EGL extension seems not provide simple way to accessing its bitmap directly. In the future, we need to find out the way to integrate them.

#[derive(Clone, Copy)]
pub struct NativeGraphicsMetadata {
    pub display: EGLDisplay,
}
unsafe impl Send for NativeGraphicsMetadata {}

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

#[derive(Copy, Clone)]
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

unsafe impl Send for EGLImageNativeSurface {}

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

    pub fn from_skia_shared_gl_context(context: SkiaSkNativeSharedGLContextRef)
                                       -> EGLImageNativeSurface {
        unsafe {
            let surface = SkiaSkNativeSharedGLContextStealSurface(context);
            EGLImageNativeSurface::from_image_khr(mem::transmute(surface))
        }
    }

    /// This may only be called on the case of CPU rendering.
    pub fn new(_: &NativePaintingGraphicsContext, size: Size2D<i32>, _stride: i32) -> EGLImageNativeSurface {
        let len = size.width * size.height * 4;
        let bitmap: Vec<u8> = repeat(0).take(len as usize).collect();

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
                           size: Size2D<isize>) {
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
                bitmap.clear();
                bitmap.push_all(data);
            }
            None => {
                debug!("Cannot upload the buffer(CPU rendering), there is no bitmap");
            }
        }
    }

    pub fn get_id(&self) -> isize {
        match self.image {
            None => 0,
            Some(image_khr) => image_khr as isize,
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

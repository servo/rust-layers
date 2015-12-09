// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Implementation of cross-process surfaces for Android. This uses EGL surface.

use platform::surface::MemoryBufferNativeSurface;
use texturegl::Texture;

use egl::egl::{EGLDisplay, GetCurrentDisplay};
use egl::eglext::{EGLImageKHR, DestroyImageKHR};
use euclid::size::Size2D;
use gleam::gl::{egl_image_target_texture2d_oes, TEXTURE_2D};
use skia::gl_context::{GLContext, PlatformDisplayData};
use skia::gl_rasterization_context::GLRasterizationContext;
use std::mem;
use std::os::raw::c_void;
use std::sync::Arc;

/// FIXME(Aydin Kim) :Currently, native surface is consist of 2 types of hybrid image
/// buffer. EGLImageKHR is used to GPU rendering and vector is used to CPU rendering. EGL
/// extension seems not provide simple way to accessing its bitmap directly. In the
/// future, we need to find out the way to integrate them.

#[derive(Clone, Copy)]
pub struct NativeDisplay {
    pub display: EGLDisplay,
}
unsafe impl Send for NativeDisplay {}

impl NativeDisplay {
    pub fn new() -> NativeDisplay {
        NativeDisplay::new_with_display(GetCurrentDisplay())
    }

    pub fn new_with_display(display: EGLDisplay) -> NativeDisplay {
        NativeDisplay {
            display: display,
        }
    }

    pub fn platform_display_data(&self) -> PlatformDisplayData {
        PlatformDisplayData {
            display: self.display,
        }
    }
}

pub struct EGLImageNativeSurface {
    /// An EGLImage, which stores the contents of the surface.
    egl_image: EGLImageKHR,

    /// Whether this pixmap will leak if the destructor runs. This is for debugging purposes.
    will_leak: bool,

    /// The size of this surface.
    pub size: Size2D<i32>,
}

impl EGLImageNativeSurface {
    pub fn new(egl_image: EGLImageKHR, size: Size2D<i32>) -> EGLImageNativeSurface {
        EGLImageNativeSurface {
            egl_image: egl_image,
            will_leak: true,
            size: size,
        }
    }

    /// This may only be called on the compositor side.
    pub fn bind_to_texture(&self, _: &NativeDisplay, texture: &Texture) {
        let _bound = texture.bind();
        egl_image_target_texture2d_oes(TEXTURE_2D, self.egl_image as *const c_void);
    }

    /// This may only be called on the painting side.
    pub fn upload(&mut self, _: &NativeDisplay, _: &[u8]) {
        panic!("Cannot upload a to an EGLImage surface.");
    }

    pub fn get_id(&self) -> isize {
        self.egl_image as isize
    }

    pub fn destroy(&mut self, egl_display: EGLDisplay) {
        DestroyImageKHR(egl_display, self.egl_image);
        self.mark_wont_leak()
    }

    pub fn mark_will_leak(&mut self) {
        self.will_leak = true
    }

    pub fn mark_wont_leak(&mut self) {
        self.will_leak = false
    }
}

pub enum AndroidNativeSurface {
    EGLImage(EGLImageNativeSurface),
    MemoryBuffer(MemoryBufferNativeSurface),
}

unsafe impl Send for AndroidNativeSurface {}

impl AndroidNativeSurface {
    pub fn new(native_display: &NativeDisplay, size: Size2D<i32>) -> AndroidNativeSurface {
        AndroidNativeSurface::MemoryBuffer(MemoryBufferNativeSurface::new(native_display, size))
    }

    /// This may only be called on the compositor side.
    pub fn bind_to_texture(&self, native_display: &NativeDisplay, texture: &Texture) {
        match *self {
            AndroidNativeSurface::EGLImage(ref surface) =>
                surface.bind_to_texture(native_display, texture),
            AndroidNativeSurface::MemoryBuffer(ref surface) =>
                surface.bind_to_texture(native_display, texture),
        }
    }

    /// This may only be called on the painting side.
    pub fn upload(&mut self, native_display: &NativeDisplay, data: &[u8]) {
        match *self {
            AndroidNativeSurface::EGLImage(_) => panic!("Cannot upload a to an EGLImage surface."),
            AndroidNativeSurface::MemoryBuffer(ref mut surface) => surface.upload(native_display, data),
        }
    }

    pub fn get_id(&self) -> isize {
        match *self {
            AndroidNativeSurface::EGLImage(ref surface) => surface.get_id(),
            AndroidNativeSurface::MemoryBuffer(ref surface) => surface.get_id(),
        }
    }

    pub fn get_size(&self) -> Size2D<i32> {
        match *self {
            AndroidNativeSurface::EGLImage(ref surface) => surface.size,
            AndroidNativeSurface::MemoryBuffer(ref surface) => surface.get_size(),
        }
    }

    pub fn destroy(&mut self, native_display: &NativeDisplay) {
        match *self {
            AndroidNativeSurface::EGLImage(ref mut surface) => surface.destroy(native_display.display),
            AndroidNativeSurface::MemoryBuffer(ref mut surface) => surface.destroy(native_display),
        }
    }

    pub fn mark_will_leak(&mut self) {
        match *self {
            AndroidNativeSurface::EGLImage(ref mut surface) => surface.mark_will_leak(),
            AndroidNativeSurface::MemoryBuffer(ref mut surface) => surface.mark_will_leak(),
        }
    }

    pub fn mark_wont_leak(&mut self) {
        match *self {
            AndroidNativeSurface::EGLImage(ref mut surface) => surface.mark_wont_leak(),
            AndroidNativeSurface::MemoryBuffer(ref mut surface) => surface.mark_wont_leak(),
        }
    }

    pub fn gl_rasterization_context(&mut self,
                                    gl_context: Arc<GLContext>)
                                    -> Option<GLRasterizationContext> {
        // TODO: Eventually we should preserve the previous GLRasterizationContext,
        // so that we don't have to keep destroying and recreating the image.
        let size = self.get_size();
        let gl_rasterization_context = GLRasterizationContext::new(gl_context, size);
        if let Some(ref gl_rasterization_context) = gl_rasterization_context {
            match *self {
                AndroidNativeSurface::EGLImage(ref mut surface) =>
                    surface.destroy(gl_rasterization_context.gl_context.platform_context.display),
                AndroidNativeSurface::MemoryBuffer(_) => {}
            }
            mem::replace(self, AndroidNativeSurface::EGLImage(
                EGLImageNativeSurface::new(gl_rasterization_context.egl_image, size)));
        }
        gl_rasterization_context
    }
}

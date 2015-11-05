// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Implementation of cross-process surfaces. This delegates to the platform-specific
//! implementation.

use texturegl::Texture;

use euclid::size::Size2D;
use skia::gl_rasterization_context::GLRasterizationContext;
use skia::gl_context::GLContext;
use skia::gl_context::PlatformDisplayData;
use std::sync::Arc;

#[cfg(not(target_os="android"))]
use gleam::gl;

#[cfg(target_os="macos")]
pub use platform::macos::surface::{NativeDisplay,
                                   IOSurfaceNativeSurface};

#[cfg(target_os="linux")]
pub use platform::linux::surface::{NativeDisplay,
                                   PixmapNativeSurface};
#[cfg(target_os="linux")]
use std::ptr;

#[cfg(target_os="android")]
pub use platform::android::surface::{NativeDisplay,
                                     EGLImageNativeSurface};

#[cfg(target_os="windows")]
pub use platform::windows::surface::NativeDisplay;

pub enum NativeSurface {
    MemoryBuffer(MemoryBufferNativeSurface),
#[cfg(target_os="linux")]
    Pixmap(PixmapNativeSurface),
#[cfg(target_os="macos")]
    IOSurface(IOSurfaceNativeSurface),
#[cfg(target_os="android")]
    EGLImage(EGLImageNativeSurface),
}

#[cfg(target_os="linux")]
impl NativeSurface {
    /// Creates a new native surface with uninitialized data.
    pub fn new(display: &NativeDisplay, size: Size2D<i32>) -> NativeSurface {
        if display.display == ptr::null_mut() {
            NativeSurface::MemoryBuffer(MemoryBufferNativeSurface::new(display, size))
        } else {
            NativeSurface::Pixmap(PixmapNativeSurface::new(display, size))
        }
   }
}

#[cfg(target_os="macos")]
impl NativeSurface {
    /// Creates a new native surface with uninitialized data.
    pub fn new(display: &NativeDisplay, size: Size2D<i32>) -> NativeSurface {
        NativeSurface::IOSurface(IOSurfaceNativeSurface::new(display, size))
   }
}

#[cfg(target_os="android")]
impl NativeSurface {
    /// Creates a new native surface with uninitialized data.
    pub fn new(display: &NativeDisplay, size: Size2D<i32>) -> NativeSurface {
        NativeSurface::EGLImage(EGLImageNativeSurface::new(display, size))
   }
}

#[cfg(target_os="windows")]
impl NativeSurface {
    /// Creates a new native surface with uninitialized data.
    pub fn new(display: &NativeDisplay, size: Size2D<i32>) -> NativeSurface {
        NativeSurface::MemoryBuffer(MemoryBufferNativeSurface::new(display, size))
   }
}

macro_rules! native_surface_method_with_mutability {
    ($self_:ident, $function_name:ident, $surface:ident, $pattern:pat, $($argument:ident),*) => {
        match *$self_ {
            NativeSurface::MemoryBuffer($pattern) =>
                $surface.$function_name($($argument), *),
            #[cfg(target_os="linux")]
            NativeSurface::Pixmap($pattern) =>
                $surface.$function_name($($argument), *),
            #[cfg(target_os="macos")]
            NativeSurface::IOSurface($pattern) =>
                $surface.$function_name($($argument), *),
            #[cfg(target_os="android")]
            NativeSurface::EGLImage($pattern) =>
                $surface.$function_name($($argument), *),
        }
    };
}

macro_rules! native_surface_method_mut {
    ($self_:ident $function_name:ident ($($argument:ident),*)) => {
        native_surface_method_with_mutability!($self_,
                                               $function_name,
                                               surface,
                                               ref mut surface,
                                               $($argument),
                                               *)
    };
}

macro_rules! native_surface_method {
    ($self_:ident $function_name:ident ($($argument:ident),*)) => {
        native_surface_method_with_mutability!($self_,
                                               $function_name,
                                               surface,
                                               ref surface,
                                               $($argument),
                                               *)
    };
}

macro_rules! native_surface_property {
    ($self_:ident $property_name:ident) => {
        match *$self_ {
            NativeSurface::MemoryBuffer(ref surface) => surface.$property_name,
            #[cfg(target_os="linux")]
            NativeSurface::Pixmap(ref surface) => surface.$property_name,
            #[cfg(target_os="macos")]
            NativeSurface::IOSurface(ref surface) => surface.$property_name,
            #[cfg(target_os="android")]
            NativeSurface::EGLImage(ref surface) => surface.$property_name,
        }
    };
}

impl NativeSurface {
    /// Binds the surface to a GPU texture. Compositing task only.
    pub fn bind_to_texture(&self, display: &NativeDisplay, texture: &Texture) {
        native_surface_method!(self bind_to_texture (display, texture))
    }

    /// Uploads pixel data to the surface. Painting task only.
    pub fn upload(&mut self, display: &NativeDisplay, data: &[u8]) {
        native_surface_method_mut!(self upload (display, data))
    }

    /// Returns an opaque ID identifying the surface for debugging.
    pub fn get_id(&self) -> isize {
        native_surface_method!(self get_id ())
    }

    /// Destroys the surface. After this, it is an error to use the surface. Painting task only.
    pub fn destroy(&mut self, display: &NativeDisplay) {
        native_surface_method_mut!(self destroy (display))
    }

    /// Records that the surface will leak if destroyed. This is done by the compositor immediately
    /// after receiving the surface.
    pub fn mark_will_leak(&mut self) {
        native_surface_method_mut!(self mark_will_leak ())
    }

    /// Marks the surface as not leaking. The painting task and the compositing task call this when
    /// they are certain that the surface will not leak. For example:
    ///
    /// 1. When sending buffers back to the render task, either the render task will receive them
    ///    or the render task has crashed. In the former case, they're the render task's
    ///    responsibility, so this is OK. In the latter case, the kernel or window server is going
    ///    to clean up the layer buffers. Either way, no leaks.
    ///
    /// 2. If the compositor is shutting down, the render task is also shutting down. In that case
    ///    it will destroy all its pixmaps, so no leak.
    ///
    /// 3. If the painting task is sending buffers to the compositor, then they are marked as not
    ///    leaking, because of the possibility that the compositor will die before the buffers are
    ///    destroyed.
    ///
    /// This helps debug leaks. For performance this may want to become a no-op in the future.
    pub fn mark_wont_leak(&mut self) {
        native_surface_method_mut!(self mark_wont_leak ())
    }

    pub fn gl_rasterization_context(&mut self,
                                    gl_context: Arc<GLContext>)
                                    -> Option<Arc<GLRasterizationContext>> {
        match native_surface_method_mut!(self gl_rasterization_context (gl_context)) {
            Some(context) => Some(Arc::new(context)),
            None => None,
        }

    }

    /// Get the memory usage of this native surface. This memory may be allocated
    /// on the GPU or on the heap.
    pub fn get_memory_usage(&self) -> usize {
        // This works for now, but in the future we may want a better heuristic
        let size = self.get_size();
        size.width as usize * size.height as usize
    }

    /// Get the size of this native surface.
    pub fn get_size(&self) -> Size2D<i32> {
        native_surface_property!(self size)
    }
}

#[derive(RustcDecodable, RustcEncodable)]
pub struct MemoryBufferNativeSurface {
    bytes: Vec<u8>,
    pub size: Size2D<i32>,
}

impl MemoryBufferNativeSurface {
    pub fn new(_: &NativeDisplay, size: Size2D<i32>) -> MemoryBufferNativeSurface {
        MemoryBufferNativeSurface{
            bytes: vec!(),
            size: size,
        }
    }

    /// This may only be called on the compositor side.
    #[cfg(not(target_os="android"))]
    pub fn bind_to_texture(&self, _: &NativeDisplay, texture: &Texture) {
        let _bound = texture.bind();
        gl::tex_image_2d(gl::TEXTURE_2D,
                         0,
                         gl::RGBA as i32,
                         self.size.width as i32,
                         self.size.height as i32,
                         0,
                         gl::BGRA,
                         gl::UNSIGNED_BYTE,
                         Some(&self.bytes));
        unsafe { if cfg!(feature = "gldebug") { assert_eq!(gl::GetError(), gl::NO_ERROR); }}
    }

    #[cfg(target_os="android")]
    pub fn bind_to_texture(&self, _: &NativeDisplay, _: &Texture) {
        panic!("Binding a memory surface to a texture is not yet supported on Android.");
    }

    /// This may only be called on the painting side.
    pub fn upload(&mut self, _: &NativeDisplay, data: &[u8]) {
        self.bytes.clear();
        self.bytes.push_all(data);
    }

    pub fn get_id(&self) -> isize {
        0
    }

    pub fn destroy(&mut self, _: &NativeDisplay) {
    }

    pub fn mark_will_leak(&mut self) {
    }

    pub fn mark_wont_leak(&mut self) {
    }

    pub fn gl_rasterization_context(&mut self,
                                    _: Arc<GLContext>)
                                    -> Option<GLRasterizationContext> {
        None
    }
}


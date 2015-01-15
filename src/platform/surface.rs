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

use geom::size::Size2D;
use azure::azure_hl::DrawTargetBacking;
use std::ptr;

#[cfg(not(target_os="android"))]
use gleam::gl;

#[cfg(target_os="macos")]
pub use platform::macos::surface::{NativeCompositingGraphicsContext,
                                   NativeGraphicsMetadata,
                                   NativePaintingGraphicsContext,
                                   IOSurfaceNativeSurface};

#[cfg(target_os="linux")]
pub use platform::linux::surface::{NativeCompositingGraphicsContext,
                                   NativeGraphicsMetadata,
                                   NativePaintingGraphicsContext,
                                   PixmapNativeSurface};

#[cfg(target_os="android")]
pub use platform::android::surface::{NativeCompositingGraphicsContext,
                                     NativeGraphicsMetadata,
                                     NativePaintingGraphicsContext,
                                     EGLImageNativeSurface};

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
    pub fn new(native_context: &NativePaintingGraphicsContext,
               size: Size2D<i32>,
               stride: i32)
               -> NativeSurface {
        if native_context.display == ptr::null_mut() {
            NativeSurface::MemoryBuffer(MemoryBufferNativeSurface::new(native_context, size, stride))
        } else {
            NativeSurface::Pixmap(PixmapNativeSurface::new(native_context, size, stride))
        }
   }
}

#[cfg(target_os="macos")]
impl NativeSurface {
    /// Creates a new native surface with uninitialized data.
    pub fn new(native_context: &NativePaintingGraphicsContext,
               size: Size2D<i32>,
               stride: i32)
               -> NativeSurface {
        NativeSurface::IOSurface(IOSurfaceNativeSurface::new(native_context, size, stride))
   }
}

#[cfg(target_os="android")]
impl NativeSurface {
    /// Creates a new native surface with uninitialized data.
    pub fn new(native_context: &NativePaintingGraphicsContext,
               size: Size2D<i32>,
               stride: i32)
               -> NativeSurface {
        NativeSurface::EGLImage(EGLImageNativeSurface::new(native_context, size, stride))
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

impl NativeSurface {
    pub fn from_draw_target_backing(backing: DrawTargetBacking) -> NativeSurface {
        match backing {
            #[cfg(target_os="macos")]
            DrawTargetBacking::SkiaContext(context) =>
                NativeSurface::IOSurface(IOSurfaceNativeSurface::from_skia_shared_gl_context(context)),
            #[cfg(target_os="linux")]
            DrawTargetBacking::SkiaContext(context) =>
                NativeSurface::Pixmap(PixmapNativeSurface::from_skia_shared_gl_context(context)),
            #[cfg(target_os="android")]
            DrawTargetBacking::SkiaContext(context) =>
                NativeSurface::EGLImage(EGLImageNativeSurface::from_skia_shared_gl_context(context)),
            _ => panic!("Cannot yet construct native surface from non-GL DrawTarget."),
        }
    }


    /// Binds the surface to a GPU texture. Compositing task only.
    pub fn bind_to_texture(&self,
                           native_context: &NativeCompositingGraphicsContext,
                           texture: &Texture,
                           size: Size2D<int>) {
        native_surface_method!(self bind_to_texture (native_context, texture, size))
    }

    /// Uploads pixel data to the surface. Painting task only.
    pub fn upload(&mut self, native_context: &NativePaintingGraphicsContext, data: &[u8]) {
        native_surface_method_mut!(self upload (native_context, data))
    }

    /// Returns an opaque ID identifying the surface for debugging.
    pub fn get_id(&self) -> int {
        native_surface_method!(self get_id ())
    }

    /// Destroys the surface. After this, it is an error to use the surface. Painting task only.
    pub fn destroy(&mut self, graphics_context: &NativePaintingGraphicsContext) {
        native_surface_method_mut!(self destroy (graphics_context))
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
}

#[derive(RustcDecodable, RustcEncodable)]
pub struct MemoryBufferNativeSurface {
    bytes: Vec<u8>,
}

impl MemoryBufferNativeSurface {
    pub fn new(_: &NativePaintingGraphicsContext, _: Size2D<i32>, _: i32) -> MemoryBufferNativeSurface {
        MemoryBufferNativeSurface{
            bytes: vec!(),
        }
    }

    /// This may only be called on the compositor side.
    #[cfg(not(target_os="android"))]
    pub fn bind_to_texture(&self, _: &NativeCompositingGraphicsContext, texture: &Texture, size: Size2D<int>) {
        let _bound = texture.bind();
        gl::tex_image_2d(gl::TEXTURE_2D,
                         0,
                         gl::RGBA as i32,
                         size.width as i32,
                         size.height as i32,
                         0,
                         gl::BGRA,
                         gl::UNSIGNED_BYTE,
                         Some(self.bytes.as_slice()));
    }

    #[cfg(target_os="android")]
    pub fn bind_to_texture(&self, _: &NativeCompositingGraphicsContext, _: &Texture, _: Size2D<int>) {
        panic!("Binding a memory surface to a texture is not yet supported on Android.");
    }

    /// This may only be called on the painting side.
    pub fn upload(&mut self, _: &NativePaintingGraphicsContext, data: &[u8]) {
        self.bytes.push_all(data);
    }

    pub fn get_id(&self) -> int {
        0
    }

    pub fn destroy(&mut self, _: &NativePaintingGraphicsContext) {
    }

    pub fn mark_will_leak(&mut self) {
    }

    pub fn mark_wont_leak(&mut self) {
    }
}


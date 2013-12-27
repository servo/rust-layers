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

#[cfg(target_os="macos")]
pub use platform::macos::surface::NativePaintingGraphicsContext;
#[cfg(target_os="macos")]
pub use platform::macos::surface::NativeCompositingGraphicsContext;
#[cfg(target_os="macos")]
pub use platform::macos::surface::NativeGraphicsMetadata;
#[cfg(target_os="macos")]
pub use platform::macos::surface::NativeGraphicsMetadataDescriptor;
#[cfg(target_os="macos")]
pub use platform::macos::surface::NativeSurface;
#[cfg(target_os="linux")]
pub use platform::linux::surface::NativePaintingGraphicsContext;
#[cfg(target_os="linux")]
pub use platform::linux::surface::NativeCompositingGraphicsContext;
#[cfg(target_os="linux")]
pub use platform::linux::surface::NativeGraphicsMetadata;
#[cfg(target_os="linux")]
pub use platform::linux::surface::NativeGraphicsMetadataDescriptor;
#[cfg(target_os="linux")]
pub use platform::linux::surface::NativeSurface;
#[cfg(target_os="android")]
pub use platform::android::surface::NativePaintingGraphicsContext;
#[cfg(target_os="android")]
pub use platform::android::surface::NativeCompositingGraphicsContext;
#[cfg(target_os="android")]
pub use platform::android::surface::NativeGraphicsMetadata;
#[cfg(target_os="android")]
pub use platform::android::surface::NativeSurface;

pub trait NativeSurfaceMethods {
    /// Creates a new native surface with uninitialized data.
    fn new(native_context: &NativePaintingGraphicsContext, size: Size2D<i32>, stride: i32) -> Self;

    /// Binds the surface to a GPU texture. Compositing task only.
    fn bind_to_texture(&self,
                       native_context: &NativeCompositingGraphicsContext,
                       texture: &Texture,
                       size: Size2D<int>);

    /// Uploads pixel data to the surface. Painting task only.
    fn upload(&self, native_context: &NativePaintingGraphicsContext, data: &[u8]);

    /// Returns an opaque ID identifying the surface for debugging.
    fn get_id(&self) -> int;

    /// Destroys the surface. After this, it is an error to use the surface. Painting task only.
    fn destroy(&mut self, graphics_context: &NativePaintingGraphicsContext);

    /// Records that the surface will leak if destroyed. This is done by the compositor immediately
    /// after receiving the surface.
    fn mark_will_leak(&mut self);

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
    fn mark_wont_leak(&mut self);
}


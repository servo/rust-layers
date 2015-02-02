// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Implementation of cross-process surfaces for Linux. This uses X pixmaps.

#![allow(non_snake_case)]

use texturegl::Texture;

use geom::size::Size2D;
use libc::{c_char, c_int, c_uint, c_void};
use glx;
use gleam::gl;
use skia::{SkiaSkNativeSharedGLContextRef, SkiaSkNativeSharedGLContextStealSurface};
use std::ascii::{AsciiExt, OwnedAsciiExt};
use std::ffi::{CString, c_str_to_bytes};
use std::mem;
use std::ptr;
use std::str;
use xlib::{Display, Pixmap, XCreateGC, XCreateImage, XCreatePixmap, XDefaultScreen};
use xlib::{XDisplayString, XFree, XFreePixmap, XGetGeometry, XOpenDisplay, XPutImage, XRootWindow};
use xlib::{XVisualInfo, ZPixmap};

/// The display and visual info. This is needed in order to upload on the painting side. This
/// holds a weak reference to the display and will not close it when done.
///
/// FIXME(pcwalton): Mark nonsendable.
#[allow(missing_copy_implementations)]
pub struct NativePaintingGraphicsContext {
    pub display: *mut Display,
    visual_info: *mut XVisualInfo,
}

impl NativePaintingGraphicsContext {
    pub fn from_metadata(metadata: &NativeGraphicsMetadata) -> NativePaintingGraphicsContext {
        // FIXME(pcwalton): It would be more robust to actually have the compositor pass the
        // visual.
        let (compositor_visual_info, _) =
            NativeCompositingGraphicsContext::compositor_visual_info(metadata.display);

        NativePaintingGraphicsContext {
            display: metadata.display,
            visual_info: compositor_visual_info,
        }
    }
}

/// The display, visual info, and framebuffer configuration. This is needed in order to bind to a
/// texture on the compositor side. This holds only a *weak* reference to the display and does not
/// close it.
///
/// FIXME(pcwalton): Unchecked weak references are bad and can violate memory safety. This is hard
/// to fix because the Display is given to us by the native windowing system, but we should fix it
/// someday.
///
/// FIXME(pcwalton): Mark nonsendable.
#[derive(Copy)]
pub struct NativeCompositingGraphicsContext {
    display: *mut Display,
    framebuffer_configuration: Option<glx::types::GLXFBConfig>,
}

impl NativeCompositingGraphicsContext {
    /// Chooses the compositor visual info using the same algorithm that the compositor uses.
    ///
    /// FIXME(pcwalton): It would be more robust to actually have the compositor pass the visual.
    fn compositor_visual_info(display: *mut Display) -> (*mut XVisualInfo, Option<glx::types::GLXFBConfig>) {
        // If display is null, we'll assume we are going to be rendering
        // in headless mode without X running.
        if display == ptr::null_mut() {
            return (ptr::null_mut(), None);
        }

        unsafe {
            let fbconfig_attributes = [
                glx::DOUBLEBUFFER as i32, 0,
                glx::DRAWABLE_TYPE as i32, glx::PIXMAP_BIT as i32 | glx::WINDOW_BIT as i32,
                glx::BIND_TO_TEXTURE_RGBA_EXT as i32, 1,
                glx::RENDER_TYPE as i32, glx::RGBA_BIT as i32,
                glx::ALPHA_SIZE as i32, 8,
                0
            ];

            let screen = XDefaultScreen(display);
            let mut number_of_configs = 0;
            let configs = glx::ChooseFBConfig(mem::transmute(display),
                                              screen,
                                              fbconfig_attributes.as_ptr(),
                                              &mut number_of_configs);
            NativeCompositingGraphicsContext::get_compatible_configuration(display,
                                                                           configs,
                                                                           number_of_configs)
        }
    }

    fn get_compatible_configuration(display: *mut Display,
                                    configs: *mut glx::types::GLXFBConfig,
                                    number_of_configs: i32)
                                    -> (*mut XVisualInfo, Option<glx::types::GLXFBConfig>) {
        unsafe {
            if number_of_configs == 0 {
                panic!("glx::ChooseFBConfig returned no configurations.");
            }

            if !NativeCompositingGraphicsContext::need_to_find_32_bit_depth_visual(display) {
                let config = *configs.offset(0);
                let visual = glx::GetVisualFromFBConfig(mem::transmute(display), config);
                return (mem::transmute(visual), Some(config));
            }

            // NVidia (and AMD/ATI) drivers have RGBA configurations that use 24-bit
            // XVisual, not capable of representing an alpha-channel in Pixmap form,
            // so we look for the configuration with a full set of 32 bits.
            for i in range(0, number_of_configs as int) {
                let config = *configs.offset(i);
                let visual: *mut XVisualInfo =
                    mem::transmute(glx::GetVisualFromFBConfig(mem::transmute(display), config));
                if (*visual).depth == 32 {
                    return (mem::transmute(visual), Some(config));
                }
                XFree(mem::transmute(visual));
            }

            panic!("Could not find 32-bit visual.");
        }
    }

    fn need_to_find_32_bit_depth_visual(display: *mut Display) -> bool {
        unsafe {
            let glXGetClientString: extern "C" fn(*mut Display, c_int) -> *const c_char =
                mem::transmute(glx::GetProcAddress(mem::transmute(&"glXGetClientString\x00".as_bytes()[0])));
            assert!(glXGetClientString as *mut c_void != ptr::null_mut());

            let glx_vendor = glx::GetClientString(mem::transmute(display), glx::VENDOR as i32);
            if glx_vendor == ptr::null() {
                panic!("Could not determine GLX vendor.");
            }
            let glx_vendor =
                str::from_utf8(c_str_to_bytes(&glx_vendor))
                    .ok()
                    .expect("GLX client vendor string not in UTF-8 format.");
            let glx_vendor = String::from_str(glx_vendor).into_ascii_lowercase();
            glx_vendor.contains("nvidia") || glx_vendor.contains("ati")
        }
    }

    /// Creates a native graphics context from the given X display connection. This uses GLX. Only
    /// the compositor is allowed to call this.
    pub fn from_display(display: *mut Display) -> NativeCompositingGraphicsContext {
        let (_, fbconfig) = NativeCompositingGraphicsContext::compositor_visual_info(display);

        NativeCompositingGraphicsContext {
            display: display,
            framebuffer_configuration: fbconfig,
        }
    }
}

/// The X display.
#[derive(Clone, Copy)]
pub struct NativeGraphicsMetadata {
    pub display: *mut Display,
}
unsafe impl Send for NativeGraphicsMetadata {}

impl NativeGraphicsMetadata {
    /// Creates graphics metadata from a metadata descriptor.
    pub fn from_descriptor(descriptor: &NativeGraphicsMetadataDescriptor)
                           -> NativeGraphicsMetadata {
        // WARNING: We currently rely on the X display connection being the
        // same in both the Painting and Compositing contexts, as otherwise
        // the X Pixmap will not be sharable across them. Using this
        // method breaks that assumption.
        unsafe {
            let mut c_str = CString::from_slice(descriptor.display.as_bytes());
            let display = XOpenDisplay(c_str.as_ptr() as *mut _);
            if display.is_null() {
                panic!("XOpenDisplay() failed!");
            }
            NativeGraphicsMetadata {
                display: display,
            }
        }
    }
}

/// A sendable form of the X display string.
#[derive(Clone, RustcDecodable, RustcEncodable)]
pub struct NativeGraphicsMetadataDescriptor {
    display: String,
}

impl NativeGraphicsMetadataDescriptor {
    /// Creates a metadata descriptor from metadata.
    pub fn from_metadata(metadata: NativeGraphicsMetadata) -> NativeGraphicsMetadataDescriptor {
        unsafe {
            let c_str = XDisplayString(metadata.display) as *const _;
            let bytes = c_str_to_bytes(&c_str);
            NativeGraphicsMetadataDescriptor {
                display: str::from_utf8(bytes).unwrap().to_string(),
            }
        }
    }
}

#[derive(RustcDecodable, RustcEncodable)]
pub struct PixmapNativeSurface {
    /// The pixmap.
    pixmap: Pixmap,

    /// Whether this pixmap will leak if the destructor runs. This is for debugging purposes.
    will_leak: bool,
}

impl Drop for PixmapNativeSurface {
    fn drop(&mut self) {
        if self.will_leak {
            panic!("You should have disposed of the pixmap properly with destroy()! This pixmap \
                   will leak!");
        }
    }
}

impl PixmapNativeSurface {
    fn from_pixmap(pixmap: Pixmap) -> PixmapNativeSurface {
        PixmapNativeSurface {
            pixmap: pixmap,
            will_leak: true,
        }
    }

    pub fn from_skia_shared_gl_context(context: SkiaSkNativeSharedGLContextRef)
                                       -> PixmapNativeSurface {
        unsafe {
            let surface = SkiaSkNativeSharedGLContextStealSurface(context);
            PixmapNativeSurface::from_pixmap(mem::transmute(surface))
        }
    }

    pub fn new(native_context: &NativePaintingGraphicsContext, size: Size2D<i32>, _stride: i32)
           -> PixmapNativeSurface {
        unsafe {
            // Create the pixmap.
            let screen = XDefaultScreen(native_context.display);
            let window = XRootWindow(native_context.display, screen);
            // The X server we use for testing on build machines always returns
            // visuals that report 24 bit depth. But creating a 32 bit pixmap does work, so
            // hard code the depth here.
            let pixmap = XCreatePixmap(native_context.display,
                                       window,
                                       size.width as c_uint,
                                       size.height as c_uint,
                                       32);
            PixmapNativeSurface::from_pixmap(pixmap)
        }
    }

    /// This may only be called on the compositor side.
    pub fn bind_to_texture(&self,
                           native_context: &NativeCompositingGraphicsContext,
                           texture: &Texture,
                           size: Size2D<int>) {
        // Create the GLX pixmap.
        //
        // FIXME(pcwalton): RAII for exception safety?
        unsafe {
            let pixmap_attributes = [
                glx::TEXTURE_TARGET_EXT as i32, glx::TEXTURE_2D_EXT as i32,
                glx::TEXTURE_FORMAT_EXT as i32, glx::TEXTURE_FORMAT_RGBA_EXT as i32,
                0
            ];

            let glx_display = mem::transmute(native_context.display);

            let glx_pixmap = glx::CreatePixmap(glx_display,
                                             native_context.framebuffer_configuration.expect(
                                                 "GLX 1.3 should have a framebuffer_configuration"),
                                             self.pixmap,
                                             pixmap_attributes.as_ptr());

            let glXBindTexImageEXT: extern "C" fn(*mut Display, glx::types::GLXDrawable, c_int, *mut c_int) =
                mem::transmute(glx::GetProcAddress(mem::transmute(&"glXBindTexImageEXT\x00".as_bytes()[0])));
            assert!(glXBindTexImageEXT as *mut c_void != ptr::null_mut());
            let _bound = texture.bind();
            glXBindTexImageEXT(native_context.display,
                               mem::transmute(glx_pixmap),
                               glx::FRONT_EXT  as i32,
                               ptr::null_mut());
            assert_eq!(gl::GetError(), gl::NO_ERROR);

            // FIXME(pcwalton): Recycle these for speed?
            glx::DestroyPixmap(glx_display, glx_pixmap);
        }
    }

    /// This may only be called on the painting side.
    pub fn upload(&mut self, graphics_context: &NativePaintingGraphicsContext, data: &[u8]) {
        unsafe {
            // Ensure that we're running on the render task. Take the display.
            let pixmap = self.pixmap;

            // Figure out the width, height, and depth of the pixmap.
            let mut root_window = 0;
            let mut x = 0;
            let mut y = 0;
            let mut width = 0;
            let mut height = 0;
            let mut border_width = 0;
            let mut depth = 0;
            let _ = XGetGeometry(graphics_context.display,
                                 mem::transmute(pixmap),
                                 &mut root_window,
                                 &mut x,
                                 &mut y,
                                 &mut width,
                                 &mut height,
                                 &mut border_width,
                                 &mut depth);

            // Create the image.
            let image = XCreateImage(graphics_context.display,
                                     (*graphics_context.visual_info).visual,
                                     depth,
                                     ZPixmap,
                                     0,
                                     mem::transmute(&data[0]),
                                     width as c_uint,
                                     height as c_uint,
                                     32,
                                     0);

            // Create the X graphics context.
            let gc = XCreateGC(graphics_context.display, pixmap, 0, ptr::null_mut());

            // Draw the image.
            let _ = XPutImage(graphics_context.display,
                              pixmap,
                              gc,
                              image,
                              0,
                              0,
                              0,
                              0,
                              width,
                              height);
        }
    }

    pub fn get_id(&self) -> int {
        self.pixmap as int
    }

    pub fn destroy(&mut self, graphics_context: &NativePaintingGraphicsContext) {
        unsafe {
            assert!(self.pixmap != 0);
            XFreePixmap(graphics_context.display, self.pixmap);
            self.mark_wont_leak()
        }
    }

    pub fn mark_will_leak(&mut self) {
        self.will_leak = true;
    }

    pub fn mark_wont_leak(&mut self) {
        self.will_leak = false;
    }
}

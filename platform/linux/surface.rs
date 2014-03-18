// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Implementation of cross-process surfaces for Linux. This uses X pixmaps.

use platform::surface::NativeSurfaceMethods;
use texturegl::Texture;

use geom::size::Size2D;
use opengles::glx::{GLXFBConfig, GLXDrawable};
use opengles::glx::{GLX_BIND_TO_TEXTURE_RGBA_EXT};
use opengles::glx::{GLX_DRAWABLE_TYPE, GLX_FRONT_EXT, GLX_PIXMAP_BIT};
use opengles::glx::{GLX_TEXTURE_2D_EXT, GLX_TEXTURE_FORMAT_EXT, GLX_TEXTURE_FORMAT_RGBA_EXT};
use opengles::glx::{GLX_TEXTURE_TARGET_EXT, glXCreatePixmap, glXDestroyPixmap};
use opengles::glx::{glXGetProcAddress, glXChooseFBConfig};
use opengles::glx::{glXGetVisualFromFBConfig};
use opengles::glx::{GLX_RGBA_BIT, GLX_WINDOW_BIT, GLX_RENDER_TYPE, GLX_DOUBLEBUFFER};
use opengles::gl2::NO_ERROR;
use opengles::gl2;
use std::cast;
use std::c_str::CString;
use std::libc::{c_int, c_uint, c_void};
use std::ptr;
use xlib::xlib::{Display, Pixmap, XCreateGC, XCreateImage, XCreatePixmap, XDefaultScreen};
use xlib::xlib::{XDisplayString, XFreePixmap, XGetGeometry, XOpenDisplay, XPutImage, XRootWindow};
use xlib::xlib::{XVisualInfo, ZPixmap};

/// The display and visual info. This is needed in order to upload on the painting side. This
/// holds a weak reference to the display and will not close it when done.
///
/// FIXME(pcwalton): Mark nonsendable and noncopyable.
pub struct NativePaintingGraphicsContext {
    display: *Display,
    visual_info: *XVisualInfo,
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
pub struct NativeCompositingGraphicsContext {
    display: *Display,
    visual_info: *XVisualInfo,
    framebuffer_configuration: Option<GLXFBConfig>,
}

impl NativeCompositingGraphicsContext {
    /// Chooses the compositor visual info using the same algorithm that the compositor uses.
    ///
    /// FIXME(pcwalton): It would be more robust to actually have the compositor pass the visual.
    fn compositor_visual_info(display: *Display) -> (*XVisualInfo, Option<GLXFBConfig>) {
        unsafe {
            let glx_display = cast::transmute(display);

            // CONSIDER:
            // In skia, they compute the GLX_ALPHA_SIZE minimum and request
            // that as well.

            let fbconfig_attributes = [
                GLX_DOUBLEBUFFER, 0,
                GLX_DRAWABLE_TYPE, GLX_PIXMAP_BIT | GLX_WINDOW_BIT,
                GLX_BIND_TO_TEXTURE_RGBA_EXT, 1,
                GLX_RENDER_TYPE, GLX_RGBA_BIT,
                0
            ];

            let screen = XDefaultScreen(display);
            let mut configs = 0;
            let fbconfigs = glXChooseFBConfig(glx_display, screen,
                                              &fbconfig_attributes[0], &mut configs);
            if configs == 0 {
                fail!("Unable to locate a GLX FB configuration that supports RGBA.");
            }
            
            let fbconfig = *fbconfigs.offset(0);
            let vi = glXGetVisualFromFBConfig(glx_display, fbconfig);
            (cast::transmute(vi), Some(fbconfig))
        }
    }

    /// Creates a native graphics context from the given X display connection. This uses GLX. Only
    /// the compositor is allowed to call this.
    pub fn from_display(display: *Display) -> NativeCompositingGraphicsContext {
        let (visual_info, fbconfig) = NativeCompositingGraphicsContext::compositor_visual_info(display);

        NativeCompositingGraphicsContext {
            display: display,
            visual_info: visual_info,
            framebuffer_configuration: fbconfig,
        }
    }
}

/// The X display.
#[deriving(Clone)]
pub struct NativeGraphicsMetadata {
    display: *Display,
}

impl NativeGraphicsMetadata {
    /// Creates graphics metadata from a metadata descriptor.
    pub fn from_descriptor(descriptor: &NativeGraphicsMetadataDescriptor)
                           -> NativeGraphicsMetadata {
        // WARNING: We currently rely on the X display connection being the
        // same in both the Painting and Compositing contexts, as otherwise
        // the X Pixmap will not be sharable across them. Using this
        // method breaks that assumption.
        unsafe {
            let display = descriptor.display.with_c_str(|c_str| {
                    XOpenDisplay(c_str)
                });
            
            if display.is_null() {
                fail!("XOpenDisplay() failed!");
            }
            
            NativeGraphicsMetadata {
                display: display,
            }
        }
    }
}

/// A sendable form of the X display string.
#[deriving(Clone, Decodable, Encodable)]
pub struct NativeGraphicsMetadataDescriptor {
    display: ~str,
}

impl NativeGraphicsMetadataDescriptor {
    /// Creates a metadata descriptor from metadata.
    pub fn from_metadata(metadata: NativeGraphicsMetadata) -> NativeGraphicsMetadataDescriptor {
        unsafe {
            let c_str = CString::new(XDisplayString(metadata.display), false);
            NativeGraphicsMetadataDescriptor {
                display: c_str.as_str().unwrap().to_str(),
            }
        }
    }
}

#[deriving(Eq)]
pub enum NativeSurfaceTransientData {
    NoTransientData,
    RenderTaskTransientData(*Display, *XVisualInfo),
}

#[deriving(Decodable, Encodable)]
pub struct NativeSurface {
    /// The pixmap.
    pixmap: Pixmap,

    /// Whether this pixmap will leak if the destructor runs. This is for debugging purposes.
    will_leak: bool,
}

impl Drop for NativeSurface {
    fn drop(&mut self) {
        if self.will_leak {
            fail!("You should have disposed of the pixmap properly with destroy()! This pixmap \
                   will leak!");
        }
    }
}

impl NativeSurface {
    pub fn from_pixmap(pixmap: Pixmap) -> NativeSurface {
        NativeSurface {
            pixmap: pixmap,
            will_leak: true,
        }
    }
}

impl NativeSurfaceMethods for NativeSurface {
    fn new(native_context: &NativePaintingGraphicsContext, size: Size2D<i32>, stride: i32)
           -> NativeSurface {
        unsafe {
            // Create the pixmap.
            let screen = XDefaultScreen(native_context.display);
            let window = XRootWindow(native_context.display, screen);
            let pixmap = XCreatePixmap(native_context.display,
                                       window,
                                       size.width as c_uint,
                                       size.height as c_uint,
                                       ((stride / size.width) * 8) as c_uint);
            NativeSurface::from_pixmap(pixmap)
        }
    }

    /// This may only be called on the compositor side.
    fn bind_to_texture(&self,
                       native_context: &NativeCompositingGraphicsContext,
                       texture: &Texture,
                       _size: Size2D<int>) {
        unsafe {
            // Create the GLX pixmap.
            //
            // FIXME(pcwalton): RAII for exception safety?
            let pixmap_attributes = [
                GLX_TEXTURE_TARGET_EXT, GLX_TEXTURE_2D_EXT,
                GLX_TEXTURE_FORMAT_EXT, GLX_TEXTURE_FORMAT_RGBA_EXT,
                0
            ];

            let glx_display = cast::transmute(native_context.display);
        
            let glx_pixmap = glXCreatePixmap(glx_display,
                                             native_context.framebuffer_configuration.expect(
                                                 "GLX 1.3 should have a framebuffer_configuration"),
                                             self.pixmap,
                                             &pixmap_attributes[0]);

            let glXBindTexImageEXT: extern "C" fn(*Display, GLXDrawable, c_int, *c_int) =
                cast::transmute(glXGetProcAddress(cast::transmute(&"glXBindTexImageEXT\x00"[0])));
            assert!(glXBindTexImageEXT as *c_void != ptr::null());
            let _bound = texture.bind();
            glXBindTexImageEXT(native_context.display,
                               cast::transmute(glx_pixmap),
                               GLX_FRONT_EXT,
                               ptr::null());
            assert_eq!(gl2::get_error(), NO_ERROR);

            // FIXME(pcwalton): Recycle these for speed?
            glXDestroyPixmap(glx_display, glx_pixmap);
        }
    }

    /// This may only be called on the painting side.
    fn upload(&self, graphics_context: &NativePaintingGraphicsContext, data: &[u8]) {
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
                                 cast::transmute(pixmap),
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
                                     cast::transmute(&data[0]),
                                     width as c_uint,
                                     height as c_uint,
                                     32,
                                     0);

            // Create the X graphics context.
            let gc = XCreateGC(graphics_context.display, pixmap, 0, ptr::null());

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

    fn get_id(&self) -> int {
        self.pixmap as int
    }

    fn destroy(&mut self, graphics_context: &NativePaintingGraphicsContext) {
        unsafe {
            assert!(self.pixmap != 0);
            XFreePixmap(graphics_context.display, self.pixmap);
            self.mark_wont_leak()
        }
    }

    fn mark_will_leak(&mut self) {
        self.will_leak = true
    }

    fn mark_wont_leak(&mut self) {
        self.will_leak = false
    }
}


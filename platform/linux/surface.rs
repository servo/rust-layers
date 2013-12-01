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
use opengles::glx::{GLX_DEPTH_SIZE, GLX_DRAWABLE_TYPE, GLX_FRONT_EXT, GLX_PIXMAP_BIT, GLX_RGBA};
use opengles::glx::{GLX_TEXTURE_2D_EXT, GLX_TEXTURE_FORMAT_EXT, GLX_TEXTURE_FORMAT_RGBA_EXT};
use opengles::glx::{GLX_TEXTURE_TARGET_EXT, glXChooseVisual, glXCreatePixmap, glXDestroyPixmap};
use opengles::glx::{glXCreateGLXPixmap, glXDestroyGLXPixmap};
use opengles::glx::{glXGetProcAddress, glXChooseFBConfig};
use opengles::glx::{glXGetVisualFromFBConfig, glXQueryVersion, get_version};
use opengles::glx::{GLX_RGBA_BIT, GLX_WINDOW_BIT, GLX_RENDER_TYPE, GLX_ALPHA_SIZE, GLX_DOUBLEBUFFER};
use opengles::gl2::NO_ERROR;
use opengles::gl2;
use std::cast;
use std::libc::{c_int, c_uint, c_void};
use std::ptr;
use xlib::xlib::{Display, Pixmap, XCloseDisplay, XCreateGC, XCreateImage, XCreatePixmap};
use xlib::xlib::{XDefaultScreen, XFreePixmap, XGetGeometry, XOpenDisplay, XPutImage, XRootWindow};
use xlib::xlib::{XVisualInfo, ZPixmap};

/// The display and visual info. This is needed in order to upload on the painting side. This holds
/// a *strong* reference to the display and will close it when done.
///
/// FIXME(pcwalton): Mark nonsendable and noncopyable.
pub struct NativePaintingGraphicsContext {
    display: *Display,
    visual_info: *XVisualInfo,
}

impl NativePaintingGraphicsContext {
    #[fixed_stack_segment]
    pub fn from_metadata(metadata: &NativeGraphicsMetadata) -> NativePaintingGraphicsContext {
        unsafe {
            let display = do metadata.display.with_c_str |c_str| {
                XOpenDisplay(c_str)
            };

            if display.is_null() {
                fail!("XOpenDisplay() failed!");
            }

            // FIXME(pcwalton): It would be more robust to actually have the compositor pass the
            // visual.
            let (compositor_visual_info, _) =
                NativeCompositingGraphicsContext::compositor_visual_info(display);

            NativePaintingGraphicsContext {
                display: display,
                visual_info: compositor_visual_info,
            }
        }
    }
}

impl Drop for NativePaintingGraphicsContext {
    #[fixed_stack_segment]
    fn drop(&mut self) {
        unsafe {
            let _ = XCloseDisplay(self.display);
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
    major: int,
    minor: int,
}

impl NativeCompositingGraphicsContext {
    /// Chooses the compositor visual info using the same algorithm that the compositor uses.
    ///
    /// FIXME(pcwalton): It would be more robust to actually have the compositor pass the visual.
    #[fixed_stack_segment]
    fn compositor_visual_info(display: *Display) -> (*XVisualInfo, Option<GLXFBConfig>) {
        unsafe {
            let glx_display = cast::transmute(display);
            let (major, minor) = get_version(glx_display);

            if (major >= 1 && minor >= 3) {
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
                if (configs == 0) {
                    fail!("Unable to locate a GLX FB configuration that supports RGBA.");
                }
                
                let fbconfig = *ptr::offset(fbconfigs, 0);
                let vi = glXGetVisualFromFBConfig(glx_display, fbconfig);
                (cast::transmute(vi), Some(fbconfig))
            } else {
                let screen = XDefaultScreen(display);
                let attributes = [ GLX_RGBA, GLX_DEPTH_SIZE, 24, 0 ];
                let vi = glXChooseVisual(glx_display, screen, &attributes[0]);
                (cast::transmute(vi), None)
            }
        }
    }

    /// Creates a native graphics context from the given X display connection. This uses GLX. Only
    /// the compositor is allowed to call this.
    #[fixed_stack_segment]
    pub fn from_display(display: *Display) -> NativeCompositingGraphicsContext {
        unsafe {

            let (visual_info, fbconfig) = NativeCompositingGraphicsContext::compositor_visual_info(display);
            let glx_display = cast::transmute(display);
            let mut major = 0;
            let mut minor = 0;
            glXQueryVersion(glx_display, &mut major, &mut minor);
        
            NativeCompositingGraphicsContext {
                display: display,
                visual_info: visual_info,
                framebuffer_configuration: fbconfig,
                major: major as int,
                minor: minor as int,
            }
        }
    }
}

/// The X display string.
#[deriving(Clone)]
pub struct NativeGraphicsMetadata {
    display: ~str,
}

impl NativeGraphicsMetadata {
    /// Creates graphics metadata from a metadata descriptor.
    #[fixed_stack_segment]
    pub fn from_descriptor(descriptor: &NativeGraphicsMetadataDescriptor)
                           -> NativeGraphicsMetadata {
        NativeGraphicsMetadata {
            display: descriptor.display.to_str(),
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
    #[fixed_stack_segment]
    pub fn from_metadata(metadata: NativeGraphicsMetadata) -> NativeGraphicsMetadataDescriptor {
        NativeGraphicsMetadataDescriptor {
            display: metadata.display.to_str()
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
    #[fixed_stack_segment]
    pub fn from_pixmap(pixmap: Pixmap) -> NativeSurface {
        NativeSurface {
            pixmap: pixmap,
            will_leak: true,
        }
    }
}

impl NativeSurfaceMethods for NativeSurface {
    #[fixed_stack_segment]
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
    #[fixed_stack_segment]
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
            let (major, minor) = get_version(glx_display);
        
            let glx_pixmap = if (major == 1 && minor < 3) {
                    let glx_visual_info = cast::transmute(native_context.visual_info);
                    glXCreateGLXPixmap(glx_display,
                                       glx_visual_info,
                                       self.pixmap)
                } else {
                    glXCreatePixmap(glx_display,
                                    native_context.framebuffer_configuration.expect(
                                        "GLX 1.3 should have a framebuffer_configuration"),
                                    self.pixmap,
                                    &pixmap_attributes[0])
                };

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
            if (major == 1 && minor < 3) {
                glXDestroyGLXPixmap(glx_display, glx_pixmap);
            } else {
                glXDestroyPixmap(glx_display, glx_pixmap);
            }
        }
    }

    /// This may only be called on the painting side.
    #[fixed_stack_segment]
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

    #[fixed_stack_segment]
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


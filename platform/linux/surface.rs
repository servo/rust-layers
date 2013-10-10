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

use extra::arc::Arc;
use geom::size::Size2D;
use opengles::glx::{GLXFBConfig, GLXDrawable, GLXPixmap, GLX_BIND_TO_TEXTURE_RGBA_EXT};
use opengles::glx::{GLX_DEPTH_SIZE, GLX_DRAWABLE_TYPE, GLX_FRONT_EXT, GLX_PIXMAP_BIT, GLX_RGBA};
use opengles::glx::{GLX_TEXTURE_2D_EXT, GLX_TEXTURE_FORMAT_EXT, GLX_TEXTURE_FORMAT_RGBA_EXT};
use opengles::glx::{GLX_TEXTURE_TARGET_EXT, glXChooseVisual, glXCreatePixmap, glXDestroyPixmap};
use opengles::glx::{glXGetFBConfigs, glXGetProcAddress, glXGetFBConfigAttrib, glXGetFBConfigs};
use opengles::glx::{glXGetVisualFromFBConfig};
use opengles::glx;
use opengles::gl2::NO_ERROR;
use opengles::gl2;
use std::cast;
use std::libc::{c_char, c_int, c_uint, c_void};
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
            let display = XOpenDisplay(*metadata);

            // FIXME(pcwalton): It would be more robust to actually have the compositor pass the
            // visual.
            let compositor_visual_info =
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
    framebuffer_configuration: GLXFBConfig,
}

impl NativeCompositingGraphicsContext {
    /// Chooses the compositor visual info using the same algorithm that the compositor uses.
    ///
    /// FIXME(pcwalton): It would be more robust to actually have the compositor pass the visual.
    #[fixed_stack_segment]
    fn compositor_visual_info(display: *Display) -> *XVisualInfo {
        unsafe {
            let glx_display: *glx::Display = cast::transmute(display);
            let screen = XDefaultScreen(display);
            let attributes = [ GLX_RGBA, GLX_DEPTH_SIZE, 24, 0 ];
            let compositor_visual_info = glXChooseVisual(glx_display, screen, &attributes[0]);
            cast::transmute(compositor_visual_info)
        }
    }

    /// Creates a native graphics context from the given X display connection. This uses GLX. Only
    /// the compositor is allowed to call this.
    #[fixed_stack_segment]
    pub fn from_display(display: *Display) -> NativeCompositingGraphicsContext {
        unsafe {
            // FIXME(pcwalton): It would be more robust to actually have the compositor pass the
            // visual.
            let compositor_visual_info =
                NativeCompositingGraphicsContext::compositor_visual_info(display);
            let compositor_visual_id = (*compositor_visual_info).visualid;

            // Choose an FB config.
            let glx_display = cast::transmute(display);
            let screen = XDefaultScreen(display);
            let mut fbconfig_count = 0;
            let fbconfigs = glXGetFBConfigs(glx_display, screen, &mut fbconfig_count);
            let mut fbconfig_index = None;
            for i in range(0, fbconfig_count) {
                let fbconfig = *ptr::offset(fbconfigs, i as int);
                let visual_info = glXGetVisualFromFBConfig(glx_display, fbconfig);
                let visual_info: *XVisualInfo = cast::transmute(visual_info);
                if visual_info == ptr::null() || (*visual_info).visualid != compositor_visual_id {
                    continue
                }

                let mut value = 0;
                glXGetFBConfigAttrib(glx_display, fbconfig, GLX_DRAWABLE_TYPE, &mut value);
                if (value & GLX_PIXMAP_BIT) == 0 {
                    continue
                }

                glXGetFBConfigAttrib(glx_display,
                                     fbconfig,
                                     GLX_BIND_TO_TEXTURE_RGBA_EXT,
                                     &mut value);
                if value == 0 {
                    continue
                }

                fbconfig_index = Some(i);
                break
            }

            let framebuffer_configuration = match fbconfig_index {
                None => fail!("No appropriate framebuffer config found!"),
                Some(index) => *ptr::offset(fbconfigs, index as int),
            };

            NativeCompositingGraphicsContext {
                display: display,
                visual_info: compositor_visual_info,
                framebuffer_configuration: framebuffer_configuration,
            }
        }
    }
}

/// The X display string.
pub type NativeGraphicsMetadata = *c_char;

#[deriving(Eq)]
pub enum NativeSurfaceTransientData {
    NoTransientData,
    RenderTaskTransientData(*Display, *XVisualInfo),
}

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
                       size: Size2D<int>) {
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
                                             native_context.framebuffer_configuration,
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


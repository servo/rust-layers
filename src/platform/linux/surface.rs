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

use euclid::size::Size2D;
use libc::{c_int, c_uint, c_void};
use glx;
use skia::gl_context::{GLContext, PlatformDisplayData};
use skia::gl_rasterization_context::GLRasterizationContext;
use std::ascii::AsciiExt;
use std::ffi::CStr;
use std::mem;
use std::ptr;
use std::str;
use std::sync::Arc;
use x11::xlib;

use egl::egl::{EGLDisplay};

/// The display, visual info, and framebuffer configuration. This is needed in order to bind to a
/// texture on the compositor side. This holds only a *weak* reference to the display and does not
/// close it.
///
/// FIXME(pcwalton): Unchecked weak references are bad and can violate memory safety. This is hard
/// to fix because the Display is given to us by the native windowing system, but we should fix it
/// someday.
/// FIXME(pcwalton): Mark nonsendable.

#[derive(Copy, Clone)]
 struct GlxDisplayInfo {
    pub display: *mut xlib::Display,
    visual_info: *mut xlib::XVisualInfo,
    framebuffer_configuration: Option<glx::types::GLXFBConfig>,
}
#[derive(Copy, Clone)]
 struct EglDisplayInfo {
    pub display: EGLDisplay,
}

#[derive(Copy, Clone)]
pub enum NativeDisplay {
    Egl(EglDisplayInfo),
    Glx(GlxDisplayInfo),
}



unsafe impl Send for NativeDisplay {}

impl NativeDisplay {
    pub fn new(display: *mut xlib::Display) -> NativeDisplay {
        // FIXME(pcwalton): It would be more robust to actually have the compositor pass the
        // visual.
        let (compositor_visual_info, frambuffer_configuration) =
            NativeDisplay::compositor_visual_info(display);

        NativeDisplay {
            display: display,
            visual_info: compositor_visual_info,
            framebuffer_configuration: frambuffer_configuration,
        }
    }

    /// Chooses the compositor visual info using the same algorithm that the compositor uses.
    ///
    /// FIXME(pcwalton): It would be more robust to actually have the compositor pass the visual.
    fn compositor_visual_info(display: *mut xlib::Display)
                              -> (*mut xlib::XVisualInfo, Option<glx::types::GLXFBConfig>) {
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

            let screen = xlib::XDefaultScreen(display);
            let mut number_of_configs = 0;
            let configs = glx::ChooseFBConfig(mem::transmute(display),
                                              screen,
                                              fbconfig_attributes.as_ptr(),
                                              &mut number_of_configs);
            NativeDisplay::get_compatible_configuration(display, configs, number_of_configs)
        }
    }

    fn get_compatible_configuration(display: *mut xlib::Display,
                                    configs: *mut glx::types::GLXFBConfig,
                                    number_of_configs: i32)
                                    -> (*mut xlib::XVisualInfo, Option<glx::types::GLXFBConfig>) {
        unsafe {
            if number_of_configs == 0 {
                panic!("glx::ChooseFBConfig returned no configurations.");
            }

            if !NativeDisplay::need_to_find_32_bit_depth_visual(display) {
                let config = *configs.offset(0);
                let visual = glx::GetVisualFromFBConfig(mem::transmute(display), config);

                xlib::XFree(configs as *mut _);
                return (mem::transmute(visual), Some(config));
            }

            // NVidia (and AMD/ATI) drivers have RGBA configurations that use 24-bit
            // XVisual, not capable of representing an alpha-channel in Pixmap form,
            // so we look for the configuration with a full set of 32 bits.
            for i in 0..number_of_configs as isize {
                let config = *configs.offset(i);
                let visual: *mut xlib::XVisualInfo =
                    mem::transmute(glx::GetVisualFromFBConfig(mem::transmute(display), config));
                if (*visual).depth == 32 {
                    xlib::XFree(configs as *mut _);
                    return (visual, Some(config));
                }
                xlib::XFree(visual as *mut _);
            }

            xlib::XFree(configs as *mut _);
            panic!("Could not find 32-bit visual.");
        }
    }

    fn need_to_find_32_bit_depth_visual(display: *mut xlib::Display) -> bool {
        unsafe {
            let glx_vendor = glx::GetClientString(mem::transmute(display), glx::VENDOR as i32);
            if glx_vendor == ptr::null() {
                panic!("Could not determine GLX vendor.");
            }
            let glx_vendor =
                str::from_utf8(CStr::from_ptr(glx_vendor).to_bytes())
                    .ok()
                    .expect("GLX client vendor string not in UTF-8 format.")
                    .to_string()
                    .to_ascii_lowercase();
            glx_vendor.contains("nvidia") || glx_vendor.contains("ati")
        }
    }

    pub fn platform_display_data(&self) -> PlatformDisplayData {
        PlatformDisplayData {
            display: self.display,
            visual_info: self.visual_info,
        }
    }
}

#[derive(RustcDecodable, RustcEncodable)]
pub struct PixmapNativeSurface {
    /// The pixmap.
    pixmap: xlib::Pixmap,

    /// Whether this pixmap will leak if the destructor runs. This is for debugging purposes.
    will_leak: bool,

    /// The size of this surface.
    pub size: Size2D<i32>,
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
    pub fn new(display: &GlxDisplayInfo, size: Size2D<i32>) -> PixmapNativeSurface {
        unsafe {
            // Create the pixmap.
            let screen = xlib::XDefaultScreen(display.display);
            let window = xlib::XRootWindow(display.display, screen);
            // The X server we use for testing on build machines always returns
            // visuals that report 24 bit depth. But creating a 32 bit pixmap does work, so
            // hard code the depth here.
            let pixmap = xlib::XCreatePixmap(display.display,
                                             window,
                                             size.width as c_uint,
                                             size.height as c_uint,
                                             32);
            PixmapNativeSurface {
                pixmap: pixmap,
                will_leak: true,
                size: size,
            }
        }
    }

    /// This may only be called on the compositor side.
    pub fn bind_to_texture(&self, display: &NativeDisplay, texture: &Texture) {
        // Create the GLX pixmap.
        //
        // FIXME(pcwalton): RAII for exception safety?
        unsafe {
            let display = match display {
                &NativeDisplay::Glx(info) => info,
                &NativeDisplay::Egl(_) => unreachable!(),
            };

            let pixmap_attributes = [
                glx::TEXTURE_TARGET_EXT as i32, glx::TEXTURE_2D_EXT as i32,
                glx::TEXTURE_FORMAT_EXT as i32, glx::TEXTURE_FORMAT_RGBA_EXT as i32,
                0
            ];

            let glx_display = mem::transmute(display.display);

            let glx_pixmap = glx::CreatePixmap(glx_display,
                                               display.framebuffer_configuration.expect(
                                                   "GLX 1.3 should have a framebuffer_configuration"),
                                               self.pixmap,
                                               pixmap_attributes.as_ptr());

            let glx_bind_tex_image: extern "C" fn(*mut xlib::Display, glx::types::GLXDrawable, c_int, *mut c_int) =
                mem::transmute(glx::GetProcAddress(mem::transmute(&"glXBindTexImageEXT\x00".as_bytes()[0])));
            assert!(glx_bind_tex_image as *mut c_void != ptr::null_mut());
            let _bound = texture.bind();
            glx_bind_tex_image(display.display,
                               mem::transmute(glx_pixmap),
                               glx::FRONT_EXT  as i32,
                               ptr::null_mut());

            // FIXME(pcwalton): Recycle these for speed?
            glx::DestroyPixmap(glx_display, glx_pixmap);
        }
    }

    /// This may only be called on the painting side.
    pub fn upload(&mut self, display: &NativeDisplay, data: &[u8]) {
        unsafe {
            let image = xlib::XCreateImage(display.display,
                                           (*display.visual_info).visual,
                                           32,
                                           xlib::ZPixmap,
                                           0,
                                           mem::transmute(&data[0]),
                                           self.size.width as c_uint,
                                           self.size.height as c_uint,
                                           32,
                                           0);

            let gc = xlib::XCreateGC(display.display, self.pixmap, 0, ptr::null_mut());
            let _ = xlib::XPutImage(display.display,
                                    self.pixmap,
                                    gc,
                                    image,
                                    0,
                                    0,
                                    0,
                                    0,
                                    self.size.width as c_uint,
                                    self.size.height as c_uint);
        }
    }

    pub fn get_id(&self) -> isize {
        self.pixmap as isize
    }

    pub fn destroy(&mut self, display: &NativeDisplay) {
        unsafe {
            assert!(self.pixmap != 0);
            xlib::XFreePixmap(display.display, self.pixmap);
            self.mark_wont_leak()
        }
    }

    pub fn mark_will_leak(&mut self) {
        self.will_leak = true;
    }

    pub fn mark_wont_leak(&mut self) {
        self.will_leak = false;
    }

    pub fn gl_rasterization_context(&mut self,
                                    gl_context: Arc<GLContext>)
                                    -> Option<GLRasterizationContext> {
        GLRasterizationContext::new(gl_context, self.pixmap, self.size)
    }
}

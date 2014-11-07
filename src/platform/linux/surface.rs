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

use platform::surface::NativeSurfaceMethods;
use texturegl::Texture;

use geom::size::Size2D;
use libc::{c_char, c_int, c_uint, c_void};
use glx;
use gleam::gl;
use std::c_str::CString;
use std::mem;
use std::ptr;
use xlib::{Display, Pixmap, XCreateGC, XCreateImage, XCreatePixmap, XDefaultScreen};
use xlib::{XDisplayString, XFreePixmap, XGetGeometry, XOpenDisplay, XPutImage, XRootWindow};
use xlib::{XVisualInfo, ZPixmap};

/// The display and visual info. This is needed in order to upload on the painting side. This
/// holds a weak reference to the display and will not close it when done.
///
/// FIXME(pcwalton): Mark nonsendable and noncopyable.
pub struct NativePaintingGraphicsContext {
    display: *mut Display,
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
            let glx_display = mem::transmute(display);

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
            let configs = glx::ChooseFBConfig(glx_display, screen,
                                            fbconfig_attributes.as_ptr(), &mut number_of_configs);
            let glXGetClientString: extern "C" fn(*mut Display, c_int) -> *const c_char =
                mem::transmute(glx::GetProcAddress(mem::transmute(&"glXGetClientString\x00".as_bytes()[0])));
            assert!(glXGetClientString as *mut c_void != ptr::null_mut());
            let glx_cli_vendor_c_str = CString::new(glx::GetClientString(glx_display, glx::VENDOR as i32), false);
            let glx_cli_vendor = match glx_cli_vendor_c_str.as_str() { Some(s) => s,
                                                                       None => panic!("Can't get glx client vendor.") };
            if glx_cli_vendor.to_ascii().eq_ignore_case("NVIDIA".to_ascii()) ||
               glx_cli_vendor.to_ascii().eq_ignore_case("ATI".to_ascii()) {
                // NVidia (and AMD/ATI) drivers have RGBA configurations that use 24-bit XVisual, not capable of
                // representing an alpha-channel in Pixmap form, so we look for the configuration
                // with a full set of 32 bits.
                for i in range(0, number_of_configs as int) {
                    let config = *configs.offset(i);
                    let visual_info : *mut XVisualInfo = mem::transmute(glx::GetVisualFromFBConfig(glx_display, config));
                    if (*visual_info).depth == 32 {
                        return (visual_info, Some(config))
                    }
                }
            } else if number_of_configs != 0 {
                let fbconfig = *configs.offset(0);
                let vi = glx::GetVisualFromFBConfig(glx_display, fbconfig);
                return (mem::transmute(vi), Some(fbconfig));
            }
            panic!("Unable to locate a GLX FB configuration that supports RGBA.");
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
#[deriving(Clone)]
pub struct NativeGraphicsMetadata {
    pub display: *mut Display,
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
            let display = XOpenDisplay(descriptor.display.to_c_str().as_mut_ptr());
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
#[deriving(Clone, Decodable, Encodable)]
pub struct NativeGraphicsMetadataDescriptor {
    display: String,
}

impl NativeGraphicsMetadataDescriptor {
    /// Creates a metadata descriptor from metadata.
    pub fn from_metadata(metadata: NativeGraphicsMetadata) -> NativeGraphicsMetadataDescriptor {
        unsafe {
            let c_str = CString::new(XDisplayString(metadata.display) as *const i8, false);
            NativeGraphicsMetadataDescriptor {
                display: c_str.as_str().unwrap().to_string(),
            }
        }
    }
}

#[deriving(PartialEq)]
pub enum NativeSurfaceTransientData {
    NoTransientData,
    RenderTaskTransientData(*mut Display, *mut XVisualInfo),
}

#[deriving(Decodable, Encodable)]
pub struct WindowNativeSurface {
    /// The pixmap.
    pixmap: Pixmap,

    /// Whether this pixmap will leak if the destructor runs. This is for debugging purposes.
    will_leak: bool,
}

#[deriving(Decodable, Encodable)]
pub struct HeadlessNativeSurface {
    bytes: Vec<u8>,
}

#[deriving(Decodable, Encodable)]
pub enum NativeSurface {
    Windowed(WindowNativeSurface),
    Headless(HeadlessNativeSurface),
}

impl Drop for NativeSurface {
    fn drop(&mut self) {
        match *self {
            Windowed(ns) => {
                if ns.will_leak {
                    panic!("You should have disposed of the pixmap properly with destroy()! This pixmap \
                           will leak!");
                }
            }
            Headless(_) => {}
        }
    }
}

impl NativeSurface {
    pub fn from_pixmap(pixmap: Pixmap) -> NativeSurface {
        Windowed(WindowNativeSurface {
            pixmap: pixmap,
            will_leak: true,
        })
    }
}

impl NativeSurfaceMethods for NativeSurface {
    fn new(native_context: &NativePaintingGraphicsContext, size: Size2D<i32>, _stride: i32)
           -> NativeSurface {
        if native_context.display == ptr::null_mut() {
            return Headless(HeadlessNativeSurface {
                bytes: vec!(),
            });
        }

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
            NativeSurface::from_pixmap(pixmap)
        }
    }

    /// This may only be called on the compositor side.
    fn bind_to_texture(&self,
                       native_context: &NativeCompositingGraphicsContext,
                       texture: &Texture,
                       size: Size2D<int>) {
        match *self {
            Windowed(ref ns) => {
                unsafe {
                    // Create the GLX pixmap.
                    //
                    // FIXME(pcwalton): RAII for exception safety?
                    let pixmap_attributes = [
                        glx::TEXTURE_TARGET_EXT as i32, glx::TEXTURE_2D_EXT as i32,
                        glx::TEXTURE_FORMAT_EXT as i32, glx::TEXTURE_FORMAT_RGBA_EXT as i32,
                        0
                    ];

                    let glx_display = mem::transmute(native_context.display);

                    let glx_pixmap = glx::CreatePixmap(glx_display,
                                                     native_context.framebuffer_configuration.expect(
                                                         "GLX 1.3 should have a framebuffer_configuration"),
                                                     ns.pixmap,
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
            Headless(ref ns) => {
                let _bound = texture.bind();
                gl::tex_image_2d(gl::TEXTURE_2D, 0, gl::RGBA as i32,
                                size.width as i32, size.height as i32, 0,
                                gl::BGRA, gl::UNSIGNED_BYTE, Some(ns.bytes.as_slice()));
            }
        }
    }

    /// This may only be called on the painting side.
    fn upload(&mut self, graphics_context: &NativePaintingGraphicsContext, data: &[u8]) {
        match *self {
            Windowed(ref ns) => {
                unsafe {
                    // Ensure that we're running on the render task. Take the display.
                    let pixmap = ns.pixmap;

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
            Headless(ref mut ns) => {
                ns.bytes.push_all(data);
            }
        }
    }

    fn get_id(&self) -> int {
        match *self {
            Windowed(ref ns) => ns.pixmap as int,
            Headless(_) => 0,
        }
    }

    fn destroy(&mut self, graphics_context: &NativePaintingGraphicsContext) {
        match *self {
            Windowed(ns) => {
                unsafe {
                    assert!(ns.pixmap != 0);
                    XFreePixmap(graphics_context.display, ns.pixmap);
                    self.mark_wont_leak()
                }
            }
            Headless(_) => {},
        }
    }

    fn mark_will_leak(&mut self) {
        match *self {
            Windowed(ref mut ns) => ns.will_leak = true,
            Headless(_) => {}
        }
    }

    fn mark_wont_leak(&mut self) {
        match *self {
            Windowed(ref mut ns) => ns.will_leak = false,
            Headless(_) => {}
        }
    }
}


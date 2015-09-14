// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Mac OS-specific implementation of cross-process surfaces. This uses `IOSurface`, introduced
//! in Mac OS X 10.6 Snow Leopard.

use texturegl::Texture;

use cgl;
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use euclid::size::Size2D;
use io_surface;
use skia::gl_context::{GLContext, PlatformDisplayData};
use skia::gl_rasterization_context::GLRasterizationContext;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

thread_local!(static IO_SURFACE_REPOSITORY: Rc<RefCell<HashMap<io_surface::IOSurfaceID, io_surface::IOSurface>>> =
    Rc::new(RefCell::new(HashMap::new())));

#[derive(Clone, Copy)]
pub struct NativeDisplay {
    pub pixel_format: cgl::CGLPixelFormatObj,
}
unsafe impl Send for NativeDisplay {}

impl NativeDisplay {
    pub fn new() -> NativeDisplay {
        unsafe {
            NativeDisplay {
                pixel_format: cgl::CGLGetPixelFormat(cgl::CGLGetCurrentContext()),
            }
        }
    }

    pub fn platform_display_data(&self) -> PlatformDisplayData {
        PlatformDisplayData {
            pixel_format: self.pixel_format,
        }
    }
}

#[derive(Clone, RustcDecodable, RustcEncodable)]
pub struct IOSurfaceNativeSurface {
    io_surface_id: Option<io_surface::IOSurfaceID>,
    will_leak: bool,
    pub size: Size2D<i32>,
}

impl IOSurfaceNativeSurface {
    pub fn new(_: &NativeDisplay, size: Size2D<i32>) -> IOSurfaceNativeSurface {
        unsafe {
            let width_key: CFString = TCFType::wrap_under_get_rule(io_surface::kIOSurfaceWidth);
            let width_value: CFNumber = CFNumber::from_i32(size.width);

            let height_key: CFString = TCFType::wrap_under_get_rule(io_surface::kIOSurfaceHeight);
            let height_value: CFNumber = CFNumber::from_i32(size.height);

            let bytes_per_row_key: CFString =
                TCFType::wrap_under_get_rule(io_surface::kIOSurfaceBytesPerRow);
            let bytes_per_row_value: CFNumber = CFNumber::from_i32(size.width * 4);

            let bytes_per_elem_key: CFString =
                TCFType::wrap_under_get_rule(io_surface::kIOSurfaceBytesPerElement);
            let bytes_per_elem_value: CFNumber = CFNumber::from_i32(4);

            let is_global_key: CFString =
                TCFType::wrap_under_get_rule(io_surface::kIOSurfaceIsGlobal);
            let is_global_value = CFBoolean::true_value();

            let surface = io_surface::new(&CFDictionary::from_CFType_pairs(&[
                (width_key.as_CFType(), width_value.as_CFType()),
                (height_key.as_CFType(), height_value.as_CFType()),
                (bytes_per_row_key.as_CFType(), bytes_per_row_value.as_CFType()),
                (bytes_per_elem_key.as_CFType(), bytes_per_elem_value.as_CFType()),
                (is_global_key.as_CFType(), is_global_value.as_CFType()),
            ]));

            // Take the surface by ID (so that we can send it cross-process) and consume its reference.
            let id = surface.get_id();
            IO_SURFACE_REPOSITORY.with(|ref r| {
                r.borrow_mut().insert(id, surface)
            });

            IOSurfaceNativeSurface {
                io_surface_id: Some(id),
                will_leak: true,
                size: size,
            }
        }
    }

    pub fn bind_to_texture(&self, _: &NativeDisplay, texture: &Texture) {
        let _bound_texture = texture.bind();
        let io_surface = io_surface::lookup(self.io_surface_id.unwrap());
        io_surface.bind_to_gl_texture(self.size);
    }

    pub fn upload(&mut self, _: &NativeDisplay, data: &[u8]) {
        let io_surface = io_surface::lookup(self.io_surface_id.unwrap());
        io_surface.upload(data)
    }

    pub fn get_id(&self) -> isize {
        match self.io_surface_id {
            None => 0,
            Some(id) => id as isize,
        }
    }

    pub fn destroy(&mut self, _: &NativeDisplay) {
        IO_SURFACE_REPOSITORY.with(|ref r| {
            r.borrow_mut().remove(&self.io_surface_id.unwrap())
        });
        self.io_surface_id = None;
        self.mark_wont_leak()
    }

    pub fn mark_will_leak(&mut self) {
        self.will_leak = true
    }

    pub fn mark_wont_leak(&mut self) {
        self.will_leak = false
    }

    pub fn gl_rasterization_context(&mut self,
                                    gl_context: Arc<GLContext>)
                                    -> Option<GLRasterizationContext> {
        GLRasterizationContext::new(gl_context,
                                    io_surface::lookup(self.io_surface_id.unwrap()).obj,
                                    self.size)
    }
}

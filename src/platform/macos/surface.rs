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
use rustc_serialize::{Decoder, Decodable, Encoder, Encodable};
use skia::gl_context::{GLContext, PlatformDisplayData};
use skia::gl_rasterization_context::GLRasterizationContext;
use std::sync::Arc;

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

pub struct IOSurfaceNativeSurface {
    surface: Option<io_surface::IOSurface>,
    will_leak: bool,
    pub size: Size2D<i32>,
}

unsafe impl Send for IOSurfaceNativeSurface {}
unsafe impl Sync for IOSurfaceNativeSurface {}

impl Decodable for IOSurfaceNativeSurface {
    fn decode<D: Decoder>(d: &mut D) -> Result<Self, D::Error> {
        let id: Option<io_surface::IOSurfaceID> = try!(Decodable::decode(d));
        Ok(IOSurfaceNativeSurface {
            surface: id.map(io_surface::lookup),
            will_leak: try!(Decodable::decode(d)),
            size: try!(Decodable::decode(d)),
        })
    }
}
impl Encodable for IOSurfaceNativeSurface {
    fn encode<E: Encoder>(&self, e: &mut E) -> Result<(), E::Error> {
        try!(self.surface.as_ref().map(io_surface::IOSurface::get_id).encode(e));
        try!(self.will_leak.encode(e));
        try!(self.size.encode(e));
        Ok(())
    }
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

            IOSurfaceNativeSurface {
                surface: Some(surface),
                will_leak: true,
                size: size,
            }
        }
    }

    pub fn bind_to_texture(&self, _: &NativeDisplay, texture: &Texture) {
        let _bound_texture = texture.bind();
        let io_surface = self.surface.as_ref().unwrap();
        io_surface.bind_to_gl_texture(self.size);
    }

    pub fn upload(&mut self, _: &NativeDisplay, data: &[u8]) {
        let io_surface = self.surface.as_ref().unwrap();
        io_surface.upload(data)
    }

    pub fn get_id(&self) -> isize {
        match self.surface {
            None => 0,
            Some(ref io_surface) => io_surface.get_id() as isize,
        }
    }

    pub fn destroy(&mut self, _: &NativeDisplay) {
        self.surface = None;
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
                                    self.surface.as_ref().unwrap().obj,
                                    self.size)
    }
}

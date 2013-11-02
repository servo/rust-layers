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

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use geom::size::Size2D;
use io_surface::{kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow, kIOSurfaceHeight};
use io_surface::{kIOSurfaceIsGlobal, kIOSurfaceWidth, IOSurface};
use io_surface;
use opengles::cgl::CGLPixelFormatObj;
use platform::surface::NativeSurfaceMethods;
use std::util;
use texturegl::Texture;

pub struct NativeGraphicsMetadata {
    pixel_format: CGLPixelFormatObj,
}

pub struct NativePaintingGraphicsContext {
    metadata: NativeGraphicsMetadata,
}

impl NativePaintingGraphicsContext {
    pub fn from_metadata(metadata: &NativeGraphicsMetadata) -> NativePaintingGraphicsContext {
        NativePaintingGraphicsContext {
            metadata: *metadata,
        }
    }
}

impl Drop for NativePaintingGraphicsContext {
    fn drop(&mut self) {}
}

pub struct NativeCompositingGraphicsContext {
    contents: (),
}

impl NativeCompositingGraphicsContext {
    pub fn new() -> NativeCompositingGraphicsContext {
        NativeCompositingGraphicsContext {
            contents: (),
        }
    }
}

pub struct NativeSurface {
    io_surface: Option<IOSurface>,
    will_leak: bool,
}

impl NativeSurface {
    #[fixed_stack_segment]
    pub fn from_io_surface(io_surface: IOSurface) -> NativeSurface {
        NativeSurface {
            io_surface: Some(io_surface),
            will_leak: true,
        }
    }
}

impl NativeSurfaceMethods for NativeSurface {
    fn new(_: &NativePaintingGraphicsContext, size: Size2D<i32>, stride: i32) -> NativeSurface {
        unsafe {
            let width_key: CFString = TCFType::wrap_under_get_rule(kIOSurfaceWidth);
            let width_value: CFNumber = FromPrimitive::from_i32(size.width).unwrap();

            let height_key: CFString = TCFType::wrap_under_get_rule(kIOSurfaceHeight);
            let height_value: CFNumber = FromPrimitive::from_i32(size.height).unwrap();

            let bytes_per_row_key: CFString = TCFType::wrap_under_get_rule(kIOSurfaceBytesPerRow);
            let bytes_per_row_value: CFNumber = FromPrimitive::from_i32(stride).unwrap();

            let bytes_per_elem_key: CFString =
                TCFType::wrap_under_get_rule(kIOSurfaceBytesPerElement);
            let bytes_per_elem_value: CFNumber = FromPrimitive::from_i32(4).unwrap();

            let is_global_key: CFString = TCFType::wrap_under_get_rule(kIOSurfaceIsGlobal);
            let is_global_value = CFBoolean::true_value();

            let surface = io_surface::new(&CFDictionary::from_CFType_pairs([
                (width_key.as_CFType(), width_value.as_CFType()),
                (height_key.as_CFType(), height_value.as_CFType()),
                (bytes_per_row_key.as_CFType(), bytes_per_row_value.as_CFType()),
                (bytes_per_elem_key.as_CFType(), bytes_per_elem_value.as_CFType()),
                (is_global_key.as_CFType(), is_global_value.as_CFType()),
            ]));

            NativeSurface {
                io_surface: Some(surface),
                will_leak: true,
            }
        }
    }

    fn bind_to_texture(&self,
                       _: &NativeCompositingGraphicsContext,
                       texture: &Texture,
                       size: Size2D<int>) {
        let _bound_texture = texture.bind();
        self.io_surface.get_ref().bind_to_gl_texture(size)
    }

    fn upload(&self, _: &NativePaintingGraphicsContext, data: &[u8]) {
        self.io_surface.get_ref().upload(data)
    }

    fn get_id(&self) -> int {
        match self.io_surface {
            None => 0,
            Some(ref io_surface) => io_surface.get_id() as int,
        }
    }

    fn destroy(&mut self, _: &NativePaintingGraphicsContext) {
        let _ = util::replace(&mut self.io_surface, None);
        self.mark_wont_leak()
    }

    fn mark_will_leak(&mut self) {
        self.will_leak = true
    }

    fn mark_wont_leak(&mut self) {
        self.will_leak = false
    }
}


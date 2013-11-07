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
use io_surface::{kIOSurfaceIsGlobal, kIOSurfaceWidth, IOSurface, IOSurfaceID};
use io_surface;
use opengles::cgl::CGLPixelFormatObj;
use platform::surface::NativeSurfaceMethods;
use std::cast;
use std::cell::Cell;
use std::hashmap::HashMap;
use std::local_data;
use std::util;
use texturegl::Texture;

local_data_key!(io_surface_repository: HashMap<IOSurfaceID,IOSurface>)

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

#[deriving(Decodable, Encodable)]
pub struct NativeSurface {
    io_surface_id: Option<IOSurfaceID>,
    will_leak: bool,
}

impl NativeSurface {
    #[fixed_stack_segment]
    pub fn from_io_surface(io_surface: IOSurface) -> NativeSurface {
        unsafe {
            // Take the surface by ID (so that we can send it cross-process) and consume its
            // reference.
            let id = io_surface.get_id();

            let io_surface_cell = Cell::new(io_surface);
            local_data::modify(io_surface_repository, |opt_repository| {
                let mut repository = match opt_repository {
                    None => HashMap::new(),
                    Some(repository) => repository,
                };
                repository.insert(id, io_surface_cell.take());
                Some(repository)
            });

            NativeSurface {
                io_surface_id: Some(id),
                will_leak: true,
            }
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

            NativeSurface::from_io_surface(surface)
        }
    }

    fn bind_to_texture(&self,
                       _: &NativeCompositingGraphicsContext,
                       texture: &Texture,
                       size: Size2D<int>) {
        let _bound_texture = texture.bind();
        let io_surface = io_surface::lookup(self.io_surface_id.unwrap());
        io_surface.bind_to_gl_texture(size)
    }

    fn upload(&self, _: &NativePaintingGraphicsContext, data: &[u8]) {
        let io_surface = io_surface::lookup(self.io_surface_id.unwrap());
        io_surface.upload(data)
    }

    fn get_id(&self) -> int {
        match self.io_surface_id {
            None => 0,
            Some(id) => id as int,
        }
    }

    fn destroy(&mut self, _: &NativePaintingGraphicsContext) {
        local_data::get_mut(io_surface_repository, |opt_repository| {
            opt_repository.unwrap().remove(&self.io_surface_id.unwrap())
        });
        self.io_surface_id = None;
        self.mark_wont_leak()
    }

    fn mark_will_leak(&mut self) {
        self.will_leak = true
    }

    fn mark_wont_leak(&mut self) {
        self.will_leak = false
    }
}


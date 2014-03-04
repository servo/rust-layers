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
use serialize::{Decoder, Encodable, Encoder};
use geom::size::Size2D;
use io_surface::{kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow, kIOSurfaceHeight};
use io_surface::{kIOSurfaceIsGlobal, kIOSurfaceWidth, IOSurface, IOSurfaceID};
use io_surface;
use opengles::cgl::{CGLChoosePixelFormat, CGLDescribePixelFormat, CGLPixelFormatAttribute};
use opengles::cgl::{CGLPixelFormatObj, CORE_BOOLEAN_ATTRIBUTES, CORE_INTEGER_ATTRIBUTES};
use opengles::cgl::{kCGLNoError};
use opengles::gl2::GLint;
use collections::HashMap;
use std::local_data;
use std::ptr;

use platform::surface::NativeSurfaceMethods;
use texturegl::Texture;

local_data_key!(io_surface_repository: HashMap<IOSurfaceID,IOSurface>)

/// The Mac native graphics metadata.
#[deriving(Clone)]
pub struct NativeGraphicsMetadata {
    pixel_format: CGLPixelFormatObj,
}

impl NativeGraphicsMetadata {
    /// Creates a native graphics metadatum from a CGL pixel format.
    pub fn from_cgl_pixel_format(pixel_format: CGLPixelFormatObj) -> NativeGraphicsMetadata {
        NativeGraphicsMetadata {
            pixel_format: pixel_format,
        }
    }

    pub fn from_descriptor(descriptor: &NativeGraphicsMetadataDescriptor)
                           -> NativeGraphicsMetadata {
        unsafe {
            let mut attributes = ~[];
            for (i, &set) in descriptor.boolean_attributes.iter().enumerate() {
                if set {
                    attributes.push(CORE_BOOLEAN_ATTRIBUTES[i]);
                }
            }
            for (i, &value) in descriptor.integer_attributes.iter().enumerate() {
                attributes.push(CORE_INTEGER_ATTRIBUTES[i]);
                attributes.push(value as CGLPixelFormatAttribute);
            }
            attributes.push(0);
            let mut pixel_format = ptr::null();
            let mut count = 0;
            assert!(CGLChoosePixelFormat(&attributes[0], &mut pixel_format, &mut count) ==
                    kCGLNoError);
            assert!(pixel_format != ptr::null());
            assert!(count > 0);

            NativeGraphicsMetadata {
                pixel_format: pixel_format,
            }
        }
    }
}

/// The Mac native graphics metadata descriptor, which encompasses the values needed to create a
/// pixel format object.
#[deriving(Clone, Decodable, Encodable)]
pub struct NativeGraphicsMetadataDescriptor {
    boolean_attributes: ~[bool],
    integer_attributes: ~[GLint],
}

impl NativeGraphicsMetadataDescriptor {
    pub fn from_metadata(metadata: NativeGraphicsMetadata) -> NativeGraphicsMetadataDescriptor {
        unsafe {
            let mut descriptor = NativeGraphicsMetadataDescriptor {
                boolean_attributes: ~[],
                integer_attributes: ~[],
            };
            for &attribute in CORE_BOOLEAN_ATTRIBUTES.iter() {
                let mut value = 0;
                assert!(CGLDescribePixelFormat(metadata.pixel_format, 0, attribute, &mut value) ==
                        kCGLNoError);
                descriptor.boolean_attributes.push(value != 0);
                println!("{}: bool = {}", attribute, value);
            }
            for &attribute in CORE_INTEGER_ATTRIBUTES.iter() {
                let mut value = 0;
                assert!(CGLDescribePixelFormat(metadata.pixel_format, 0, attribute, &mut value) ==
                        kCGLNoError);
                descriptor.integer_attributes.push(value);
                println!("{}: int = {}", attribute, value);
            }
            descriptor
        }
    }

}

pub struct NativePaintingGraphicsContext {
    metadata: NativeGraphicsMetadata,
}

impl NativePaintingGraphicsContext {
    pub fn from_metadata(metadata: &NativeGraphicsMetadata) -> NativePaintingGraphicsContext {
        NativePaintingGraphicsContext {
            metadata: (*metadata).clone(),
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
    pub fn from_io_surface(io_surface: IOSurface) -> NativeSurface {
        // Take the surface by ID (so that we can send it cross-process) and consume its reference.
        let id = io_surface.get_id();

        let mut io_surface = Some(io_surface);
        local_data::modify(io_surface_repository, |opt_repository| {
            let mut repository = match opt_repository {
                None => HashMap::new(),
                Some(repository) => repository,
            };
            repository.insert(id, io_surface.take().unwrap());
            Some(repository)
        });

        NativeSurface {
            io_surface_id: Some(id),
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


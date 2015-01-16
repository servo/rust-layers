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

use azure::AzSkiaGrGLSharedSurfaceRef;
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use geom::size::Size2D;
use io_surface::{kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow, kIOSurfaceHeight};
use io_surface::{kIOSurfaceIsGlobal, kIOSurfaceWidth, IOSurface, IOSurfaceID};
use io_surface;
use cgl::{CGLChoosePixelFormat, CGLDescribePixelFormat, CGLPixelFormatAttribute};
use cgl::{CGLPixelFormatObj, CORE_BOOLEAN_ATTRIBUTES, CORE_INTEGER_ATTRIBUTES};
use cgl::{kCGLNoError};
use gleam::gl::GLint;
use std::cell::RefCell;
use std::collections::HashMap;
use std::mem;
use std::num::FromPrimitive;
use std::ptr;
use std::rc::Rc;
use std::vec::Vec;

thread_local!(static io_surface_repository: Rc<RefCell<HashMap<IOSurfaceID,IOSurface>>> = Rc::new(RefCell::new(HashMap::new())));

/// The Mac native graphics metadata.
#[derive(Clone, Copy)]
pub struct NativeGraphicsMetadata {
    pub pixel_format: CGLPixelFormatObj,
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
            let mut attributes = Vec::new();
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
            let mut pixel_format = ptr::null_mut();
            let mut count = 0;
            assert!(CGLChoosePixelFormat(attributes.as_ptr(), &mut pixel_format, &mut count) ==
                    kCGLNoError);
            assert!(pixel_format != ptr::null_mut());
            assert!(count > 0);

            NativeGraphicsMetadata {
                pixel_format: pixel_format,
            }
        }
    }
}

/// The Mac native graphics metadata descriptor, which encompasses the values needed to create a
/// pixel format object.
#[derive(Clone, RustcDecodable, RustcEncodable)]
pub struct NativeGraphicsMetadataDescriptor {
    boolean_attributes: Vec<bool>,
    integer_attributes: Vec<GLint>,
}

impl NativeGraphicsMetadataDescriptor {
    pub fn from_metadata(metadata: NativeGraphicsMetadata) -> NativeGraphicsMetadataDescriptor {
        unsafe {
            let mut descriptor = NativeGraphicsMetadataDescriptor {
                boolean_attributes: Vec::new(),
                integer_attributes: Vec::new(),
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
    _metadata: NativeGraphicsMetadata,
}

impl NativePaintingGraphicsContext {
    pub fn from_metadata(metadata: &NativeGraphicsMetadata) -> NativePaintingGraphicsContext {
        NativePaintingGraphicsContext {
            _metadata: (*metadata).clone(),
        }
    }
}

impl Drop for NativePaintingGraphicsContext {
    fn drop(&mut self) {}
}

#[derive(Copy)]
pub struct NativeCompositingGraphicsContext {
    _contents: (),
}

impl NativeCompositingGraphicsContext {
    pub fn new() -> NativeCompositingGraphicsContext {
        NativeCompositingGraphicsContext {
            _contents: (),
        }
    }
}

#[derive(RustcDecodable, RustcEncodable)]
pub struct IOSurfaceNativeSurface {
    io_surface_id: Option<IOSurfaceID>,
    will_leak: bool,
}

impl IOSurfaceNativeSurface {
    pub fn from_io_surface(io_surface: IOSurface) -> IOSurfaceNativeSurface {
        // Take the surface by ID (so that we can send it cross-process) and consume its reference.
        let id = io_surface.get_id();

        let mut io_surface = Some(io_surface);

        io_surface_repository.with(|ref r| {
            r.borrow_mut().insert(id, io_surface.take().unwrap())
        });

        IOSurfaceNativeSurface {
            io_surface_id: Some(id),
            will_leak: true,
        }
    }

    pub fn from_azure_surface(surface: AzSkiaGrGLSharedSurfaceRef) -> IOSurfaceNativeSurface {
        unsafe {
            let io_surface = IOSurface {
                obj: mem::transmute(surface),
            };
            IOSurfaceNativeSurface::from_io_surface(io_surface)
        }
    }

    pub fn new(_: &NativePaintingGraphicsContext,
               size: Size2D<i32>,
               stride: i32)
               -> IOSurfaceNativeSurface {
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

            let surface = io_surface::new(&CFDictionary::from_CFType_pairs(&[
                (width_key.as_CFType(), width_value.as_CFType()),
                (height_key.as_CFType(), height_value.as_CFType()),
                (bytes_per_row_key.as_CFType(), bytes_per_row_value.as_CFType()),
                (bytes_per_elem_key.as_CFType(), bytes_per_elem_value.as_CFType()),
                (is_global_key.as_CFType(), is_global_value.as_CFType()),
            ]));

            IOSurfaceNativeSurface::from_io_surface(surface)
        }
    }

    pub fn bind_to_texture(&self,
                           _: &NativeCompositingGraphicsContext,
                           texture: &Texture,
                           size: Size2D<int>) {
        let _bound_texture = texture.bind();
        let io_surface = io_surface::lookup(self.io_surface_id.unwrap());
        io_surface.bind_to_gl_texture(size)
    }

    pub fn upload(&mut self, _: &NativePaintingGraphicsContext, data: &[u8]) {
        let io_surface = io_surface::lookup(self.io_surface_id.unwrap());
        io_surface.upload(data)
    }

    pub fn get_id(&self) -> int {
        match self.io_surface_id {
            None => 0,
            Some(id) => id as int,
        }
    }

    pub fn destroy(&mut self, _: &NativePaintingGraphicsContext) {
        io_surface_repository.with(|ref r| {
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
}

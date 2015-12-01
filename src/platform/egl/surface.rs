// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Implementation of cross-process surfaces implementing  EGL surface.

use texturegl::Texture;

use egl::egl::{EGLDisplay, GetCurrentDisplay};
use egl::eglext::{EGLImageKHR, DestroyImageKHR};
use euclid::size::Size2D;
//use gleam::gl::{egl_image_target_texture2d_oes, TEXTURE_2D}; //, TexImage2D, BGRA_EXT, UNSIGNED_BYTE};
use libc::c_void;
use skia::gl_context::{GLContext, PlatformDisplayData};
use skia::gl_rasterization_context::GLRasterizationContext;
use std::iter::repeat;
use std::mem;
use std::sync::Arc;
use std::vec::Vec;


#[derive(Clone, Copy)]
pub struct NativeDisplay {
    pub display: EGLDisplay,
}


pub struct EGLImageNativeSurface {
    /// An EGLImage for the case of GPU rendering.
    image: Option<EGLImageKHR>,

    /// A heap-allocated bitmap for the case of CPU rendering.
    bitmap: Option<Vec<u8>>,

    /// Whether this pixmap will leak if the destructor runs. This is for debugging purposes.
    will_leak: bool,

    /// The size of this surface.
    pub size: Size2D<i32>,
}

unsafe impl Send for EGLImageNativeSurface {}

impl EGLImageNativeSurface {
    pub fn new(_: &NativeDisplay, size: Size2D<i32>) -> EGLImageNativeSurface {
        let len = size.width * size.height * 4;
        let bitmap: Vec<u8> = repeat(0).take(len as usize).collect();

        EGLImageNativeSurface {
            image: None,
            bitmap: Some(bitmap),
            will_leak: true,
            size: size,
        }
    }

    /// This may only be called on the compositor side.
    pub fn bind_to_texture(&self, _: &NativeDisplay, texture: &Texture) {
        let _bound = texture.bind();
        match self.image {
            None => match self.bitmap {
                Some(ref bitmap) => {
                    let data = bitmap.as_ptr() as *const c_void;
                    unsafe {
                    
                    panic!("TO DO  TexImage2D  "); 
                    /*
                        TexImage2D(TEXTURE_2D,
                                   0,
                                   BGRA_EXT as i32,
                                   self.size.width as i32,
                                   self.size.height as i32,
                                   0,
                                   BGRA_EXT as u32,
                                   UNSIGNED_BYTE,
                                   data);
                    */
                     } 
                }
                None => {
                    debug!("Cannot bind the buffer(CPU rendering), there is no bitmap");
                }
            },
            Some(image_khr) => {
                panic!("TO DO  egl_image_target_texture2d_oes");
            	//egl_image_target_texture2d_oes(TEXTURE_2D, image_khr as *const c_void);
            }
        }
    }

    /// This may only be called on the painting side.
    pub fn upload(&mut self, _: &NativeDisplay, data: &[u8]) {
        match self.bitmap {
            Some(ref mut bitmap) => {
                bitmap.clear();
                bitmap.push_all(data);
            }
            None => {
                debug!("Cannot upload the buffer(CPU rendering), there is no bitmap");
            }
        }
    }

    pub fn get_id(&self) -> isize {
        match self.image {
            None => 0,
            Some(image_khr) => image_khr as isize,
        }
    }

    pub fn destroy(&mut self, graphics_context: &NativeDisplay) {
        match self.image {
            None => {},
            Some(image_khr) => {
                DestroyImageKHR(graphics_context.display, image_khr);
                mem::replace(&mut self.image, None);
            }
        }
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
        // TODO: Eventually we should preserve the previous GLRasterizationContext,
        // so that we don't have to keep destroying and recreating the image.
        if let Some(egl_image) = self.image.take() {
            panic!("TO DO, DestroyImageKHR typecast ");
        	//DestroyImageKHR(gl_context.platform_context.display, egl_image);
        }

        panic!("TO DO, num params in GLRasterizationContext.new() ");
       /* let gl_rasterization_context = GLRasterizationContext::new(gl_context, self.size);
        if let Some(ref gl_rasterization_context) = gl_rasterization_context {
            self.bitmap = None;
            self.image = Some(gl_rasterization_context.egl_image);
        }
        gl_rasterization_context
        */
    }
}

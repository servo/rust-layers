// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use layers::{ARGB32Format, ContainerLayerKind, Flip, NoFlip, RGB24Format, TextureLayerKind};
use layers::{VerticalFlip};
use layers;
use scene::Scene;
use texturegl::Texture;

use geom::matrix::{Matrix4, ortho};
use geom::point::Point2D;
use geom::size::Size2D;
use geom::rect::Rect;
use opengles::gl2::{ARRAY_BUFFER, COLOR_BUFFER_BIT, CLAMP_TO_EDGE, COMPILE_STATUS};
use opengles::gl2::{FRAGMENT_SHADER, LINK_STATUS, LINEAR, NO_ERROR, RGB, RGBA, BGRA, SCISSOR_BOX};
use opengles::gl2::{SCISSOR_TEST, STATIC_DRAW, TEXTURE_2D, TEXTURE_MAG_FILTER, TEXTURE_MIN_FILTER};
use opengles::gl2::{TEXTURE_WRAP_S, TEXTURE_WRAP_T, TEXTURE0};
use opengles::gl2::{TRIANGLE_STRIP, UNPACK_ALIGNMENT, UNPACK_CLIENT_STORAGE_APPLE};
use opengles::gl2::{UNPACK_ROW_LENGTH, UNSIGNED_BYTE, UNSIGNED_INT_8_8_8_8_REV, VERTEX_SHADER, VIEWPORT};
use opengles::gl2::{GLenum, GLint, GLsizei, GLuint, attach_shader, bind_buffer};
use opengles::gl2::{buffer_data, create_program, clear, clear_color};
use opengles::gl2::{compile_shader, create_shader, draw_arrays, disable, enable, is_enabled};
use opengles::gl2::{enable_vertex_attrib_array, gen_buffers, gen_textures};
use opengles::gl2::{get_attrib_location, get_error, get_integer_v, get_program_iv};
use opengles::gl2::{get_shader_info_log, get_shader_iv, active_texture};
use opengles::gl2::{get_uniform_location, link_program, pixel_store_i, scissor, shader_source};
use opengles::gl2::{tex_image_2d, tex_parameter_i, uniform_1i, uniform_matrix_4fv, use_program};
use opengles::gl2::{vertex_attrib_pointer_f32, viewport};

use std::libc::c_int;
use std::libc::c_uint;

pub fn FRAGMENT_SHADER_SOURCE() -> ~str {
    ~"
        #ifdef GL_ES
            precision mediump float;
        #endif

        varying vec2 vTextureCoord;

        uniform sampler2D uSampler;

        void main(void) {
            gl_FragColor = texture2D(uSampler, vTextureCoord);
        }
    "
}

pub fn VERTEX_SHADER_SOURCE() -> ~str {
    ~"
        attribute vec3 aVertexPosition;
        attribute vec2 aTextureCoord;

        uniform mat4 uMVMatrix;
        uniform mat4 uPMatrix;

        varying vec2 vTextureCoord;

        void main(void) {
            gl_Position = uPMatrix * uMVMatrix * vec4(aVertexPosition, 1.0);
            vTextureCoord = aTextureCoord;
        }
    "
}

pub fn load_shader(source_string: ~str, shader_type: GLenum) -> GLuint {
    let shader_id = create_shader(shader_type);
    shader_source(shader_id, [ source_string.as_bytes().to_owned() ]);
    compile_shader(shader_id);

    if get_error() != NO_ERROR {
        println(fmt!("error: %d", get_error() as int));
        fail!(~"failed to compile shader");
    }

    if get_shader_iv(shader_id, COMPILE_STATUS) == (0 as GLint) {
        println(fmt!("shader info log: %s", get_shader_info_log(shader_id)));
        fail!(~"failed to compile shader");
    }

    return shader_id;
}

pub struct RenderContext {
    program: GLuint,
    vertex_position_attr: c_int,
    texture_coord_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    sampler_uniform: c_int,
    vertex_buffer: GLuint,
    flipped_vertex_buffer: GLuint,
    texture_coord_buffer: GLuint,
}

pub fn RenderContext(program: GLuint) -> RenderContext {
    let (vertex_buffer, flipped_vertex_buffer, texture_coord_buffer) = init_buffers();
    let rc = RenderContext {
        program: program,
        vertex_position_attr: get_attrib_location(program, ~"aVertexPosition"),
        texture_coord_attr: get_attrib_location(program, ~"aTextureCoord"),
        modelview_uniform: get_uniform_location(program, ~"uMVMatrix"),
        projection_uniform: get_uniform_location(program, ~"uPMatrix"),
        sampler_uniform: get_uniform_location(program, ~"uSampler"),
        vertex_buffer: vertex_buffer,
        flipped_vertex_buffer: flipped_vertex_buffer,
        texture_coord_buffer: texture_coord_buffer,
    };

    enable_vertex_attrib_array(rc.vertex_position_attr as GLuint);
    enable_vertex_attrib_array(rc.texture_coord_attr as GLuint);

    rc
}

pub fn init_render_context() -> RenderContext {
    let vertex_shader = load_shader(VERTEX_SHADER_SOURCE(), VERTEX_SHADER);
    let fragment_shader = load_shader(FRAGMENT_SHADER_SOURCE(), FRAGMENT_SHADER);

    let program = create_program();
    attach_shader(program, vertex_shader);
    attach_shader(program, fragment_shader);
    link_program(program);

    if get_program_iv(program, LINK_STATUS) == (0 as GLint) {
        fail!(~"failed to initialize program");
    }

    use_program(program);
    enable(TEXTURE_2D);

    return RenderContext(program);
}

pub fn init_buffers() -> (GLuint, GLuint, GLuint) {
    let vertex_buffer = gen_buffers(1 as GLsizei)[0];
    bind_buffer(ARRAY_BUFFER, vertex_buffer);
    let vertices: [f32, ..8] = [
        0.0, 0.0,
        0.0, 1.0,
        1.0, 0.0,
        1.0, 1.0,
    ];
    buffer_data(ARRAY_BUFFER, vertices, STATIC_DRAW);

    let flipped_vertex_buffer = gen_buffers(1 as GLsizei)[0];
    bind_buffer(ARRAY_BUFFER, flipped_vertex_buffer);
    let flipped_vertices: [f32, ..8] = [
        0.0, 1.0,
        0.0, 0.0,
        1.0, 1.0,
        1.0, 0.0,
    ];
    buffer_data(ARRAY_BUFFER, flipped_vertices, STATIC_DRAW);

    let texture_coord_buffer = gen_buffers(1 as GLsizei)[0];
    bind_buffer(ARRAY_BUFFER, texture_coord_buffer);

    return (vertex_buffer, flipped_vertex_buffer, texture_coord_buffer);
}

pub fn bind_and_render_quad(render_context: RenderContext, texture: &Texture, flip: Flip) {
    active_texture(TEXTURE0);
    let _bound_texture = texture.bind();

    uniform_1i(render_context.sampler_uniform, 0);

    match flip {
        NoFlip => bind_buffer(ARRAY_BUFFER, render_context.vertex_buffer),
        VerticalFlip => bind_buffer(ARRAY_BUFFER, render_context.flipped_vertex_buffer),
    }

    vertex_attrib_pointer_f32(render_context.vertex_position_attr as GLuint, 3, false, 0, 0);

    // Create the texture coordinate array.
    bind_buffer(ARRAY_BUFFER, render_context.texture_coord_buffer);

    let vertices = [
        0.0f32, 1.0f32,
        0.0f32, 0.0f32,
        1.0f32, 1.0f32,
        1.0f32, 0.0f32,
    ];
    buffer_data(ARRAY_BUFFER, vertices, STATIC_DRAW);
    vertex_attrib_pointer_f32(render_context.texture_coord_attr as GLuint, 2, false, 0, 0);
    draw_arrays(TRIANGLE_STRIP, 0, 4);
}

// Layer rendering

pub trait Render {
    fn render(@mut self, render_context: RenderContext, transform: Matrix4<f32>);
}

impl Render for layers::ContainerLayer {
    fn render(@mut self, render_context: RenderContext, transform: Matrix4<f32>) {
        let old_rect_opt = if is_enabled(SCISSOR_TEST) {
            let mut old_rect = [0 as GLint, ..4];
            get_integer_v(SCISSOR_BOX, old_rect);
            Some(Rect(Point2D(old_rect[0], old_rect[1]), Size2D(old_rect[2], old_rect[3])))
        } else {
            None
        };

        match self.scissor {
            Some(rect) => {
                let size = Size2D((rect.size.width * transform.m11) as GLint,
                                  (rect.size.height * transform.m22) as GLint);
                
                // Get the viewport height so we can flip the origin horizontally
                // since glScissor measures from the bottom left of the viewport
                let mut viewport = [0 as GLint, ..4];
                get_integer_v(VIEWPORT, viewport);
                let w_height = viewport[3];
                let origin = Point2D((rect.origin.x * transform.m11 + transform.m41) as GLint,
                                     w_height
                                     - ((rect.origin.y * transform.m22 + transform.m42) as GLint)
                                     - size.height);                
                let rect = Rect(origin, size);
                
                match old_rect_opt {
                    Some(old_rect) => {
                        // A parent ContainerLayer is already being scissored, so set the
                        // new scissor to the intersection of the two rects.
                        let intersection = rect.intersection(&old_rect);
                        match intersection {
                            Some(new_rect) => {
                                scissor(new_rect.origin.x,
                                        new_rect.origin.y,
                                        new_rect.size.width as GLsizei,
                                        new_rect.size.height as GLsizei);
                                maybe_get_error();
                            }
                            None => {
                                return; // Layer is occluded/offscreen
                            }
                        }
                    }
                    None => {
                        // We are the first ContainerLayer to be scissored.
                        // Check against the viewport to prevent invalid values
                        let w_rect = Rect(Point2D(0 as GLint, 0 as GLint),
                                          Size2D(viewport[2], viewport[3]));
                        let intersection = rect.intersection(&w_rect);
                        match intersection {
                            Some(new_rect) => {
                                enable(SCISSOR_TEST);
                                scissor(new_rect.origin.x,
                                        new_rect.origin.y,
                                        new_rect.size.width as GLsizei,
                                        new_rect.size.height as GLsizei);
                                maybe_get_error();
                            }
                            None => {
                                return; // Layer is offscreen
                            }
                        }
                    }
                }
            }
            None => {} // Nothing to do
        }

        let transform = transform.mul(&self.common.transform);
        for child in self.children() {
            render_layer(render_context, transform, child);
        }
        
        match (self.scissor, old_rect_opt) {
            (Some(_), Some(old_rect)) => {
                // Set scissor back to the parent's scissoring rect.
                scissor(old_rect.origin.x, old_rect.origin.y, 
                        old_rect.size.width as GLsizei,
                        old_rect.size.height as GLsizei);
            }
            (Some(_), None) => {
                // Our parents are not being scissored, so disable scissoring for now
                disable(SCISSOR_TEST);
            }
            (None, _) => {} // Nothing to do
        }
    }
}

impl Render for layers::TextureLayer {
    fn render(@mut self, render_context: RenderContext, transform: Matrix4<f32>) {
        let transform = transform.mul(&self.common.transform);
        uniform_matrix_4fv(render_context.modelview_uniform, false, transform.to_array());
        bind_and_render_quad(render_context, self.texture.get(), self.flip);
    }
}

fn render_layer(render_context: RenderContext, transform: Matrix4<f32>, layer: layers::Layer) {
    match layer {
        ContainerLayerKind(container_layer) => {
            container_layer.render(render_context, transform);
        }
        TextureLayerKind(texture_layer) => {
            texture_layer.render(render_context, transform);
        }
    }
}

pub fn render_scene(render_context: RenderContext, scene: &Scene) {
    // Set the viewport.
    viewport(0 as GLint, 0 as GLint, scene.size.width as GLsizei, scene.size.height as GLsizei);

    // Clear the screen.
    clear_color(0.38f32, 0.36f32, 0.36f32, 1.0f32);
    clear(COLOR_BUFFER_BIT);

    // Set the projection matrix.
    let projection_matrix = ortho(0.0, scene.size.width, scene.size.height, 0.0, -10.0, 10.0);
    uniform_matrix_4fv(render_context.projection_uniform, false, projection_matrix.to_array());

    // Set up the initial modelview matrix.
    let transform = scene.transform;

    // Render the root layer.
    render_layer(render_context, transform, scene.root);
}

#[cfg(debug)]
fn maybe_get_error() {
    if get_error() != NO_ERROR {
        fail!("GL error: %d", get_error() as int);
    }
}

#[cfg(not(debug))]
fn maybe_get_error() {
    // do nothing
}

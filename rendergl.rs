// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use layers::{ContainerLayer, TextureLayer, Flip, NoFlip, VerticalFlip};
use layers;
use scene::Scene;
use texturegl::{Texture, TextureTarget2D, TextureTargetRectangle};

use geom::matrix::{Matrix4, ortho};
use geom::size::Size2D;
use libc::c_int;
use opengles::gl2::{ARRAY_BUFFER, BLEND, COLOR_BUFFER_BIT, COMPILE_STATUS, FRAGMENT_SHADER};
use opengles::gl2::{LINK_STATUS, NO_ERROR, ONE_MINUS_SRC_ALPHA};
use opengles::gl2::{SRC_ALPHA, STATIC_DRAW, TEXTURE_2D, TEXTURE0};
use opengles::gl2::{TRIANGLE_STRIP, VERTEX_SHADER, GLenum, GLfloat, GLint, GLsizei};
use opengles::gl2::{GLuint, active_texture, attach_shader, bind_buffer, bind_texture, blend_func};
use opengles::gl2::{buffer_data, create_program, clear, clear_color, compile_shader};
use opengles::gl2::{create_shader, draw_arrays, enable, enable_vertex_attrib_array};
use opengles::gl2::{gen_buffers, get_attrib_location, get_error, get_program_iv};
use opengles::gl2::{get_shader_info_log, get_shader_iv, get_uniform_location};
use opengles::gl2::{link_program, shader_source, uniform_1i, uniform_2f};
use opengles::gl2::{uniform_matrix_4fv, use_program, vertex_attrib_pointer_f32, viewport};
use std::rc::Rc;

static FRAGMENT_2D_SHADER_SOURCE: &'static str = "
    #ifdef GL_ES
        precision mediump float;
    #endif

    varying vec2 vTextureCoord;

    uniform sampler2D uSampler;

    void main(void) {
        gl_FragColor = texture2D(uSampler, vTextureCoord);
    }
";

static FRAGMENT_RECTANGLE_SHADER_SOURCE: &'static str = "
    #ifdef GL_ES
        precision mediump float;
    #endif

    varying vec2 vTextureCoord;

    uniform sampler2DRect uSampler;
    uniform vec2 uSize;

    void main(void) {
        gl_FragColor = texture2DRect(uSampler, vTextureCoord * uSize);
    }
";

static VERTEX_SHADER_SOURCE: &'static str = "
    attribute vec3 aVertexPosition;
    attribute vec2 aTextureCoord;

    uniform mat4 uMVMatrix;
    uniform mat4 uPMatrix;

    varying vec2 vTextureCoord;

    void main(void) {
        gl_Position = uPMatrix * uMVMatrix * vec4(aVertexPosition, 1.0);
        vTextureCoord = aTextureCoord;
    }
";

static VERTICES: [f32, ..12] = [
    0.0, 0.0, 0.0,
    0.0, 1.0, 0.0,
    1.0, 0.0, 0.0,
    1.0, 1.0, 0.0,
];

static TEXTURE_COORDINATES: [f32, ..8] = [
    0.0, 0.0,
    0.0, 1.0,
    1.0, 0.0,
    1.0, 1.0,
];

static FLIPPED_TEXTURE_COORDINATES: [f32, ..8] = [
    0.0, 1.0,
    0.0, 0.0,
    1.0, 1.0,
    1.0, 0.0,
];

pub fn load_shader(source_string: &str, shader_type: GLenum) -> GLuint {
    let shader_id = create_shader(shader_type);
    shader_source(shader_id, [ source_string.as_bytes() ]);
    compile_shader(shader_id);

    if get_error() != NO_ERROR {
        println!("error: {:d}", get_error() as int);
        fail!("failed to compile shader");
    }

    if get_shader_iv(shader_id, COMPILE_STATUS) == (0 as GLint) {
        println!("shader info log: {:s}", get_shader_info_log(shader_id));
        fail!("failed to compile shader");
    }

    return shader_id;
}

struct Buffers {
    vertex_buffer: GLuint,
    texture_coordinate_buffer: GLuint,
    flipped_texture_coordinate_buffer: GLuint,
}

struct Program2D {
    id: GLuint,
    vertex_position_attr: c_int,
    texture_coord_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    sampler_uniform: c_int,
}

struct ProgramRectangle {
    id: GLuint,
    vertex_position_attr: c_int,
    texture_coord_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    sampler_uniform: c_int,
    size_uniform: c_int,
}

pub struct RenderContext {
    program_2d: Option<Program2D>,
    program_rectangle: Option<ProgramRectangle>,
    buffers: Buffers,
}

impl RenderContext {
    fn new(program_2d: Option<GLuint>, program_rectangle: Option<GLuint>) -> RenderContext {
        let render_context = RenderContext {
            program_2d: match program_2d {
                Some(program) => {
                    Some(Program2D {
                        id: program,
                        vertex_position_attr: get_attrib_location(program, "aVertexPosition"),
                        texture_coord_attr: get_attrib_location(program, "aTextureCoord"),
                        modelview_uniform: get_uniform_location(program, "uMVMatrix"),
                        projection_uniform: get_uniform_location(program, "uPMatrix"),
                        sampler_uniform: get_uniform_location(program, "uSampler"),
                    })
                },
                None => None,
            },
            program_rectangle: match program_rectangle {
                Some(program) => {
                    Some(ProgramRectangle {
                        id: program,
                        vertex_position_attr: get_attrib_location(program, "aVertexPosition"),
                        texture_coord_attr: get_attrib_location(program, "aTextureCoord"),
                        modelview_uniform: get_uniform_location(program, "uMVMatrix"),
                        projection_uniform: get_uniform_location(program, "uPMatrix"),
                        sampler_uniform: get_uniform_location(program, "uSampler"),
                        size_uniform: get_uniform_location(program, "uSize"),
                    })
                },
                None => None,
            },
            buffers: RenderContext::init_buffers(),
        };

        match render_context.program_2d {
            Some(program) => {
                enable_vertex_attrib_array(program.vertex_position_attr as GLuint);
                enable_vertex_attrib_array(program.texture_coord_attr as GLuint);
            },
            None => {}
        }

        match render_context.program_rectangle {
            Some(program) => {
                enable_vertex_attrib_array(program.vertex_position_attr as GLuint);
                enable_vertex_attrib_array(program.texture_coord_attr as GLuint);
            },
            None=> {}
        }

        render_context
    }

    fn init_buffers() -> Buffers {
        let vertex_buffer = *gen_buffers(1).get(0);
        bind_buffer(ARRAY_BUFFER, vertex_buffer);
        buffer_data(ARRAY_BUFFER, VERTICES, STATIC_DRAW);

        let texture_coordinate_buffer = *gen_buffers(1).get(0);
        bind_buffer(ARRAY_BUFFER, texture_coordinate_buffer);
        buffer_data(ARRAY_BUFFER, TEXTURE_COORDINATES, STATIC_DRAW);

        let flipped_texture_coordinate_buffer = *gen_buffers(1).get(0);
        bind_buffer(ARRAY_BUFFER, flipped_texture_coordinate_buffer);
        buffer_data(ARRAY_BUFFER, FLIPPED_TEXTURE_COORDINATES, STATIC_DRAW);

        Buffers {
            vertex_buffer: vertex_buffer,
            texture_coordinate_buffer: texture_coordinate_buffer,
            flipped_texture_coordinate_buffer: flipped_texture_coordinate_buffer,
        }
    }
}

pub fn init_program(vertex_shader: GLuint, fragment_shader: GLuint) -> GLuint {
    let program = create_program();
    attach_shader(program, vertex_shader);
    attach_shader(program, fragment_shader);
    link_program(program);
    if get_program_iv(program, LINK_STATUS) == (0 as GLint) {
        fail!("failed to initialize program");
    }

    program
}

#[cfg(target_os="linux")]
#[cfg(target_os="macos")]
pub fn init_render_context() -> RenderContext {
    use opengles::gl2::TEXTURE_RECTANGLE_ARB;

    let vertex_2d_shader = load_shader(VERTEX_SHADER_SOURCE, VERTEX_SHADER);
    let fragment_2d_shader = load_shader(FRAGMENT_2D_SHADER_SOURCE, FRAGMENT_SHADER);
    let program_2d = init_program(vertex_2d_shader, fragment_2d_shader);

    let vertex_rectangle_shader = load_shader(VERTEX_SHADER_SOURCE, VERTEX_SHADER);
    let fragment_rectangle_shader = load_shader(FRAGMENT_RECTANGLE_SHADER_SOURCE, FRAGMENT_SHADER);
    let program_rectangle = init_program(vertex_rectangle_shader, fragment_rectangle_shader);

    enable(TEXTURE_2D);
    enable(TEXTURE_RECTANGLE_ARB);
    enable(BLEND);
    blend_func(SRC_ALPHA, ONE_MINUS_SRC_ALPHA);

    RenderContext::new(Some(program_2d), Some(program_rectangle))
}

#[cfg(target_os="android")]
pub fn init_render_context() -> RenderContext {
    let vertex_2d_shader = load_shader(VERTEX_SHADER_SOURCE, VERTEX_SHADER);
    let fragment_2d_shader = load_shader(FRAGMENT_2D_SHADER_SOURCE, FRAGMENT_SHADER);
    let program_2d = init_program(vertex_2d_shader, fragment_2d_shader);

    enable(TEXTURE_2D);
    enable(BLEND);
    blend_func(SRC_ALPHA, ONE_MINUS_SRC_ALPHA);

    RenderContext::new(Some(program_2d), None)
}

fn bind_texture_coordinate_buffer(render_context: RenderContext, flip: Flip) {
    match flip {
        NoFlip => bind_buffer(ARRAY_BUFFER, render_context.buffers.texture_coordinate_buffer),
        VerticalFlip => {
            bind_buffer(ARRAY_BUFFER, render_context.buffers.flipped_texture_coordinate_buffer)
        }
    }
}

pub fn bind_and_render_quad(render_context: RenderContext,
                            texture: &Texture,
                            flip: Flip,
                            transform: &Matrix4<f32>,
                            scene_size: Size2D<f32>) {
    let program_id = match texture.target {
        TextureTarget2D => match render_context.program_2d {
            Some(program) => {program.id},
            None => {fail!("There is no shader program for texture 2D");}
        },
        TextureTargetRectangle(..) => match render_context.program_rectangle {
            Some(program) => {program.id},
            None => {fail!("There is no shader program for texture rectangle");}
        },
    };

    use_program(program_id);
    active_texture(TEXTURE0);
    let _bound_texture = texture.bind();

    // Set the projection matrix.
    let projection_matrix = ortho(0.0, scene_size.width, scene_size.height, 0.0, -10.0, 10.0);

    // Set uniforms and vertex attribute pointers.
    match texture.target {
        TextureTarget2D => {
            uniform_1i(render_context.program_2d.unwrap().sampler_uniform, 0);
            uniform_matrix_4fv(render_context.program_2d.unwrap().modelview_uniform,
                               false,
                               transform.to_array());
            uniform_matrix_4fv(render_context.program_2d.unwrap().projection_uniform,
                               false,
                               projection_matrix.to_array());

            bind_buffer(ARRAY_BUFFER, render_context.buffers.vertex_buffer);
            vertex_attrib_pointer_f32(render_context.program_2d.unwrap().vertex_position_attr as GLuint,
                                      3,
                                      false,
                                      0,
                                      0);

            bind_texture_coordinate_buffer(render_context, flip);
            vertex_attrib_pointer_f32(render_context.program_2d.unwrap().texture_coord_attr as GLuint,
                                      2,
                                      false,
                                      0,
                                      0);
        }
        TextureTargetRectangle(size) => {
            uniform_1i(render_context.program_rectangle.unwrap().sampler_uniform, 0);
            uniform_2f(render_context.program_rectangle.unwrap().size_uniform,
                       size.width as GLfloat,
                       size.height as GLfloat);
            uniform_matrix_4fv(render_context.program_rectangle.unwrap().modelview_uniform,
                               false,
                               transform.to_array());
            uniform_matrix_4fv(render_context.program_rectangle.unwrap().projection_uniform,
                               false,
                               projection_matrix.to_array());

            bind_buffer(ARRAY_BUFFER, render_context.buffers.vertex_buffer);
            vertex_attrib_pointer_f32(render_context.program_rectangle.unwrap().vertex_position_attr as
                                      GLuint,
                                      3,
                                      false,
                                      0,
                                      0);

            bind_texture_coordinate_buffer(render_context, flip);
            vertex_attrib_pointer_f32(render_context.program_rectangle.unwrap().texture_coord_attr as
                                      GLuint,
                                      2,
                                      false,
                                      0,
                                      0);
        }
    }

    // Draw!
    draw_arrays(TRIANGLE_STRIP, 0, 4);
    bind_texture(TEXTURE_2D, 0);
}

// Layer rendering

pub trait Render {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>);
}

impl<T> Render for layers::ContainerLayer<T> {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>) {
        let tmp = self.common.borrow();
        let transform = transform.translate(tmp.origin.x, tmp.origin.y, 0.0).mul(&tmp.transform);
        for tile in self.tiles.borrow().iter() {
            tile.render(render_context, transform, scene_size)
        }
        for child in self.children() {
            child.render(render_context, transform, scene_size)
        }
    }
}

impl Render for layers::TextureLayer {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>) {
        let transform = transform.mul(&self.transform);
        bind_and_render_quad(render_context, &self.texture, self.flip, &transform, scene_size);
    }
}

pub fn render_scene<T>(root_layer: Rc<ContainerLayer<T>>, render_context: RenderContext, scene: &Scene<T>) {
    // Set the viewport.
    viewport(0 as GLint, 0 as GLint, scene.size.width as GLsizei, scene.size.height as GLsizei);

    // Clear the screen.
    clear_color(scene.background_color.r,
                scene.background_color.g,
                scene.background_color.b,
                scene.background_color.a);
    clear(COLOR_BUFFER_BIT);

    // Set up the initial modelview matrix.
    let transform = scene.transform;

    // Render the root layer.
    root_layer.render(render_context, transform, scene.size);
}

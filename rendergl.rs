// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use layers::{ContainerLayerKind, Flip, NoFlip, TextureLayerKind, VerticalFlip};
use layers;
use scene::Scene;
use texturegl::{Texture, TextureTarget2D, TextureTargetRectangle};

use geom::matrix::{Matrix4, ortho};
use geom::point::Point2D;
use geom::size::Size2D;
use geom::rect::Rect;
use opengles::gl2::{ARRAY_BUFFER, COLOR_BUFFER_BIT, COMPILE_STATUS, FRAGMENT_SHADER, LINK_STATUS};
use opengles::gl2::{NO_ERROR, SCISSOR_BOX, SCISSOR_TEST, STATIC_DRAW, TEXTURE_2D};
use opengles::gl2::{TEXTURE_RECTANGLE_ARB, TEXTURE0, TRIANGLE_STRIP, VERTEX_SHADER, VIEWPORT};
use opengles::gl2::{GLenum, GLfloat, GLint, GLsizei, GLuint, active_texture, attach_shader};
use opengles::gl2::{bind_buffer, bind_texture, buffer_data, create_program, clear, clear_color};
use opengles::gl2::{compile_shader};
use opengles::gl2::{create_shader, draw_arrays, disable, enable, enable_vertex_attrib_array};
use opengles::gl2::{gen_buffers, get_attrib_location, get_error, get_integer_v, get_program_iv};
use opengles::gl2::{get_shader_info_log, get_shader_iv, get_uniform_location, is_enabled};
use opengles::gl2::{link_program, scissor, shader_source, uniform_1i, uniform_2f};
use opengles::gl2::{uniform_matrix_4fv, use_program, vertex_attrib_pointer_f32, viewport};
use std::libc::c_int;

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
    shader_source(shader_id, [ source_string.as_bytes().to_owned() ]);
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
        let vertex_buffer = gen_buffers(1)[0];
        bind_buffer(ARRAY_BUFFER, vertex_buffer);
        buffer_data(ARRAY_BUFFER, VERTICES, STATIC_DRAW);

        let texture_coordinate_buffer = gen_buffers(1)[0];
        bind_buffer(ARRAY_BUFFER, texture_coordinate_buffer);
        buffer_data(ARRAY_BUFFER, TEXTURE_COORDINATES, STATIC_DRAW);

        let flipped_texture_coordinate_buffer = gen_buffers(1)[0];
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
    let vertex_2d_shader = load_shader(VERTEX_SHADER_SOURCE, VERTEX_SHADER);
    let fragment_2d_shader = load_shader(FRAGMENT_2D_SHADER_SOURCE, FRAGMENT_SHADER);
    let program_2d = init_program(vertex_2d_shader, fragment_2d_shader);

    let vertex_rectangle_shader = load_shader(VERTEX_SHADER_SOURCE, VERTEX_SHADER);
    let fragment_rectangle_shader = load_shader(FRAGMENT_RECTANGLE_SHADER_SOURCE, FRAGMENT_SHADER);
    let program_rectangle = init_program(vertex_rectangle_shader, fragment_rectangle_shader);

    enable(TEXTURE_2D);
    enable(TEXTURE_RECTANGLE_ARB);

    RenderContext::new(Some(program_2d), Some(program_rectangle))
}

#[cfg(target_os="android")]
pub fn init_render_context() -> RenderContext {
    let vertex_2d_shader = load_shader(VERTEX_SHADER_SOURCE, VERTEX_SHADER);
    let fragment_2d_shader = load_shader(FRAGMENT_2D_SHADER_SOURCE, FRAGMENT_SHADER);
    let program_2d = init_program(vertex_2d_shader, fragment_2d_shader);

    enable(TEXTURE_2D);

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

impl Render for layers::ContainerLayer {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>) {
        let old_rect_opt = if is_enabled(SCISSOR_TEST) {
            let mut old_rect = [0 as GLint, ..4];
            get_integer_v(SCISSOR_BOX, old_rect);
            Some(Rect(Point2D(old_rect[0], old_rect[1]), Size2D(old_rect[2], old_rect[3])))
        } else {
            None
        };

        // NOTE: work around borrowchk
        let tmp = self.scissor.borrow();
        match tmp.get() {
            &Some(rect) => {
                let size = Size2D((rect.size.width * transform.m11) as GLint,
                                  (rect.size.height * transform.m22) as GLint);
                
                // Get the viewport height so we can flip the origin horizontally
                // since glScissor measures from the bottom left of the viewport
                let mut viewport = [0 as GLint, ..4];
                get_integer_v(VIEWPORT, viewport);
                let w_height = viewport[3];
                let origin = Point2D((rect.origin.x * transform.m11 + transform.m41) as GLint,
                                     w_height
                                     -((rect.origin.y * transform.m22 + transform.m42) as GLint)
                                     -size.height);                
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
            &None => {} // Nothing to do
        }

        // NOTE: work around borrowchk
        {
            let tmp = self.common.borrow();
            let transform = transform.mul(&tmp.get().transform);
            for child in self.children() {
                render_layer(render_context, transform, scene_size, child);
            }
        }

        // NOTE: work around borrowchk
        let tmp = self.scissor.borrow();
        match (tmp.get(), old_rect_opt) {
            (&Some(_), Some(old_rect)) => {
                // Set scissor back to the parent's scissoring rect.
                scissor(old_rect.origin.x, old_rect.origin.y, 
                        old_rect.size.width as GLsizei,
                        old_rect.size.height as GLsizei);
            }
            (&Some(_), None) => {
                // Our parents are not being scissored, so disable scissoring for now
                disable(SCISSOR_TEST);
            }
            (&None, _) => {} // Nothing to do
        }
    }
}

impl Render for layers::TextureLayer {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>) {
        let tmp = self.common.borrow();
        let transform = transform.mul(&tmp.get().transform);
        bind_and_render_quad(render_context, &self.texture, self.flip, &transform, scene_size);
    }
}

fn render_layer(render_context: RenderContext,
                transform: Matrix4<f32>,
                scene_size: Size2D<f32>,
                layer: layers::Layer) {
    match layer {
        ContainerLayerKind(container_layer) => {
            container_layer.borrow().render(render_context, transform, scene_size)
        }
        TextureLayerKind(texture_layer) => {
            texture_layer.borrow().render(render_context, transform, scene_size)
        }
    }
}

pub fn render_scene(render_context: RenderContext, scene: &Scene) {
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
    render_layer(render_context, transform, scene.size, scene.root.clone());
}

#[cfg(debug)]
fn maybe_get_error() {
    if get_error() != NO_ERROR {
        fail!("GL error: {:d}", get_error() as int);
    }
}

#[cfg(not(debug))]
fn maybe_get_error() {
    // do nothing
}

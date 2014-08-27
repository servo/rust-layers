// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use color::Color;
use layers::Layer;
use layers;
use scene::Scene;
use texturegl::{Flip, Linear, Nearest, NoFlip, VerticalFlip};
use texturegl::{Texture, TextureTarget2D, TextureTargetRectangle};
use tiling::Tile;
use platform::surface::NativeCompositingGraphicsContext;

use geom::matrix::{Matrix4, ortho};
use geom::size::Size2D;
use libc::c_int;
use opengles::gl2::{ARRAY_BUFFER, BLEND, COLOR_BUFFER_BIT, COMPILE_STATUS, FRAGMENT_SHADER};
use opengles::gl2::{LINK_STATUS, ONE_MINUS_SRC_ALPHA};
use opengles::gl2::{SRC_ALPHA, STATIC_DRAW, TEXTURE_2D, TEXTURE0};
use opengles::gl2::{LINE_STRIP, TRIANGLE_STRIP, VERTEX_SHADER, GLenum, GLfloat, GLint, GLsizei};
use opengles::gl2::{GLuint, active_texture, attach_shader, bind_buffer, bind_texture, blend_func};
use opengles::gl2::{buffer_data, create_program, clear, clear_color, compile_shader};
use opengles::gl2::{create_shader, draw_arrays, enable, enable_vertex_attrib_array, disable_vertex_attrib_array};
use opengles::gl2::{gen_buffers, get_attrib_location, get_program_iv};
use opengles::gl2::{get_shader_info_log, get_shader_iv, get_uniform_location, line_width};
use opengles::gl2::{link_program, shader_source, uniform_1i, uniform_2f, uniform_4f};
use opengles::gl2::{uniform_matrix_4fv, use_program, vertex_attrib_pointer_f32, viewport};
use std::num::Zero;
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

#[cfg(not(target_os="android"))]
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

static SOLID_COLOR_FRAGMENT_SHADER_SOURCE: &'static str = "
    #ifdef GL_ES
        precision mediump float;
    #endif

    uniform vec4 uColor;
    void main(void) {
        gl_FragColor = uColor;
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

static TEXTURED_QUAD_VERTICES: [f32, ..12] = [
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

static LINE_QUAD_VERTICES: [f32, ..15] = [
    0.0, 0.0, 0.0,
    0.0, 1.0, 0.0,
    1.0, 1.0, 0.0,
    1.0, 0.0, 0.0,
    0.0, 0.0, 0.0,
];

static TILE_DEBUG_BORDER_COLOR: Color = Color { r: 0., g: 1., b: 1., a: 1.0 };
static TILE_DEBUG_BORDER_THICKNESS: uint = 1;
static LAYER_DEBUG_BORDER_COLOR: Color = Color { r: 1., g: 0.5, b: 0., a: 1.0 };
static LAYER_DEBUG_BORDER_THICKNESS: uint = 2;

pub fn load_shader(source_string: &str, shader_type: GLenum) -> GLuint {
    let shader_id = create_shader(shader_type);
    shader_source(shader_id, [ source_string.as_bytes() ]);
    compile_shader(shader_id);

    if get_shader_iv(shader_id, COMPILE_STATUS) == (0 as GLint) {
        debug!("shader info log: {:s}", get_shader_info_log(shader_id));
        fail!("failed to compile shader");
    }

    return shader_id;
}

struct Buffers {
    textured_quad_vertex_buffer: GLuint,
    texture_coordinate_buffer: GLuint,
    flipped_texture_coordinate_buffer: GLuint,
    line_quad_vertex_buffer: GLuint,
}

struct Texture2DProgram {
    id: GLuint,
    vertex_position_attr: c_int,
    texture_coord_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    sampler_uniform: c_int,
}

impl Texture2DProgram {
    fn new() -> Texture2DProgram {
        let vertex_shader = load_shader(VERTEX_SHADER_SOURCE, VERTEX_SHADER);
        let fragment_shader = load_shader(FRAGMENT_2D_SHADER_SOURCE, FRAGMENT_SHADER);
        let program_id = init_program(vertex_shader, fragment_shader);

        Texture2DProgram {
            id: program_id,
            vertex_position_attr: get_attrib_location(program_id, "aVertexPosition"),
            texture_coord_attr: get_attrib_location(program_id, "aTextureCoord"),
            modelview_uniform: get_uniform_location(program_id, "uMVMatrix"),
            projection_uniform: get_uniform_location(program_id, "uPMatrix"),
            sampler_uniform: get_uniform_location(program_id, "uSampler"),
        }
    }

    fn bind_uniforms_and_attributes(&self,
                                    texture: &Texture,
                                    transform: &Matrix4<f32>,
                                    projection_matrix: &Matrix4<f32>,
                                    buffers: &Buffers) {
        uniform_1i(self.sampler_uniform, 0);
        uniform_matrix_4fv(self.modelview_uniform, false, transform.to_array());
        uniform_matrix_4fv(self.projection_uniform, false, projection_matrix.to_array());

        bind_buffer(ARRAY_BUFFER, buffers.textured_quad_vertex_buffer);
        vertex_attrib_pointer_f32(self.vertex_position_attr as GLuint, 3, false, 0, 0);

        bind_texture_coordinate_buffer(buffers, texture.flip);
        vertex_attrib_pointer_f32(self.texture_coord_attr as GLuint, 2, false, 0, 0);
    }

    fn enable_attribute_arrays(&self) {
        enable_vertex_attrib_array(self.vertex_position_attr as GLuint);
        enable_vertex_attrib_array(self.texture_coord_attr as GLuint);
    }

    fn disable_attribute_arrays(&self) {
        disable_vertex_attrib_array(self.vertex_position_attr as GLuint);
        disable_vertex_attrib_array(self.texture_coord_attr as GLuint);
    }
}

struct TextureRectangleProgram {
    id: GLuint,
    vertex_position_attr: c_int,
    texture_coord_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    sampler_uniform: c_int,
    size_uniform: c_int,
}

impl TextureRectangleProgram {
    #[cfg(not(target_os="android"))]
    fn new() -> TextureRectangleProgram {
        let vertex_shader = load_shader(VERTEX_SHADER_SOURCE, VERTEX_SHADER);
        let fragment_shader = load_shader(FRAGMENT_RECTANGLE_SHADER_SOURCE, FRAGMENT_SHADER);
        let program_id = init_program(vertex_shader, fragment_shader);

        TextureRectangleProgram {
            id: program_id,
            vertex_position_attr: get_attrib_location(program_id, "aVertexPosition"),
            texture_coord_attr: get_attrib_location(program_id, "aTextureCoord"),
            modelview_uniform: get_uniform_location(program_id, "uMVMatrix"),
            projection_uniform: get_uniform_location(program_id, "uPMatrix"),
            sampler_uniform: get_uniform_location(program_id, "uSampler"),
            size_uniform: get_uniform_location(program_id, "uSize"),
        }
    }

    #[cfg(not(target_os="android"))]
    fn create_if_necessary() -> Option<TextureRectangleProgram> {
        use opengles::gl2::TEXTURE_RECTANGLE_ARB;
        enable(TEXTURE_RECTANGLE_ARB);
        Some(TextureRectangleProgram::new())
    }

    #[cfg(target_os="android")]
    fn create_if_necessary() -> Option<TextureRectangleProgram> {
        None
    }

    fn bind_uniforms_and_attributes(&self,
                                    texture: &Texture,
                                    transform: &Matrix4<f32>,
                                    projection_matrix: &Matrix4<f32>,
                                    buffers: &Buffers) {
        uniform_1i(self.sampler_uniform, 0);
        uniform_2f(self.size_uniform,
                   texture.size.width as GLfloat,
                   texture.size.height as GLfloat);
        uniform_matrix_4fv(self.modelview_uniform, false, transform.to_array());
        uniform_matrix_4fv(self.projection_uniform, false, projection_matrix.to_array());

        bind_buffer(ARRAY_BUFFER, buffers.textured_quad_vertex_buffer);
        vertex_attrib_pointer_f32(self.vertex_position_attr as GLuint, 3, false, 0, 0);

        bind_texture_coordinate_buffer(buffers, texture.flip);
        vertex_attrib_pointer_f32(self.texture_coord_attr as GLuint, 2, false, 0, 0);
    }

    fn enable_attribute_arrays(&self) {
        enable_vertex_attrib_array(self.vertex_position_attr as GLuint);
        enable_vertex_attrib_array(self.texture_coord_attr as GLuint);
    }

    fn disable_attribute_arrays(&self) {
        disable_vertex_attrib_array(self.vertex_position_attr as GLuint);
        disable_vertex_attrib_array(self.texture_coord_attr as GLuint);
    }
}

struct SolidLineProgram {
    id: GLuint,
    vertex_position_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    color_uniform: c_int,
}

impl SolidLineProgram {
    fn new() -> SolidLineProgram {
        let vertex_shader = load_shader(VERTEX_SHADER_SOURCE, VERTEX_SHADER);
        let fragment_shader = load_shader(SOLID_COLOR_FRAGMENT_SHADER_SOURCE, FRAGMENT_SHADER);
        let program_id = init_program(vertex_shader, fragment_shader);

        SolidLineProgram {
            id: program_id,
            vertex_position_attr: get_attrib_location(program_id, "aVertexPosition"),
            modelview_uniform: get_uniform_location(program_id, "uMVMatrix"),
            projection_uniform: get_uniform_location(program_id, "uPMatrix"),
            color_uniform: get_uniform_location(program_id, "uColor"),
        }
    }

    fn bind_uniforms_and_attributes(&self,
                                    transform: &Matrix4<f32>,
                                    projection_matrix: &Matrix4<f32>,
                                    buffers: &Buffers,
                                    color: Color) {
        uniform_matrix_4fv(self.modelview_uniform, false, transform.to_array());
        uniform_matrix_4fv(self.projection_uniform, false, projection_matrix.to_array());
        uniform_4f(self.color_uniform,
                   color.r as GLfloat,
                   color.g as GLfloat,
                   color.b as GLfloat,
                   color.a as GLfloat);

        bind_buffer(ARRAY_BUFFER, buffers.line_quad_vertex_buffer);
        vertex_attrib_pointer_f32(self.vertex_position_attr as GLuint, 3, false, 0, 0);
    }

    fn enable_attribute_arrays(&self) {
        enable_vertex_attrib_array(self.vertex_position_attr as GLuint);
    }

    fn disable_attribute_arrays(&self) {
        disable_vertex_attrib_array(self.vertex_position_attr as GLuint);
    }
}

pub struct RenderContext {
    texture_2d_program: Texture2DProgram,
    texture_rectangle_program: Option<TextureRectangleProgram>,
    solid_line_program: SolidLineProgram,
    buffers: Buffers,

    /// The platform-specific graphics context.
    compositing_context: NativeCompositingGraphicsContext,

    /// Whether to show lines at border and tile boundaries for debugging purposes.
    show_debug_borders: bool,
}

impl RenderContext {
    pub fn new(compositing_context: NativeCompositingGraphicsContext,
               show_debug_borders: bool) -> RenderContext {
        enable(TEXTURE_2D);
        enable(BLEND);
        blend_func(SRC_ALPHA, ONE_MINUS_SRC_ALPHA);

        let texture_2d_program = Texture2DProgram::new();
        let solid_line_program = SolidLineProgram::new();
        let texture_rectangle_program = TextureRectangleProgram::create_if_necessary();

        RenderContext {
            texture_2d_program: texture_2d_program,
            texture_rectangle_program: texture_rectangle_program,
            solid_line_program: solid_line_program,
            buffers: RenderContext::init_buffers(),
            compositing_context: compositing_context,
            show_debug_borders: show_debug_borders,
        }
    }

    fn init_buffers() -> Buffers {
        let textured_quad_vertex_buffer = gen_buffers(1)[0];
        bind_buffer(ARRAY_BUFFER, textured_quad_vertex_buffer);
        buffer_data(ARRAY_BUFFER, TEXTURED_QUAD_VERTICES, STATIC_DRAW);

        let line_quad_vertex_buffer = gen_buffers(1)[0];
        bind_buffer(ARRAY_BUFFER, line_quad_vertex_buffer);
        buffer_data(ARRAY_BUFFER, LINE_QUAD_VERTICES, STATIC_DRAW);

        let texture_coordinate_buffer = gen_buffers(1)[0];
        bind_buffer(ARRAY_BUFFER, texture_coordinate_buffer);
        buffer_data(ARRAY_BUFFER, TEXTURE_COORDINATES, STATIC_DRAW);

        let flipped_texture_coordinate_buffer = gen_buffers(1)[0];
        bind_buffer(ARRAY_BUFFER, flipped_texture_coordinate_buffer);
        buffer_data(ARRAY_BUFFER, FLIPPED_TEXTURE_COORDINATES, STATIC_DRAW);

        Buffers {
            textured_quad_vertex_buffer: textured_quad_vertex_buffer,
            texture_coordinate_buffer: texture_coordinate_buffer,
            flipped_texture_coordinate_buffer: flipped_texture_coordinate_buffer,
            line_quad_vertex_buffer: line_quad_vertex_buffer,
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

fn bind_texture_coordinate_buffer(buffers: &Buffers, flip: Flip) {
    match flip {
        NoFlip => bind_buffer(ARRAY_BUFFER, buffers.texture_coordinate_buffer),
        VerticalFlip => {
            bind_buffer(ARRAY_BUFFER, buffers.flipped_texture_coordinate_buffer)
        }
    }
}

pub fn bind_and_render_quad(render_context: RenderContext,
                            texture: &Texture,
                            transform: &Matrix4<f32>,
                            scene_size: Size2D<f32>) {
    let program_id = match texture.target {
        TextureTarget2D => {
            render_context.texture_2d_program.enable_attribute_arrays();
            render_context.texture_2d_program.id
        }
        TextureTargetRectangle(..) => match render_context.texture_rectangle_program {
            Some(program) => {
                program.enable_attribute_arrays();
                program.id
            }
            None => fail!("There is no shader program for texture rectangle"),
        },
    };

    use_program(program_id);
    active_texture(TEXTURE0);

    // FIXME: This should technically check that the transform
    // matrix only contains scale in these components.
    let has_scale = transform.m11 as uint != texture.size.width ||
                    transform.m22 as uint != texture.size.height;
    let filter_mode = if has_scale {
        Linear
    } else {
        Nearest
    };
    texture.set_filter_mode(filter_mode);

    let _bound_texture = texture.bind();

    // Set the projection matrix.
    let projection_matrix = ortho(0.0, scene_size.width, scene_size.height, 0.0, -10.0, 10.0);

    // Set uniforms and vertex attribute pointers.
    match texture.target {
        TextureTarget2D => {
            render_context.texture_2d_program.
                bind_uniforms_and_attributes(texture,
                                             transform,
                                             &projection_matrix,
                                             &render_context.buffers);
        }
        TextureTargetRectangle => {
            render_context.texture_rectangle_program.unwrap().
                bind_uniforms_and_attributes(texture,
                                             transform,
                                             &projection_matrix,
                                             &render_context.buffers);
        }
    }

    // Draw!
    draw_arrays(TRIANGLE_STRIP, 0, 4);
    bind_texture(TEXTURE_2D, 0);

    match texture.target {
        TextureTarget2D => render_context.texture_2d_program.disable_attribute_arrays(),
        TextureTargetRectangle(..) => match render_context.texture_rectangle_program {
            Some(program) => program.disable_attribute_arrays(),
            None => {},
        },
    };
}

pub fn bind_and_render_quad_lines(render_context: RenderContext,
                                  transform: &Matrix4<f32>,
                                  scene_size: Size2D<f32>,
                                  color: Color,
                                  line_thickness: uint) {
    let solid_line_program = render_context.solid_line_program;
    solid_line_program.enable_attribute_arrays();
    use_program(solid_line_program.id);
    let projection_matrix = ortho(0.0, scene_size.width, scene_size.height, 0.0, -10.0, 10.0);
    solid_line_program.bind_uniforms_and_attributes(transform,
                                                    &projection_matrix,
                                                    &render_context.buffers,
                                                    color);
    line_width(line_thickness as GLfloat);
    draw_arrays(LINE_STRIP, 0, 5);
    solid_line_program.disable_attribute_arrays();
}

// Layer rendering

pub trait Render {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>);
}

impl<T> Render for layers::Layer<T> {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>) {
        let bounds = self.bounds.borrow().to_untyped();
        let transform = transform.translate(bounds.origin.x, bounds.origin.y, 0.0)
            .mul(&*self.transform.borrow());

        self.create_textures(&render_context.compositing_context);
        self.do_for_all_tiles(|tile: &Tile| {
            tile.render(render_context, transform, scene_size)
        });

        if render_context.show_debug_borders {
            let quad_transform = transform.scale(bounds.size.width, bounds.size.height, 1.);
            bind_and_render_quad_lines(render_context,
                                       &quad_transform,
                                       scene_size,
                                       LAYER_DEBUG_BORDER_COLOR,
                                       LAYER_DEBUG_BORDER_THICKNESS);
        }

        for child in self.children().iter() {
            child.render(render_context, transform, scene_size)
        }

    }
}

impl Render for Tile {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>) {
        if self.texture.is_zero() {
            return;
        }

        let transform = transform.mul(&self.transform);
        bind_and_render_quad(render_context, &self.texture, &transform, scene_size);

        if render_context.show_debug_borders {
            bind_and_render_quad_lines(render_context,
                                       &transform,
                                       scene_size,
                                       TILE_DEBUG_BORDER_COLOR,
                                       TILE_DEBUG_BORDER_THICKNESS);
        }
    }
}

pub fn render_scene<T>(root_layer: Rc<Layer<T>>, render_context: RenderContext,
                        scene: &Scene<T>) {
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

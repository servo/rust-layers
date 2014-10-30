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
use texturegl::{Linear, Nearest, VerticalFlip};
use texturegl::{Texture, TextureTarget2D, TextureTargetRectangle};
use tiling::Tile;
use platform::surface::NativeCompositingGraphicsContext;

use geom::matrix::{Matrix4, identity, ortho};
use geom::point::Point2D;
use geom::rect::Rect;
use geom::size::Size2D;
use libc::c_int;
use gleam::gl;
use gleam::gl::{GLenum, GLfloat, GLint, GLsizei, GLuint};
use std::fmt;
use std::num::Zero;
use std::rc::Rc;

static FRAGMENT_SHADER_SOURCE: &'static str = "
    #ifdef GL_ES
        precision mediump float;
    #endif

    varying vec2 vTextureCoord;
    uniform samplerType uSampler;

    void main(void) {
        gl_FragColor = samplerFunction(uSampler, vTextureCoord);
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
    attribute vec2 aVertexPosition;

    uniform mat4 uMVMatrix;
    uniform mat4 uPMatrix;
    uniform mat4 uTextureSpaceTransform;

    varying vec2 vTextureCoord;

    void main(void) {
        gl_Position = uPMatrix * uMVMatrix * vec4(aVertexPosition, 0.0, 1.0);
        vTextureCoord = (uTextureSpaceTransform * vec4(aVertexPosition, 0., 1.)).xy;
    }
";

static TEXTURED_QUAD_VERTICES: [f32, ..8] = [
    0.0, 0.0,
    0.0, 1.0,
    1.0, 0.0,
    1.0, 1.0,
];

static LINE_QUAD_VERTICES: [f32, ..10] = [
    0.0, 0.0,
    0.0, 1.0,
    1.0, 1.0,
    1.0, 0.0,
    0.0, 0.0,
];

static TILE_DEBUG_BORDER_COLOR: Color = Color { r: 0., g: 1., b: 1., a: 1.0 };
static TILE_DEBUG_BORDER_THICKNESS: uint = 1;
static LAYER_DEBUG_BORDER_COLOR: Color = Color { r: 1., g: 0.5, b: 0., a: 1.0 };
static LAYER_DEBUG_BORDER_THICKNESS: uint = 2;

struct Buffers {
    textured_quad_vertex_buffer: GLuint,
    line_quad_vertex_buffer: GLuint,
}

struct ShaderProgram {
    id: GLuint,
}

impl ShaderProgram {
    pub fn new(vertex_shader_source: &str, fragment_shader_source: &str) -> ShaderProgram {
        let id = gl::create_program();
        gl::attach_shader(id, ShaderProgram::compile_shader(fragment_shader_source, gl::FRAGMENT_SHADER));
        gl::attach_shader(id, ShaderProgram::compile_shader(vertex_shader_source, gl::VERTEX_SHADER));
        gl::link_program(id);
        if gl::get_program_iv(id, gl::LINK_STATUS) == (0 as GLint) {
            fail!("Failed to compile shader program: {:s}", gl::get_program_info_log(id));
        }

        ShaderProgram {
            id: id,
        }
    }

    pub fn compile_shader(source_string: &str, shader_type: GLenum) -> GLuint {
        let id = gl::create_shader(shader_type);
        gl::shader_source(id, [ source_string.as_bytes() ]);
        gl::compile_shader(id);
        if gl::get_shader_iv(id, gl::COMPILE_STATUS) == (0 as GLint) {
            fail!("Failed to compile shader: {:s}", gl::get_shader_info_log(id));
        }

        return id;
    }

    pub fn get_attribute_location(&self, name: &str) -> GLint {
        gl::get_attrib_location(self.id, name)
    }

    pub fn get_uniform_location(&self, name: &str) -> GLint {
        gl::get_uniform_location(self.id, name)
    }
}

struct TextureProgram {
    program: ShaderProgram,
    vertex_position_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    sampler_uniform: c_int,
    texture_space_transform_uniform: c_int,
}

impl TextureProgram {
    fn new(sampler_function: &str, sampler_type: &str) -> TextureProgram {
        let fragment_shader_source
             = format_args!(fmt::format,
                            "#define samplerFunction {}\n#define samplerType {}\n{}",
                            sampler_function,
                            sampler_type,
                            FRAGMENT_SHADER_SOURCE);
        let program = ShaderProgram::new(VERTEX_SHADER_SOURCE, fragment_shader_source.as_slice());
        TextureProgram {
            program: program,
            vertex_position_attr: program.get_attribute_location("aVertexPosition"),
            modelview_uniform: program.get_uniform_location("uMVMatrix"),
            projection_uniform: program.get_uniform_location("uPMatrix"),
            sampler_uniform: program.get_uniform_location("uSampler"),
            texture_space_transform_uniform: program.get_uniform_location("uTextureSpaceTransform"),
        }
    }

    fn bind_uniforms_and_attributes(&self,
                                    transform: &Matrix4<f32>,
                                    projection_matrix: &Matrix4<f32>,
                                    texture_space_transform: &Matrix4<f32>,
                                    buffers: &Buffers,
                                    unit_rect: Rect<f32>) {
        gl::uniform_1i(self.sampler_uniform, 0);
        gl::uniform_matrix_4fv(self.modelview_uniform, false, transform.to_array());
        gl::uniform_matrix_4fv(self.projection_uniform, false, projection_matrix.to_array());

        let new_coords: [f32, ..8] = [
            unit_rect.min_x(), unit_rect.min_y(),
            unit_rect.min_x(), unit_rect.max_y(),
            unit_rect.max_x(), unit_rect.min_y(),
            unit_rect.max_x(), unit_rect.max_y(),
        ];
        gl::bind_buffer(gl::ARRAY_BUFFER, buffers.textured_quad_vertex_buffer);
        gl::buffer_data(gl::ARRAY_BUFFER, new_coords, gl::STATIC_DRAW);
        gl::vertex_attrib_pointer_f32(self.vertex_position_attr as GLuint, 2, false, 0, 0);

        gl::uniform_matrix_4fv(self.texture_space_transform_uniform,
                           false,
                           texture_space_transform.to_array());
    }

    fn enable_attribute_arrays(&self) {
        gl::enable_vertex_attrib_array(self.vertex_position_attr as GLuint);
    }

    fn disable_attribute_arrays(&self) {
        gl::disable_vertex_attrib_array(self.vertex_position_attr as GLuint);
    }

    fn create_2d_program() -> TextureProgram {
        TextureProgram::new("texture2D", "sampler2D")
    }

    #[cfg(not(target_os="android"))]
    fn create_rectangle_program_if_necessary() -> Option<TextureProgram> {
        gl::enable(gl::TEXTURE_RECTANGLE_ARB);
        Some(TextureProgram::new("texture2DRect", "sampler2DRect"))
    }

    #[cfg(target_os="android")]
    fn create_rectangle_program_if_necessary() -> Option<TextureProgram> {
        None
    }
}

struct SolidColorProgram {
    program: ShaderProgram,
    vertex_position_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    color_uniform: c_int,
    texture_space_transform_uniform: c_int,
}

impl SolidColorProgram {
    fn new() -> SolidColorProgram {
        let program = ShaderProgram::new(VERTEX_SHADER_SOURCE, SOLID_COLOR_FRAGMENT_SHADER_SOURCE);
        SolidColorProgram {
            program: program,
            vertex_position_attr: program.get_attribute_location("aVertexPosition"),
            modelview_uniform: program.get_uniform_location("uMVMatrix"),
            projection_uniform: program.get_uniform_location("uPMatrix"),
            color_uniform: program.get_uniform_location("uColor"),
            texture_space_transform_uniform: program.get_uniform_location("uTextureSpaceTransform"),
        }
    }

    fn bind_uniforms_and_attributes_common(&self,
                                           transform: &Matrix4<f32>,
                                           projection_matrix: &Matrix4<f32>,
                                           color: Color) {
        gl::uniform_matrix_4fv(self.modelview_uniform, false, transform.to_array());
        gl::uniform_matrix_4fv(self.projection_uniform, false, projection_matrix.to_array());
        gl::uniform_4f(self.color_uniform,
                   color.r as GLfloat,
                   color.g as GLfloat,
                   color.b as GLfloat,
                   color.a as GLfloat);

        let texture_transform: Matrix4<f32> = identity();
        gl::uniform_matrix_4fv(self.texture_space_transform_uniform,
                           false,
                           texture_transform.to_array());
    }

    fn bind_uniforms_and_attributes_for_lines(&self,
                                              transform: &Matrix4<f32>,
                                              projection_matrix: &Matrix4<f32>,
                                              buffers: &Buffers,
                                              color: Color) {
        self.bind_uniforms_and_attributes_common(transform, projection_matrix, color);
        gl::bind_buffer(gl::ARRAY_BUFFER, buffers.line_quad_vertex_buffer);
        gl::vertex_attrib_pointer_f32(self.vertex_position_attr as GLuint, 2, false, 0, 0);
    }

    fn bind_uniforms_and_attributes_for_quad(&self,
                                             transform: &Matrix4<f32>,
                                             projection_matrix: &Matrix4<f32>,
                                             buffers: &Buffers,
                                             color: Color,
                                             unit_rect: Rect<f32>) {
        self.bind_uniforms_and_attributes_common(transform, projection_matrix, color);

        let new_coords: [f32, ..8] = [
            unit_rect.origin.x, unit_rect.origin.y,
            unit_rect.origin.x, unit_rect.origin.y + unit_rect.size.height,
            unit_rect.origin.x + unit_rect.size.width, unit_rect.origin.y,
            unit_rect.origin.x + unit_rect.size.width, unit_rect.origin.y + unit_rect.size.height,
        ];
        gl::bind_buffer(gl::ARRAY_BUFFER, buffers.textured_quad_vertex_buffer);
        gl::buffer_data(gl::ARRAY_BUFFER, new_coords, gl::STATIC_DRAW);
        gl::vertex_attrib_pointer_f32(self.vertex_position_attr as GLuint, 2, false, 0, 0);
    }

    fn enable_attribute_arrays(&self) {
        gl::enable_vertex_attrib_array(self.vertex_position_attr as GLuint);
    }

    fn disable_attribute_arrays(&self) {
        gl::disable_vertex_attrib_array(self.vertex_position_attr as GLuint);
    }
}

pub struct RenderContext {
    texture_2d_program: TextureProgram,
    texture_rectangle_program: Option<TextureProgram>,
    solid_color_program: SolidColorProgram,
    buffers: Buffers,

    /// The platform-specific graphics context.
    compositing_context: NativeCompositingGraphicsContext,

    /// Whether to show lines at border and tile boundaries for debugging purposes.
    show_debug_borders: bool,
}

impl RenderContext {
    pub fn new(compositing_context: NativeCompositingGraphicsContext,
               show_debug_borders: bool) -> RenderContext {
        gl::enable(gl::TEXTURE_2D);
        gl::enable(gl::BLEND);
        gl::blend_func(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

        let texture_2d_program = TextureProgram::create_2d_program();
        let solid_color_program = SolidColorProgram::new();
        let texture_rectangle_program = TextureProgram::create_rectangle_program_if_necessary();

        RenderContext {
            texture_2d_program: texture_2d_program,
            texture_rectangle_program: texture_rectangle_program,
            solid_color_program: solid_color_program,
            buffers: RenderContext::init_buffers(),
            compositing_context: compositing_context,
            show_debug_borders: show_debug_borders,
        }
    }

    fn init_buffers() -> Buffers {
        let textured_quad_vertex_buffer = gl::gen_buffers(1)[0];
        gl::bind_buffer(gl::ARRAY_BUFFER, textured_quad_vertex_buffer);
        gl::buffer_data(gl::ARRAY_BUFFER, TEXTURED_QUAD_VERTICES, gl::STATIC_DRAW);

        let line_quad_vertex_buffer = gl::gen_buffers(1)[0];
        gl::bind_buffer(gl::ARRAY_BUFFER, line_quad_vertex_buffer);
        gl::buffer_data(gl::ARRAY_BUFFER, LINE_QUAD_VERTICES, gl::STATIC_DRAW);

        Buffers {
            textured_quad_vertex_buffer: textured_quad_vertex_buffer,
            line_quad_vertex_buffer: line_quad_vertex_buffer,
        }
    }
}

pub fn bind_and_render_quad(render_context: RenderContext,
                            texture: &Texture,
                            transform: &Matrix4<f32>,
                            scene_size: Size2D<f32>,
                            unit_rect: Rect<f32>) {
    let mut texture_coordinates_need_to_be_scaled_by_size = false;
    let program = match texture.target {
        TextureTarget2D => render_context.texture_2d_program,
        TextureTargetRectangle(..) => match render_context.texture_rectangle_program {
            Some(program) => {
                texture_coordinates_need_to_be_scaled_by_size = true;
                program
            }
            None => fail!("There is no shader program for texture rectangle"),
        },
    };
    program.enable_attribute_arrays();

    gl::use_program(program.program.id);
    gl::active_texture(gl::TEXTURE0);

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

    // We calculate a transformation matrix for the texture coordinates
    // which is useful for flipping the texture vertically or scaling the
    // coordinates when dealing with GL_ARB_texture_rectangle.
    let mut texture_transform: Matrix4<f32> = identity();
    if texture.flip == VerticalFlip {
        texture_transform = texture_transform.scale(1.0, -1.0, 1.0);
    }
    if texture_coordinates_need_to_be_scaled_by_size {
        texture_transform = texture_transform.scale(texture.size.width as f32,
                                                    texture.size.height as f32,
                                                    1.0);
    }
    if texture.flip == VerticalFlip {
        texture_transform = texture_transform.translate(0.0, -1.0, 0.0);
    }

    program.bind_uniforms_and_attributes(transform,
                                         &projection_matrix,
                                         &texture_transform,
                                         &render_context.buffers,
                                         unit_rect);

    // Draw!
    gl::draw_arrays(gl::TRIANGLE_STRIP, 0, 4);
    gl::bind_texture(gl::TEXTURE_2D, 0);

    program.disable_attribute_arrays()
}

pub fn bind_and_render_quad_lines(render_context: RenderContext,
                                  transform: &Matrix4<f32>,
                                  scene_size: Size2D<f32>,
                                  color: Color,
                                  line_thickness: uint) {
    let solid_color_program = render_context.solid_color_program;
    solid_color_program.enable_attribute_arrays();
    gl::use_program(solid_color_program.program.id);
    let projection_matrix = ortho(0.0, scene_size.width, scene_size.height, 0.0, -10.0, 10.0);
    solid_color_program.bind_uniforms_and_attributes_for_lines(transform,
                                                               &projection_matrix,
                                                               &render_context.buffers,
                                                               color);
    gl::line_width(line_thickness as GLfloat);
    gl::draw_arrays(gl::LINE_STRIP, 0, 5);
    solid_color_program.disable_attribute_arrays();
}

pub fn bind_and_render_solid_quad(render_context: RenderContext,
                                  transform: &Matrix4<f32>,
                                  scene_size: Size2D<f32>,
                                  color: Color,
                                  unit_rect: Rect<f32>) {
    let solid_color_program = render_context.solid_color_program;
    solid_color_program.enable_attribute_arrays();
    gl::use_program(solid_color_program.program.id);
    let projection_matrix = ortho(0.0, scene_size.width, scene_size.height, 0.0, -10.0, 10.0);
    solid_color_program.bind_uniforms_and_attributes_for_quad(transform,
                                                              &projection_matrix,
                                                              &render_context.buffers,
                                                              color,
                                                              unit_rect);
    gl::draw_arrays(gl::TRIANGLE_STRIP, 0, 4);
    solid_color_program.disable_attribute_arrays();
}

fn map_clip_to_unit_rectangle(rect: Rect<f32>,
                              clip_rect: Option<Rect<f32>>)
                              -> Rect<f32> {
    match clip_rect {
        Some(clip_rect) => {
            match clip_rect.intersection(&rect) {
                Some(intersected_rect) => {
                    let offset = Point2D(0., 0.) - rect.origin;
                    intersected_rect.translate(&offset).scale(1. / rect.size.width,
                                                              1. / rect.size.height)
                }
                None => Rect(Point2D(0., 0.), Size2D(0., 0.)),
            }
        },
        None => Rect(Point2D(0., 0.), Size2D(1., 1.))
    }
}

// Layer rendering
pub trait Render {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>,
              mut clip_rect: Option<Rect<f32>>,
              content_offset: Point2D<f32>);
}

impl<T> Render for layers::Layer<T> {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>,
              mut clip_rect: Option<Rect<f32>>,
              _: Point2D<f32>) {
        let bounds = self.bounds.borrow().to_untyped();
        let cumulative_transform = transform.translate(bounds.origin.x, bounds.origin.y, 0.0);
        let tile_transform = cumulative_transform.mul(&*self.transform.borrow());
        let content_offset = self.content_offset.borrow().to_untyped();

        if self.background_color.borrow().a != 0.0 {
            let background_transform = tile_transform.scale(bounds.size.width,
                                                            bounds.size.height,
                                                            1.0);
            let background_unit_rect = map_clip_to_unit_rectangle(bounds.translate(&content_offset),
                                                                  clip_rect);
            bind_and_render_solid_quad(render_context,
                                       &background_transform,
                                       scene_size,
                                       *self.background_color.borrow(),
                                       background_unit_rect);
        }

        self.create_textures(&render_context.compositing_context);
        self.do_for_all_tiles(|tile: &Tile| {
            tile.render(render_context, tile_transform, scene_size, clip_rect, content_offset)
        });

        if render_context.show_debug_borders {
            let quad_transform = tile_transform.scale(bounds.size.width, bounds.size.height, 1.);
            bind_and_render_quad_lines(render_context,
                                       &quad_transform,
                                       scene_size,
                                       LAYER_DEBUG_BORDER_COLOR,
                                       LAYER_DEBUG_BORDER_THICKNESS);
        }

        if *self.masks_to_bounds.borrow() {
            clip_rect = match clip_rect {
                Some(ref clip_rect) => clip_rect.intersection(&bounds),
                None => Some(bounds),
            };

            // We move our clipping rectangle into the coordinate system of child layers
            // and also account for the content offset, so that we can test based on
            // the final screen position.
            clip_rect = match clip_rect {
                Some(ref clip_rect) => {
                    let clip_offset = content_offset - bounds.origin;
                    Some(clip_rect.translate(&clip_offset))
                },
                None => return, // Don't render children, because we have an empty clip rect.
            };
        }

        for child in self.children().iter() {
            child.render(render_context,
                         cumulative_transform,
                         scene_size,
                         clip_rect,
                         Point2D(0., 0.))
        }

    }
}

impl Render for Tile {
    fn render(&self,
              render_context: RenderContext,
              transform: Matrix4<f32>,
              scene_size: Size2D<f32>,
              mut clip_rect: Option<Rect<f32>>,
              content_offset: Point2D<f32>) {
        if self.texture.is_zero() {
            return;
        }

        let bounds = match self.bounds {
            Some(ref bounds) => bounds.to_untyped().translate(&content_offset),
            None => return,
        };

        let quad_unit_rect = map_clip_to_unit_rectangle(bounds, clip_rect);
        if quad_unit_rect.is_empty() {
            return;
        }

        let transform = transform.mul(&self.transform);
        bind_and_render_quad(render_context,
                             &self.texture,
                             &transform,
                             scene_size,
                             quad_unit_rect);

        if render_context.show_debug_borders {
            bind_and_render_quad_lines(render_context,
                                       &transform,
                                       scene_size,
                                       TILE_DEBUG_BORDER_COLOR,
                                       TILE_DEBUG_BORDER_THICKNESS);
        }
    }
}

pub fn render_scene<T>(root_layer: Rc<Layer<T>>,
                       render_context: RenderContext,
                       scene: &Scene<T>) {
    // Set the viewport.
    let v = scene.viewport.to_untyped();
    gl::viewport(v.origin.x as GLint, v.origin.y as GLint,
                 v.size.width as GLsizei, v.size.height as GLsizei);

    // Set up the initial modelview matrix.
    let transform = identity().scale(scene.scale.get(), scene.scale.get(), 1.0);

    // Render the root layer.
    root_layer.render(render_context, transform, scene.viewport.size.to_untyped(), None,
                      Point2D(0., 0.));
}

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
use scene::Scene;
use texturegl::Texture;
use texturegl::Flip::VerticalFlip;
use texturegl::TextureTarget::{TextureTarget2D, TextureTargetRectangle};
use tiling::Tile;
use platform::surface::NativeDisplay;

use euclid::matrix::Matrix4;
use euclid::Matrix2D;
use euclid::point::Point2D;
use euclid::rect::Rect;
use euclid::size::Size2D;
use libc::c_int;
use gleam::gl;
use gleam::gl::{GLenum, GLfloat, GLint, GLsizei, GLuint};
use std::fmt;
use std::mem;
use std::rc::Rc;
use std::cmp::Ordering;

#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "plugins", derive(HeapSizeOf))]
pub struct ColorVertex {
    x: f32,
    y: f32,
}

impl ColorVertex {
    pub fn new(point: Point2D<f32>) -> ColorVertex {
        ColorVertex {
            x: point.x,
            y: point.y,
        }
    }
}

#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "plugins", derive(HeapSizeOf))]
pub struct TextureVertex {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
}

impl TextureVertex {
    pub fn new(point: Point2D<f32>, texture_coordinates: Point2D<f32>) -> TextureVertex {
        TextureVertex {
            x: point.x,
            y: point.y,
            u: texture_coordinates.x,
            v: texture_coordinates.y,
        }
    }
}

const ORTHO_NEAR_PLANE: f32 = -1000000.0;
const ORTHO_FAR_PLANE: f32 = 1000000.0;

fn create_ortho(scene_size: &Size2D<f32>) -> Matrix4 {
    Matrix4::ortho(0.0, scene_size.width, scene_size.height, 0.0, ORTHO_NEAR_PLANE, ORTHO_FAR_PLANE)
}

static TEXTURE_FRAGMENT_SHADER_SOURCE: &'static str = "
    #ifdef GL_ES
        precision mediump float;
    #endif

    varying vec2 vTextureCoord;
    uniform samplerType uSampler;
    uniform float uOpacity;

    void main(void) {
        vec4 lFragColor = uOpacity * samplerFunction(uSampler, vTextureCoord);
        gl_FragColor = lFragColor;
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

static TEXTURE_VERTEX_SHADER_SOURCE: &'static str = "
    attribute vec2 aVertexPosition;
    attribute vec2 aVertexUv;

    uniform mat4 uMVMatrix;
    uniform mat4 uPMatrix;
    uniform mat4 uTextureSpaceTransform;

    varying vec2 vTextureCoord;

    void main(void) {
        gl_Position = uPMatrix * uMVMatrix * vec4(aVertexPosition, 0.0, 1.0);
        vTextureCoord = (uTextureSpaceTransform * vec4(aVertexUv, 0., 1.)).xy;
    }
";

static SOLID_COLOR_VERTEX_SHADER_SOURCE: &'static str = "
    attribute vec2 aVertexPosition;

    uniform mat4 uMVMatrix;
    uniform mat4 uPMatrix;

    void main(void) {
        gl_Position = uPMatrix * uMVMatrix * vec4(aVertexPosition, 0.0, 1.0);
    }
";

static TILE_DEBUG_BORDER_COLOR: Color = Color { r: 0., g: 1., b: 1., a: 1.0 };
static TILE_DEBUG_BORDER_THICKNESS: usize = 1;
static LAYER_DEBUG_BORDER_COLOR: Color = Color { r: 1., g: 0.5, b: 0., a: 1.0 };
static LAYER_DEBUG_BORDER_THICKNESS: usize = 2;
static LAYER_AABB_DEBUG_BORDER_COLOR: Color = Color { r: 1., g: 0.0, b: 0., a: 1.0 };
static LAYER_AABB_DEBUG_BORDER_THICKNESS: usize = 1;

#[derive(Copy, Clone)]
struct Buffers {
    quad_vertex_buffer: GLuint,
    line_quad_vertex_buffer: GLuint,
}

#[derive(Copy, Clone)]
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
            panic!("Failed to compile shader program: {}", gl::get_program_info_log(id));
        }

        ShaderProgram {
            id: id,
        }
    }

    pub fn compile_shader(source_string: &str, shader_type: GLenum) -> GLuint {
        let id = gl::create_shader(shader_type);
        gl::shader_source(id, &[ source_string.as_bytes() ]);
        gl::compile_shader(id);
        if gl::get_shader_iv(id, gl::COMPILE_STATUS) == (0 as GLint) {
            panic!("Failed to compile shader: {}", gl::get_shader_info_log(id));
        }

        id
    }

    pub fn get_attribute_location(&self, name: &str) -> GLint {
        gl::get_attrib_location(self.id, name)
    }

    pub fn get_uniform_location(&self, name: &str) -> GLint {
        gl::get_uniform_location(self.id, name)
    }
}

#[derive(Copy, Clone)]
struct TextureProgram {
    program: ShaderProgram,
    vertex_position_attr: c_int,
    vertex_uv_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    sampler_uniform: c_int,
    texture_space_transform_uniform: c_int,
    opacity_uniform: c_int,
}

impl TextureProgram {
    fn new(sampler_function: &str, sampler_type: &str) -> TextureProgram {
        let fragment_shader_source
             = fmt::format(format_args!("#define samplerFunction {}\n#define samplerType {}\n{}",
                                        sampler_function,
                                        sampler_type,
                                        TEXTURE_FRAGMENT_SHADER_SOURCE));
        let program = ShaderProgram::new(TEXTURE_VERTEX_SHADER_SOURCE, &fragment_shader_source);
        TextureProgram {
            program: program,
            vertex_position_attr: program.get_attribute_location("aVertexPosition"),
            vertex_uv_attr: program.get_attribute_location("aVertexUv"),
            modelview_uniform: program.get_uniform_location("uMVMatrix"),
            projection_uniform: program.get_uniform_location("uPMatrix"),
            sampler_uniform: program.get_uniform_location("uSampler"),
            texture_space_transform_uniform: program.get_uniform_location("uTextureSpaceTransform"),
            opacity_uniform: program.get_uniform_location("uOpacity"),
        }
    }

    fn bind_uniforms_and_attributes(&self,
                                    vertices: &[TextureVertex; 4],
                                    transform: &Matrix4,
                                    projection_matrix: &Matrix4,
                                    texture_space_transform: &Matrix4,
                                    buffers: &Buffers,
                                    opacity: f32) {
        gl::uniform_1i(self.sampler_uniform, 0);
        gl::uniform_matrix_4fv(self.modelview_uniform, false, &transform.to_array());
        gl::uniform_matrix_4fv(self.projection_uniform, false, &projection_matrix.to_array());

        let vertex_size = mem::size_of::<TextureVertex>();

        gl::bind_buffer(gl::ARRAY_BUFFER, buffers.quad_vertex_buffer);
        gl::buffer_data(gl::ARRAY_BUFFER, vertices, gl::DYNAMIC_DRAW);
        gl::vertex_attrib_pointer_f32(self.vertex_position_attr as GLuint, 2, false, vertex_size as i32, 0);
        gl::vertex_attrib_pointer_f32(self.vertex_uv_attr as GLuint, 2, false, vertex_size as i32, 8);

        gl::uniform_matrix_4fv(self.texture_space_transform_uniform,
                               false,
                               &texture_space_transform.to_array());

        gl::uniform_1f(self.opacity_uniform, opacity);
    }

    fn enable_attribute_arrays(&self) {
        gl::enable_vertex_attrib_array(self.vertex_position_attr as GLuint);
        gl::enable_vertex_attrib_array(self.vertex_uv_attr as GLuint);
    }

    fn disable_attribute_arrays(&self) {
        gl::disable_vertex_attrib_array(self.vertex_uv_attr as GLuint);
        gl::disable_vertex_attrib_array(self.vertex_position_attr as GLuint);
    }

    fn create_2d_program() -> TextureProgram {
        TextureProgram::new("texture2D", "sampler2D")
    }

    #[cfg(target_os="macos")]
    fn create_rectangle_program_if_necessary() -> Option<TextureProgram> {
        gl::enable(gl::TEXTURE_RECTANGLE_ARB);
        Some(TextureProgram::new("texture2DRect", "sampler2DRect"))
    }

    #[cfg(not(target_os="macos"))]
    fn create_rectangle_program_if_necessary() -> Option<TextureProgram> {
        None
    }
}

#[derive(Copy, Clone)]
struct SolidColorProgram {
    program: ShaderProgram,
    vertex_position_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    color_uniform: c_int,
}

impl SolidColorProgram {
    fn new() -> SolidColorProgram {
        let program = ShaderProgram::new(SOLID_COLOR_VERTEX_SHADER_SOURCE,
                                         SOLID_COLOR_FRAGMENT_SHADER_SOURCE);
        SolidColorProgram {
            program: program,
            vertex_position_attr: program.get_attribute_location("aVertexPosition"),
            modelview_uniform: program.get_uniform_location("uMVMatrix"),
            projection_uniform: program.get_uniform_location("uPMatrix"),
            color_uniform: program.get_uniform_location("uColor"),
        }
    }

    fn bind_uniforms_and_attributes_common(&self,
                                           transform: &Matrix4,
                                           projection_matrix: &Matrix4,
                                           color: &Color) {
        gl::uniform_matrix_4fv(self.modelview_uniform, false, &transform.to_array());
        gl::uniform_matrix_4fv(self.projection_uniform, false, &projection_matrix.to_array());
        gl::uniform_4f(self.color_uniform,
                   color.r as GLfloat,
                   color.g as GLfloat,
                   color.b as GLfloat,
                   color.a as GLfloat);
    }

    fn bind_uniforms_and_attributes_for_lines(&self,
                                              vertices: &[ColorVertex; 5],
                                              transform: &Matrix4,
                                              projection_matrix: &Matrix4,
                                              buffers: &Buffers,
                                              color: &Color) {
        self.bind_uniforms_and_attributes_common(transform, projection_matrix, color);

        gl::bind_buffer(gl::ARRAY_BUFFER, buffers.line_quad_vertex_buffer);
        gl::buffer_data(gl::ARRAY_BUFFER, vertices, gl::DYNAMIC_DRAW);
        gl::vertex_attrib_pointer_f32(self.vertex_position_attr as GLuint, 2, false, 0, 0);
    }

    fn bind_uniforms_and_attributes_for_quad(&self,
                                             vertices: &[ColorVertex; 4],
                                             transform: &Matrix4,
                                             projection_matrix: &Matrix4,
                                             buffers: &Buffers,
                                             color: &Color) {
        self.bind_uniforms_and_attributes_common(transform, projection_matrix, color);

        gl::bind_buffer(gl::ARRAY_BUFFER, buffers.quad_vertex_buffer);
        gl::buffer_data(gl::ARRAY_BUFFER, vertices, gl::DYNAMIC_DRAW);
        gl::vertex_attrib_pointer_f32(self.vertex_position_attr as GLuint, 2, false, 0, 0);
    }

    fn enable_attribute_arrays(&self) {
        gl::enable_vertex_attrib_array(self.vertex_position_attr as GLuint);
    }

    fn disable_attribute_arrays(&self) {
        gl::disable_vertex_attrib_array(self.vertex_position_attr as GLuint);
    }
}

struct RenderContextChild<T> {
    layer: Option<Rc<Layer<T>>>,
    context: Option<RenderContext3D<T>>,
    paint_order: usize,
    z_center: f32,
}

pub struct RenderContext3D<T>{
    children: Vec<RenderContextChild<T>>,
    clip_rect: Option<Rect<f32>>,
}

impl<T> RenderContext3D<T> {
    fn new(layer: Rc<Layer<T>>) -> RenderContext3D<T> {
        let mut render_context = RenderContext3D {
            children: vec!(),
            clip_rect: RenderContext3D::calculate_context_clip(layer.clone(), None),
        };
        layer.build(&mut render_context);
        render_context.sort_children();
        render_context
    }

    fn build_child(layer: Rc<Layer<T>>,
                   parent_clip_rect: Option<Rect<f32>>)
                   -> Option<RenderContext3D<T>> {
        let clip_rect = RenderContext3D::calculate_context_clip(layer.clone(), parent_clip_rect);
        if let Some(ref clip_rect) = clip_rect {
            if clip_rect.is_empty() {
                return None;
            }
        }

        let mut render_context = RenderContext3D {
            children: vec!(),
            clip_rect: clip_rect,
        };

        for child in layer.children().iter() {
            child.build(&mut render_context);
        }

        render_context.sort_children();
        Some(render_context)
    }

    fn sort_children(&mut self) {
        // TODO(gw): This is basically what FF does, which breaks badly
        // when there are intersecting polygons. Need to split polygons
        // to handle this case correctly (Blink uses a BSP tree).
        self.children.sort_by(|a, b| {
            if a.z_center < b.z_center {
                Ordering::Less
            } else if a.z_center > b.z_center {
                Ordering::Greater
            } else if a.paint_order < b.paint_order {
                Ordering::Less
            } else if a.paint_order > b.paint_order {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        });
    }

    fn calculate_context_clip(layer: Rc<Layer<T>>,
                              parent_clip_rect: Option<Rect<f32>>)
                              -> Option<Rect<f32>> {
        // TODO(gw): This doesn't work for iframes that are transformed.
        if !*layer.masks_to_bounds.borrow() {
            return parent_clip_rect;
        }

        let layer_clip = match layer.transform_state.borrow().screen_rect.as_ref() {
            Some(screen_rect) => screen_rect.rect,
            None => return Some(Rect::zero()), // Layer is entirely clipped away.
        };

        match parent_clip_rect {
            Some(parent_clip_rect) => match layer_clip.intersection(&parent_clip_rect) {
                Some(intersected_clip) => Some(intersected_clip),
                None => Some(Rect::zero()), // No intersection.
            },
            None => Some(layer_clip),
        }
    }

    fn add_child(&mut self,
                 layer: Option<Rc<Layer<T>>>,
                 child_context: Option<RenderContext3D<T>>,
                 z_center: f32) {
        let paint_order = self.children.len();
        self.children.push(RenderContextChild {
            layer: layer,
            context: child_context,
            z_center: z_center,
            paint_order: paint_order,
        });
    }
}

pub trait RenderContext3DBuilder<T> {
    fn build(&self, current_context: &mut RenderContext3D<T>);
}

impl<T> RenderContext3DBuilder<T> for Rc<Layer<T>> {
    fn build(&self, current_context: &mut RenderContext3D<T>) {
        let (layer, z_center) = match self.transform_state.borrow().screen_rect {
            Some(ref rect) => (Some(self.clone()), rect.z_center),
            None => (None, 0.), // Layer is entirely clipped.
        };

        if !self.children.borrow().is_empty() && self.establishes_3d_context {
            let child_context =
                RenderContext3D::build_child(self.clone(), current_context.clip_rect);
            if child_context.is_some() {
                current_context.add_child(layer, child_context, z_center);
                return;
            }
        };

        // If we are completely clipped out, don't add anything to this context.
        if layer.is_none() {
            return;
        }

        current_context.add_child(layer, None, z_center);

        for child in self.children().iter() {
            child.build(current_context);
        }
    }
}

#[derive(Copy, Clone)]
pub struct RenderContext {
    texture_2d_program: TextureProgram,
    texture_rectangle_program: Option<TextureProgram>,
    solid_color_program: SolidColorProgram,
    buffers: Buffers,

    /// The platform-specific graphics context.
    compositing_display: NativeDisplay,

    /// Whether to show lines at border and tile boundaries for debugging purposes.
    show_debug_borders: bool,

    force_near_texture_filter: bool,
}

impl RenderContext {
    pub fn new(compositing_display: NativeDisplay,
               show_debug_borders: bool,
               force_near_texture_filter: bool) -> RenderContext {
        gl::enable(gl::TEXTURE_2D);

        // Each layer uses premultiplied alpha!
        gl::enable(gl::BLEND);
        gl::blend_func(gl::ONE, gl::ONE_MINUS_SRC_ALPHA);

        let texture_2d_program = TextureProgram::create_2d_program();
        let solid_color_program = SolidColorProgram::new();
        let texture_rectangle_program = TextureProgram::create_rectangle_program_if_necessary();

        RenderContext {
            texture_2d_program: texture_2d_program,
            texture_rectangle_program: texture_rectangle_program,
            solid_color_program: solid_color_program,
            buffers: RenderContext::init_buffers(),
            compositing_display: compositing_display,
            show_debug_borders: show_debug_borders,
            force_near_texture_filter: force_near_texture_filter,
        }
    }

    fn init_buffers() -> Buffers {
        let quad_vertex_buffer = gl::gen_buffers(1)[0];
        gl::bind_buffer(gl::ARRAY_BUFFER, quad_vertex_buffer);

        let line_quad_vertex_buffer = gl::gen_buffers(1)[0];
        gl::bind_buffer(gl::ARRAY_BUFFER, line_quad_vertex_buffer);

        Buffers {
            quad_vertex_buffer: quad_vertex_buffer,
            line_quad_vertex_buffer: line_quad_vertex_buffer,
        }
    }

    fn bind_and_render_solid_quad(&self,
                                  vertices: &[ColorVertex; 4],
                                  transform: &Matrix4,
                                  projection: &Matrix4,
                                  color: &Color) {
        self.solid_color_program.enable_attribute_arrays();
        gl::use_program(self.solid_color_program.program.id);
        self.solid_color_program.bind_uniforms_and_attributes_for_quad(vertices,
                                                                       transform,
                                                                       projection,
                                                                       &self.buffers,
                                                                       color);
        gl::draw_arrays(gl::TRIANGLE_STRIP, 0, 4);
        self.solid_color_program.disable_attribute_arrays();
    }

    fn bind_and_render_quad(&self,
                            vertices: &[TextureVertex; 4],
                            texture: &Texture,
                            transform: &Matrix4,
                            projection_matrix: &Matrix4,
                            opacity: f32) {
        let mut texture_coordinates_need_to_be_scaled_by_size = false;
        let program = match texture.target {
            TextureTarget2D => self.texture_2d_program,
            TextureTargetRectangle => match self.texture_rectangle_program {
                Some(program) => {
                    texture_coordinates_need_to_be_scaled_by_size = true;
                    program
                }
                None => panic!("There is no shader program for texture rectangle"),
            },
        };
        program.enable_attribute_arrays();

        gl::use_program(program.program.id);
        gl::active_texture(gl::TEXTURE0);
        gl::bind_texture(texture.target.as_gl_target(), texture.native_texture());

        let filter_mode = if self.force_near_texture_filter {
            gl::NEAREST
        } else {
            gl::LINEAR
        } as GLint;
        gl::tex_parameter_i(texture.target.as_gl_target(), gl::TEXTURE_MAG_FILTER, filter_mode);
        gl::tex_parameter_i(texture.target.as_gl_target(), gl::TEXTURE_MIN_FILTER, filter_mode);

        // We calculate a transformation matrix for the texture coordinates
        // which is useful for flipping the texture vertically or scaling the
        // coordinates when dealing with GL_ARB_texture_rectangle.
        let mut texture_transform = Matrix4::identity();
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

        program.bind_uniforms_and_attributes(vertices,
                                             transform,
                                             &projection_matrix,
                                             &texture_transform,
                                             &self.buffers,
                                             opacity);

        // Draw!
        gl::draw_arrays(gl::TRIANGLE_STRIP, 0, 4);
        gl::bind_texture(gl::TEXTURE_2D, 0);

        gl::bind_texture(texture.target.as_gl_target(), 0);
        program.disable_attribute_arrays()
    }

    pub fn bind_and_render_quad_lines(&self,
                                      vertices: &[ColorVertex; 5],
                                      transform: &Matrix4,
                                      projection: &Matrix4,
                                      color: &Color,
                                      line_thickness: usize) {
        self.solid_color_program.enable_attribute_arrays();
        gl::use_program(self.solid_color_program.program.id);
        self.solid_color_program.bind_uniforms_and_attributes_for_lines(vertices,
                                                                        transform,
                                                                        projection,
                                                                        &self.buffers,
                                                                        color);
        gl::line_width(line_thickness as GLfloat);
        gl::draw_arrays(gl::LINE_STRIP, 0, 5);
        self.solid_color_program.disable_attribute_arrays();
    }

    fn render_layer<T>(&self,
                       layer: Rc<Layer<T>>,
                       transform: &Matrix4,
                       projection: &Matrix4,
                       clip_rect: Option<Rect<f32>>,
                       gfx_context: &NativeDisplay) {
        let ts = layer.transform_state.borrow();
        let transform = transform.mul(&ts.final_transform);
        let background_color = *layer.background_color.borrow();

        // Create native textures for this layer
        layer.create_textures(gfx_context);

        let layer_rect = clip_rect.map_or(ts.world_rect, |clip_rect| {
            match clip_rect.intersection(&ts.world_rect) {
                Some(layer_rect) => layer_rect,
                None => Rect::zero(),
            }
        });

        if layer_rect.is_empty() {
            return;
        }

        if background_color.a != 0.0 {
            let bg_vertices = [
                ColorVertex::new(layer_rect.origin),
                ColorVertex::new(layer_rect.top_right()),
                ColorVertex::new(layer_rect.bottom_left()),
                ColorVertex::new(layer_rect.bottom_right()),
            ];

            self.bind_and_render_solid_quad(&bg_vertices,
                                            &transform,
                                            &projection,
                                            &background_color);
        }

        layer.do_for_all_tiles(|tile: &Tile| {
           self.render_tile(tile,
                            &ts.world_rect.origin,
                            &transform,
                            projection,
                            clip_rect,
                            *layer.opacity.borrow());
        });

        if self.show_debug_borders {
            let debug_vertices = [
                ColorVertex::new(layer_rect.origin),
                ColorVertex::new(layer_rect.top_right()),
                ColorVertex::new(layer_rect.bottom_right()),
                ColorVertex::new(layer_rect.bottom_left()),
                ColorVertex::new(layer_rect.origin),
            ];
            self.bind_and_render_quad_lines(&debug_vertices,
                                            &transform,
                                            projection,
                                            &LAYER_DEBUG_BORDER_COLOR,
                                            LAYER_DEBUG_BORDER_THICKNESS);

            let aabb = ts.screen_rect.as_ref().unwrap().rect;
            let debug_vertices = [
                ColorVertex::new(aabb.origin),
                ColorVertex::new(aabb.top_right()),
                ColorVertex::new(aabb.bottom_right()),
                ColorVertex::new(aabb.bottom_left()),
                ColorVertex::new(aabb.origin),
            ];
            self.bind_and_render_quad_lines(&debug_vertices,
                                            &Matrix4::identity(),
                                            projection,
                                            &LAYER_AABB_DEBUG_BORDER_COLOR,
                                            LAYER_AABB_DEBUG_BORDER_THICKNESS);
        }
    }

    fn render_tile(&self,
                   tile: &Tile,
                   layer_origin: &Point2D<f32>,
                   transform: &Matrix4,
                   projection: &Matrix4,
                   clip_rect: Option<Rect<f32>>,
                   opacity: f32) {
        if tile.texture.is_zero() || !tile.bounds.is_some() {
            return;
        }

        let tile_rect = tile.bounds.unwrap().to_untyped().translate(layer_origin);
        let clipped_tile_rect = clip_rect.map_or(tile_rect, |clip_rect| {
            match clip_rect.intersection(&tile_rect) {
                Some(clipped_tile_rect) => clipped_tile_rect,
                None => Rect::zero(),
            }
        });

        if clipped_tile_rect.is_empty() {
           return;
        }

        let texture_rect_origin = clipped_tile_rect.origin - tile_rect.origin;
        let texture_rect = Rect::new(
            Point2D::new(texture_rect_origin.x / tile_rect.size.width,
                         texture_rect_origin.y / tile_rect.size.height),
            Size2D::new(clipped_tile_rect.size.width / tile_rect.size.width,
                        clipped_tile_rect.size.height / tile_rect.size.height));

        let tile_vertices: [TextureVertex; 4] = [
            TextureVertex::new(clipped_tile_rect.origin, texture_rect.origin),
            TextureVertex::new(clipped_tile_rect.top_right(), texture_rect.top_right()),
            TextureVertex::new(clipped_tile_rect.bottom_left(), texture_rect.bottom_left()),
            TextureVertex::new(clipped_tile_rect.bottom_right(), texture_rect.bottom_right()),
        ];

        if self.show_debug_borders {
            let debug_vertices = [
                // The weird ordering is converting from triangle-strip into a line-strip.
                ColorVertex::new(clipped_tile_rect.origin),
                ColorVertex::new(clipped_tile_rect.top_right()),
                ColorVertex::new(clipped_tile_rect.bottom_right()),
                ColorVertex::new(clipped_tile_rect.bottom_left()),
                ColorVertex::new(clipped_tile_rect.origin),
            ];
            self.bind_and_render_quad_lines(&debug_vertices,
                                            &transform,
                                            projection,
                                            &TILE_DEBUG_BORDER_COLOR,
                                            TILE_DEBUG_BORDER_THICKNESS);
        }

        self.bind_and_render_quad(&tile_vertices,
                                  &tile.texture,
                                  &transform,
                                  projection,
                                  opacity);
    }

    fn render_3d_context<T>(&self,
                            context: &RenderContext3D<T>,
                            transform: &Matrix4,
                            projection: &Matrix4,
                            gfx_context: &NativeDisplay) {
        if context.children.is_empty() {
            return;
        }

        // Clear the z-buffer for each 3d render context
        // TODO(gw): Potential optimization here if there are no
        //           layer intersections to disable z-buffering and
        //           avoid clear.
        gl::clear(gl::DEPTH_BUFFER_BIT);

        // Render child layers with z-testing.
        for child in &context.children {
            if let Some(ref layer) = child.layer {
                // TODO(gw): Disable clipping on 3d layers for now.
                // Need to implement proper polygon clipping to
                // make this work correctly.
                let clip_rect = context.clip_rect.and_then(|cr| {
                    let m = layer.transform_state.borrow().final_transform;

                    // See https://drafts.csswg.org/css-transforms/#2d-matrix
                    let is_3d_transform = m.m31 != 0.0 || m.m32 != 0.0 ||
                                          m.m13 != 0.0 || m.m23 != 0.0 ||
                                          m.m43 != 0.0 || m.m14 != 0.0 ||
                                          m.m24 != 0.0 || m.m34 != 0.0 ||
                                          m.m33 != 1.0 || m.m44 != 1.0;

                    if is_3d_transform {
                        None
                    } else {
                        // If the transform is 2d, invert it and back-transform
                        // the clip rect into world space.
                        let transform = m.invert();
                        let xform_2d = Matrix2D::new(transform.m11, transform.m12,
                                                     transform.m21, transform.m22,
                                                     transform.m41, transform.m42);
                        Some(xform_2d.transform_rect(&cr))
                    }

                });
                self.render_layer(layer.clone(),
                                  transform,
                                  projection,
                                  clip_rect,
                                  gfx_context);
            }

            if let Some(ref context) = child.context {
                self.render_3d_context(context,
                                       transform,
                                       projection,
                                       gfx_context);

            }
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

    // Enable depth testing for 3d transforms. Set z-mode to LESS-EQUAL
    // so that layers with equal Z are able to paint correctly in
    // the order they are specified.
    gl::enable(gl::DEPTH_TEST);
    gl::clear_color(1.0, 1.0, 1.0, 1.0);
    gl::clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
    gl::depth_func(gl::LEQUAL);

    // Set up the initial modelview matrix.
    let transform = Matrix4::identity().scale(scene.scale.get(), scene.scale.get(), 1.0);
    let projection = create_ortho(&scene.viewport.size.to_untyped());

    // Build the list of render items
    render_context.render_3d_context(&RenderContext3D::new(root_layer.clone()),
                                     &transform,
                                     &projection,
                                     &render_context.compositing_display);
}

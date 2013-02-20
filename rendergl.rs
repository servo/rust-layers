use layers;
use layers::{ARGB32Format, ContainerLayerKind, Image, ImageLayerKind, RGB24Format};
use layers::{TiledImageLayerKind};
use scene::Scene;

use geom::matrix::{Matrix4, ortho};
use geom::size::Size2D;
use opengles::gl2;
use opengles::gl2::{ARRAY_BUFFER, COLOR_BUFFER_BIT, CLAMP_TO_EDGE, COMPILE_STATUS};
use opengles::gl2::{FRAGMENT_SHADER, LINEAR, LINK_STATUS, NEAREST, NO_ERROR, REPEAT, RGB, RGBA,
                      BGRA};
use opengles::gl2::{STATIC_DRAW, TEXTURE_2D, TEXTURE_MAG_FILTER, TEXTURE_MIN_FILTER};
use opengles::gl2::{TEXTURE_RECTANGLE_ARB, TEXTURE_WRAP_S, TEXTURE_WRAP_T};
use opengles::gl2::{TRIANGLE_STRIP, UNPACK_ALIGNMENT, UNPACK_CLIENT_STORAGE_APPLE, UNSIGNED_BYTE};
use opengles::gl2::{UNPACK_ROW_LENGTH, UNSIGNED_BYTE, UNSIGNED_INT_8_8_8_8_REV, VERTEX_SHADER};
use opengles::gl2::{GLclampf, GLenum, GLint, GLsizei, GLuint, attach_shader, bind_buffer};
use opengles::gl2::{bind_texture, buffer_data, create_program, clear, clear_color};
use opengles::gl2::{compile_shader, create_shader, draw_arrays, enable};
use opengles::gl2::{enable_vertex_attrib_array, gen_buffers, gen_textures};
use opengles::gl2::{get_attrib_location, get_error, get_program_iv};
use opengles::gl2::{get_shader_info_log, get_shader_iv};
use opengles::gl2::{get_uniform_location, link_program, pixel_store_i, shader_source};
use opengles::gl2::{tex_image_2d, tex_parameter_i, uniform_1i, uniform_matrix_4fv, use_program};
use opengles::gl2::{vertex_attrib_pointer_f32, viewport};

use io::println;
use libc::c_int;
use str::to_bytes;

pub fn FRAGMENT_SHADER_SOURCE() -> ~str {
    ~"
        #ifdef GLES2
            precision mediump float;
        #endif

        varying vec2 vTextureCoord;

        uniform sampler2DRect uSampler;

        void main(void) {
            gl_FragColor = texture2DRect(uSampler, vec2(vTextureCoord.s, vTextureCoord.t));
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
    shader_source(shader_id, ~[ to_bytes(source_string) ]);
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
    texture_coord_buffer: GLuint,
}

pub fn RenderContext(program: GLuint) -> RenderContext {
    let (vertex_buffer, texture_coord_buffer) = init_buffers();
    let rc = RenderContext {
        program: program,
        vertex_position_attr: get_attrib_location(program, ~"aVertexPosition"),
        texture_coord_attr: get_attrib_location(program, ~"aTextureCoord"),
        modelview_uniform: get_uniform_location(program, ~"uMVMatrix"),
        projection_uniform: get_uniform_location(program, ~"uPMatrix"),
        sampler_uniform: get_uniform_location(program, ~"uSampler"),
        vertex_buffer: vertex_buffer,
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
    enable(TEXTURE_RECTANGLE_ARB);

    return RenderContext(program);
}

pub fn init_buffers() -> (GLuint, GLuint) {
    let triangle_vertex_buffer = gen_buffers(1 as GLsizei)[0];
    bind_buffer(ARRAY_BUFFER, triangle_vertex_buffer);

    let (_0, _1) = (0.0f32, 1.0f32);
    let vertices = ~[
        _0, _0, _0,
        _0, _1, _0,
        _1, _0, _0,
        _1, _1, _0
    ];

    buffer_data(ARRAY_BUFFER, vertices, STATIC_DRAW);

    let texture_coord_buffer = gen_buffers(1 as GLsizei)[0];
    bind_buffer(ARRAY_BUFFER, texture_coord_buffer);

    return (triangle_vertex_buffer, texture_coord_buffer);
}

pub fn create_texture_for_image_if_necessary(image: @Image) {
    match image.texture {
        None => {}
        Some(_) => { return; /* Nothing to do. */ }
    }

    let texture = gen_textures(1 as GLsizei)[0];

    debug!("making texture, id=%d, format=%?", texture as int, image.data.format());

    bind_texture(TEXTURE_RECTANGLE_ARB, texture);

    // FIXME: This makes the lifetime requirements somewhat complex...
    pixel_store_i(UNPACK_CLIENT_STORAGE_APPLE, 1);

    let size = image.data.size();
    let stride = image.data.stride() as GLsizei;

    tex_parameter_i(TEXTURE_RECTANGLE_ARB, TEXTURE_MAG_FILTER, NEAREST as GLint);
    tex_parameter_i(TEXTURE_RECTANGLE_ARB, TEXTURE_MIN_FILTER, NEAREST as GLint);

    tex_parameter_i(TEXTURE_RECTANGLE_ARB, TEXTURE_WRAP_S, CLAMP_TO_EDGE as GLint);
    tex_parameter_i(TEXTURE_RECTANGLE_ARB, TEXTURE_WRAP_T, CLAMP_TO_EDGE as GLint);

    // These two are needed for DMA on the Mac. Don't touch them unless you know what you're doing!
    pixel_store_i(UNPACK_ALIGNMENT, 4);
    pixel_store_i(UNPACK_ROW_LENGTH, stride);
    if stride % 32 != 0 {
        info!("rust-layers: suggest using stride multiples of 32 for DMA on the Mac");
    }

    debug!("rust-layers stride is %u", stride as uint);

    match image.data.format() {
        RGB24Format => {
            do image.data.with_data |data| {
                tex_image_2d(TEXTURE_RECTANGLE_ARB, 0 as GLint, RGB as GLint,
                             size.width as GLsizei, size.height as GLsizei, 0 as GLint, RGB,
                             UNSIGNED_BYTE, Some(data));
            }
        }
        ARGB32Format => {
            do image.data.with_data |data| {
                debug!("(rust-layers) data size=%u expected size=%u",
                      data.len(), ((stride as uint) * size.height * 4) as uint);

                tex_parameter_i(TEXTURE_RECTANGLE_ARB, gl2::TEXTURE_STORAGE_HINT_APPLE,
                                gl2::STORAGE_CACHED_APPLE as GLint);

                tex_image_2d(TEXTURE_RECTANGLE_ARB, 0 as GLint, RGBA as GLint,
                             size.width as GLsizei, size.height as GLsizei, 0 as GLint, BGRA,
                             UNSIGNED_INT_8_8_8_8_REV, Some(data));
            }
        }
    }

    bind_texture(TEXTURE_RECTANGLE_ARB, 0);

    image.texture = Some(texture);
}

pub fn bind_and_render_quad(render_context: RenderContext, size: Size2D<uint>, texture: GLuint) {
    bind_texture(TEXTURE_RECTANGLE_ARB, texture);

    uniform_1i(render_context.sampler_uniform, 0);

    bind_buffer(ARRAY_BUFFER, render_context.vertex_buffer);
    vertex_attrib_pointer_f32(render_context.vertex_position_attr as GLuint, 3, false, 0, 0);

    // Create the texture coordinate array.
    bind_buffer(ARRAY_BUFFER, render_context.texture_coord_buffer);

    let (width, height) = (size.width as f32, size.height as f32);
    let vertices = [
        0.0f32, 0.0f32,
        0.0f32, height,
        width,  0.0f32,
        width,  height
    ];
    buffer_data(ARRAY_BUFFER, vertices, STATIC_DRAW);

    vertex_attrib_pointer_f32(render_context.texture_coord_attr as GLuint, 2, false, 0, 0);

    draw_arrays(TRIANGLE_STRIP, 0, 4);

    bind_texture(TEXTURE_RECTANGLE_ARB, 0);
}

// Layer rendering

pub trait Render {
    fn render(@self, render_context: RenderContext, transform: Matrix4<f32>);
}

impl Render for layers::ContainerLayer {
    fn render(@self, render_context: RenderContext, transform: Matrix4<f32>) {
        for self.each_child |child| {
            render_layer(render_context, transform, child);
        }
    }
}

impl Render for layers::ImageLayer {
    fn render(@self, render_context: RenderContext, transform: Matrix4<f32>) {
        create_texture_for_image_if_necessary(self.image);

        let transform = transform.mul(&self.common.transform);
        uniform_matrix_4fv(render_context.modelview_uniform, false, transform.to_array());

        bind_and_render_quad(
            render_context, self.image.data.size(), option::get(self.image.texture));
    }
}

impl Render for layers::TiledImageLayer {
    fn render(@self, render_context: RenderContext, transform: Matrix4<f32>) {
        let tiles_down = self.tiles.len() / self.tiles_across;
        for self.tiles.eachi |i, tile| {
            create_texture_for_image_if_necessary(*tile);

            let x = ((i % self.tiles_across) as f32);
            let y = ((i / self.tiles_across) as f32);

            let transform = transform.mul(&self.common.transform);
            let transform = transform.scale(1.0 / (self.tiles_across as f32),
                                            1.0 / (tiles_down as f32),
                                            1.0);
            let transform = transform.translate(x, y, 0.0);

            uniform_matrix_4fv(render_context.modelview_uniform, false, transform.to_array());

            bind_and_render_quad(render_context, tile.data.size(), option::get(tile.texture));
        }
    }
}

fn render_layer(render_context: RenderContext, transform: Matrix4<f32>, layer: layers::Layer) {
    match layer {
        ContainerLayerKind(container_layer) => {
            container_layer.render(render_context, transform);
        }
        ImageLayerKind(image_layer) => {
            image_layer.render(render_context, transform);
        }
        TiledImageLayerKind(tiled_image_layer) => {
            tiled_image_layer.render(render_context, transform);
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


use layers::{ARGB32Format, ContainerLayerKind, Image, ImageLayerKind, RGB24Format};
use layers::{TiledImageLayerKind};
use scene::Scene;

use geom::matrix::{Matrix4, ortho};
use opengles::gl2::{ARRAY_BUFFER, COLOR_BUFFER_BIT, COMPILE_STATUS};
use opengles::gl2::{FRAGMENT_SHADER, LINEAR, LINK_STATUS, NEAREST, NO_ERROR, REPEAT, RGB, RGBA,
                      BGRA};
use opengles::gl2::{STATIC_DRAW, TEXTURE_2D, TEXTURE_MAG_FILTER, TEXTURE_MIN_FILTER};
use opengles::gl2::{TEXTURE_WRAP_S, TEXTURE_WRAP_T};
use opengles::gl2::{TRIANGLE_STRIP, UNPACK_ALIGNMENT, UNSIGNED_BYTE, VERTEX_SHADER, GLclampf};
use opengles::gl2::{GLenum, GLint, GLsizei, GLuint, attach_shader, bind_buffer, bind_texture};
use opengles::gl2::{buffer_data, create_program, clear, clear_color};
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

fn FRAGMENT_SHADER_SOURCE() -> ~str {
    ~"
        #ifdef GLES2
            precision mediump float;
        #endif

        varying vec2 vTextureCoord;

        uniform sampler2D uSampler;

        void main(void) {
            gl_FragColor = texture2D(uSampler, vec2(vTextureCoord.s, vTextureCoord.t));
        }
    "
}

fn VERTEX_SHADER_SOURCE() -> ~str {
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

fn load_shader(source_string: ~str, shader_type: GLenum) -> GLuint {
    let shader_id = create_shader(shader_type);
    shader_source(shader_id, ~[ to_bytes(source_string) ]);
    compile_shader(shader_id);

    if get_error() != NO_ERROR {
        println(#fmt("error: %d", get_error() as int));
        fail ~"failed to compile shader";
    }

    if get_shader_iv(shader_id, COMPILE_STATUS) == (0 as GLint) {
        println(#fmt("shader info log: %s", get_shader_info_log(shader_id)));
        fail ~"failed to compile shader";
    }

    return shader_id;
}

struct RenderContext {
    program: GLuint,
    vertex_position_attr: c_int,
    texture_coord_attr: c_int,
    modelview_uniform: c_int,
    projection_uniform: c_int,
    sampler_uniform: c_int,
    vertex_buffer: GLuint,
    texture_coord_buffer: GLuint,
}

fn RenderContext(program: GLuint) -> RenderContext {
    let (vertex_buffer, texture_coord_buffer) = init_buffers();
    let rc = RenderContext {
        program : program,
        vertex_position_attr : get_attrib_location(program, ~"aVertexPosition"),
        texture_coord_attr : get_attrib_location(program, ~"aTextureCoord"),
        modelview_uniform : get_uniform_location(program, ~"uMVMatrix"),
        projection_uniform : get_uniform_location(program, ~"uPMatrix"),
        sampler_uniform : get_uniform_location(program, ~"uSampler"),
        vertex_buffer : vertex_buffer,
        texture_coord_buffer : texture_coord_buffer,
    };

    enable_vertex_attrib_array(rc.vertex_position_attr as GLuint);
    enable_vertex_attrib_array(rc.texture_coord_attr as GLuint);

    rc
}

fn init_render_context() -> RenderContext {
    let vertex_shader = load_shader(VERTEX_SHADER_SOURCE(), VERTEX_SHADER);
    let fragment_shader = load_shader(FRAGMENT_SHADER_SOURCE(), FRAGMENT_SHADER);

    let program = create_program();
    attach_shader(program, vertex_shader);
    attach_shader(program, fragment_shader);
    link_program(program);

    if get_program_iv(program, LINK_STATUS) == (0 as GLint) {
        fail ~"failed to initialize program";
    }

    use_program(program);

    enable(TEXTURE_2D);

    return RenderContext(program);
}

fn init_buffers() -> (GLuint, GLuint) {
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

    let vertices = ~[
        _0, _0,
        _0, _1,
        _1, _0,
        _1, _1
    ];

    buffer_data(ARRAY_BUFFER, vertices, STATIC_DRAW);

    return (triangle_vertex_buffer, texture_coord_buffer);
}

fn create_texture_for_image_if_necessary(image: @Image) {
    match image.texture {
        None => {}
        Some(_) => { return; /* Nothing to do. */ }
    }

    #debug("making texture");

    let texture = gen_textures(1 as GLsizei)[0];
    bind_texture(TEXTURE_2D, texture);

    tex_parameter_i(TEXTURE_2D, TEXTURE_WRAP_S, REPEAT as GLint);
    tex_parameter_i(TEXTURE_2D, TEXTURE_WRAP_T, REPEAT as GLint);
    tex_parameter_i(TEXTURE_2D, TEXTURE_MAG_FILTER, LINEAR as GLint);
    tex_parameter_i(TEXTURE_2D, TEXTURE_MIN_FILTER, LINEAR as GLint);

    pixel_store_i(UNPACK_ALIGNMENT, 1);

    fn borrow(a: &a/[u8]) -> &a/[u8] { a }

    match image.format {
      RGB24Format => {
        tex_image_2d(TEXTURE_2D, 0 as GLint, RGB as GLint, image.width as GLsizei,
                     image.height as GLsizei, 0 as GLint, RGB, UNSIGNED_BYTE, Some(borrow(image.data)));
      }
      ARGB32Format => {
        tex_image_2d(TEXTURE_2D, 0 as GLint, RGBA as GLint, image.width as GLsizei,
                     image.height as GLsizei, 0 as GLint, BGRA, UNSIGNED_BYTE, Some(borrow(image.data)));
      }
    }

    image.texture = Some(texture);
}

fn bind_and_render_quad(render_context: RenderContext, texture: GLuint) {
    bind_texture(TEXTURE_2D, texture);

    uniform_1i(render_context.sampler_uniform, 0);

    bind_buffer(ARRAY_BUFFER, render_context.vertex_buffer);
    vertex_attrib_pointer_f32(render_context.vertex_position_attr as GLuint, 3, false, 0, 0);

    bind_buffer(ARRAY_BUFFER, render_context.texture_coord_buffer);
    vertex_attrib_pointer_f32(render_context.texture_coord_attr as GLuint, 2, false, 0, 0);

    draw_arrays(TRIANGLE_STRIP, 0, 4);
}

// Layer rendering

trait Render {
    fn render(render_context: RenderContext);
}

impl @layers::ImageLayer : Render {
    fn render(render_context: RenderContext) {
        create_texture_for_image_if_necessary(self.image);

        uniform_matrix_4fv(render_context.modelview_uniform, false,
                           self.common.transform.to_array());

        bind_and_render_quad(render_context, option::get(self.image.texture));
    }
}

impl @layers::TiledImageLayer : Render {
    fn render(render_context: RenderContext) {
        let tiles_down = self.tiles.len() / self.tiles_across;
        for self.tiles.eachi |i, tile| {
            create_texture_for_image_if_necessary(tile);

            let x = ((i % self.tiles_across) as f32);
            let y = ((i / self.tiles_across) as f32);

            let transform = self.common.transform.scale(1.0f32 / (self.tiles_across as f32),
                                                        1.0f32 / (tiles_down as f32),
                                                        1.0f32);
            let transform = transform.translate(x * 1.0f32, y * 1.0f32, 0.0f32);

            uniform_matrix_4fv(render_context.modelview_uniform, false, transform.to_array());

            bind_and_render_quad(render_context, option::get(tile.texture));
        }
    }
}

fn render_scene(render_context: RenderContext, &scene: Scene) {
    // Set the viewport.
    viewport(0 as GLint, 0 as GLint, scene.size.width as GLsizei, scene.size.height as GLsizei);

    // Clear the screen.
    clear_color(0.0f32, 0.0f32, 1.0f32, 1.0f32);
    clear(COLOR_BUFFER_BIT);

    // Set the projection matrix.
    let projection_matrix = ortho(0.0f32, copy scene.size.width, copy scene.size.height, 0.0f32,
                                  -10.0f32, 10.0f32);
    uniform_matrix_4fv(render_context.projection_uniform, false, projection_matrix.to_array());

    match copy scene.root {
        ContainerLayerKind(*) => fail ~"container layers unsupported",
        ImageLayerKind(image_layer) => image_layer.render(render_context),
        TiledImageLayerKind(tiled_image_layer) => tiled_image_layer.render(render_context)
    }
}


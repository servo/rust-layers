import opengles::gl2::{ARRAY_BUFFER, COLOR_BUFFER_BIT, COMPILE_STATUS};
import opengles::gl2::{FRAGMENT_SHADER, LINEAR, LINK_STATUS, NEAREST, NO_ERROR, REPEAT, RGB, STATIC_DRAW};
import opengles::gl2::{TEXTURE_2D, TEXTURE_MAG_FILTER, TEXTURE_MIN_FILTER, TEXTURE_WRAP_S};
import opengles::gl2::{TEXTURE_WRAP_T, TRIANGLE_STRIP, UNSIGNED_BYTE, VERTEX_SHADER, GLclampf};
import opengles::gl2::{GLenum, GLint, GLsizei, GLuint, attach_shader, bind_buffer, bind_texture};
import opengles::gl2::{buffer_data, create_program, clear, clear_color};
import opengles::gl2::{compile_shader, create_shader, draw_arrays, enable};
import opengles::gl2::{enable_vertex_attrib_array, gen_buffers, gen_textures};
import opengles::gl2::{get_attrib_location, get_error, get_program_iv};
import opengles::gl2::{get_shader_info_log, get_shader_iv};
import opengles::gl2::{get_uniform_location, link_program, shader_source, tex_image_2d};
import opengles::gl2::{tex_parameter_i, uniform_1i, use_program, vertex_attrib_pointer_f32};

import io::println;
import libc::c_int;
import str::bytes;

fn FRAGMENT_SHADER_SOURCE() -> str {
    "
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

fn VERTEX_SHADER_SOURCE() -> str {
    "
        attribute vec3 aVertexPosition;
        attribute vec2 aTextureCoord;

        varying vec2 vTextureCoord;

        void main(void) {
            gl_Position = vec4(aVertexPosition, 1.0);
            vTextureCoord = aTextureCoord;
        }
    "
}

fn load_shader(source_string: str, shader_type: GLenum) -> GLuint {
    let shader_id = create_shader(shader_type);
    shader_source(shader_id, [ bytes(source_string) ]);
    compile_shader(shader_id);

    if get_error() != NO_ERROR {
        println(#fmt("error: %d", get_error() as int));
        fail "failed to compile shader";
    }

    if get_shader_iv(shader_id, COMPILE_STATUS) == (0 as GLint) {
        println(#fmt("shader info log: %s", get_shader_info_log(shader_id)));
        fail "failed to compile shader";
    }

    ret shader_id;
}

class RenderContext {
    let program: GLuint;
    let vertex_position_attr: c_int;
    let texture_coord_attr: c_int;
    let sampler_uniform: c_int;

    new(program: GLuint) {
        self.program = program;
        self.vertex_position_attr = get_attrib_location(program, "aVertexPosition");
        self.texture_coord_attr = get_attrib_location(program, "aTextureCoord");
        self.sampler_uniform = get_uniform_location(program, "uSampler");

        enable_vertex_attrib_array(self.vertex_position_attr as GLuint);
        enable_vertex_attrib_array(self.texture_coord_attr as GLuint);
    }
}

fn init_render_context() -> RenderContext {
    let vertex_shader = load_shader(VERTEX_SHADER_SOURCE(), VERTEX_SHADER);
    let fragment_shader = load_shader(FRAGMENT_SHADER_SOURCE(), FRAGMENT_SHADER);

    let program = create_program();
    attach_shader(program, vertex_shader);
    attach_shader(program, fragment_shader);
    link_program(program);

    if get_program_iv(program, LINK_STATUS) == (0 as GLint) {
        fail "failed to initialize program";
    }

    use_program(program);

    ret RenderContext(program);
}

fn init_buffers() -> (GLuint, GLuint) {
    let triangle_vertex_buffer = gen_buffers(1 as GLsizei)[0];
    bind_buffer(ARRAY_BUFFER, triangle_vertex_buffer);

    let (_0, _1) = (0.0f32, 1.0f32);
    let vertices = [
        _0, _0, _0,
        _0, _1, _0,
        _1, _0, _0,
        _1, _1, _0
    ];

    buffer_data(ARRAY_BUFFER, vertices, STATIC_DRAW);

    let texture_coord_buffer = gen_buffers(1 as GLsizei)[0];
    bind_buffer(ARRAY_BUFFER, texture_coord_buffer);

    let vertices = [
        _0, _1,
        _0, _0,
        _1, _1,
        _1, _0
    ];

    buffer_data(ARRAY_BUFFER, vertices, STATIC_DRAW);

    ret (triangle_vertex_buffer, texture_coord_buffer);
}

fn init_texture(width: uint, height: uint, data: [u8]) -> GLuint {
    let texture = gen_textures(1 as GLsizei)[0];
    bind_texture(TEXTURE_2D, texture);

    tex_parameter_i(TEXTURE_2D, TEXTURE_WRAP_S, REPEAT as GLint);
    tex_parameter_i(TEXTURE_2D, TEXTURE_WRAP_T, REPEAT as GLint);
    tex_parameter_i(TEXTURE_2D, TEXTURE_MAG_FILTER, LINEAR as GLint);
    tex_parameter_i(TEXTURE_2D, TEXTURE_MIN_FILTER, NEAREST as GLint);

    tex_image_2d(TEXTURE_2D, 0 as GLint, RGB as GLint, width as GLsizei, height as GLsizei,
                 0 as GLint, RGB, UNSIGNED_BYTE, data);
    ret texture;
}

fn render_scene(render_context: RenderContext, texture_width: uint, texture_height: uint,
                texture_data: [u8]) {

    let (vertex_buffer, texture_coord_buffer) = init_buffers();
    let texture = init_texture(texture_width, texture_height, texture_data);

    clear_color(0.0f32, 0.0f32, 1.0f32, 1.0f32);
    clear(COLOR_BUFFER_BIT);

    enable(TEXTURE_2D);
    bind_texture(TEXTURE_2D, texture);
    uniform_1i(render_context.sampler_uniform, 0);

    bind_buffer(ARRAY_BUFFER, vertex_buffer);
    vertex_attrib_pointer_f32(render_context.vertex_position_attr as GLuint, 3 as GLint, false,
                              0 as GLsizei, 0 as GLuint);

    bind_buffer(ARRAY_BUFFER, texture_coord_buffer);
    vertex_attrib_pointer_f32(render_context.texture_coord_attr as GLuint, 2 as GLint, false,
                              0 as GLsizei, 0 as GLuint);

    draw_arrays(TRIANGLE_STRIP, 0 as GLint, 4 as GLint);
}


use glut;
use stb_image;

import layers::*;
import rendergl::*;
import stb_image::image::load;

import glut::{create_window, display_func, init};
import glut::bindgen::{glutInitDisplayMode, glutMainLoop, glutPostRedisplay, glutSwapBuffers};

import comm::{chan, port, recv, send};
import libc::c_uint;
import os::{getenv, setenv};
import task::{builder, get_opts, run_listener, set_opts};

crust fn display_callback() {
    let mut t;
    alt getenv("RUST_LAYERS_PROGRESS") {
        none { t = 1.0f32; }
        some(string) { t = float::from_str(string).get() as f32; }
    }

    let stb_image = load("keep-calm-and-carry-on.jpg");
    let mut image = @Image(stb_image.width, stb_image.height, RGB24Format, copy stb_image.data);
    io::println(#fmt("image is %ux%u", image.width, image.height));

    let image_layer = ImageLayer(image);
    image_layer.common.transform = Matrix4(1.0f32 * t, 0.0f32,     0.0f32, 0.0f32,
                                           0.0f32,     1.0f32 * t, 0.0f32, 0.0f32,
                                           0.0f32,     0.0f32,     1.0f32, 0.0f32,
                                           0.0f32,     0.0f32,     0.0f32, 1.0f32);

    let context = init_render_context();
    render_scene(context, image_layer);

    t -= 0.004f32;
    setenv("RUST_LAYERS_PROGRESS", float::to_str(t as float, 6u));

    glutSwapBuffers();

    glutPostRedisplay();
}

#[test]
fn test_triangle_and_square() unsafe {
    let builder = builder();
    let opts = {
        sched: some({ mode: task::osmain, native_stack_size: none })
        with get_opts(builder)
    };
    set_opts(builder, opts);

    let port: port<()> = port();
    let chan = chan(port);
    let _result_ch: chan<()> = run_listener(builder, {
        |_port|

        init();
        glutInitDisplayMode(0 as c_uint);
        create_window("Rust Layers");
        display_func(display_callback);
        glutMainLoop();

        send(chan, ());
    });
    recv(port);
}



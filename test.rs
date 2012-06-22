use glut;
use stb_image;

import rendergl::*;
import stb_image::image::load;

import glut::{create_window, display_func, init};
import glut::bindgen::{glutInitDisplayMode, glutMainLoop, glutSwapBuffers};

import comm::{chan, port, recv, send};
import libc::c_uint;
import task::{builder, get_opts, run_listener, set_opts};

crust fn display_callback() {
    let image = load("keep-calm-and-carry-on.jpg");
    io::println(#fmt("image is %ux%ux%u", image.width, image.height, image.depth));

    let context = init_render_context();
    render_scene(context, image.width, image.height, image.data);

    glutSwapBuffers();
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



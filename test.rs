use glut;
use azure;
use geom;

import geom::point::Point2D;
import geom::rect::Rect;
import geom::size::Size2D;
import layers::*;
import rendergl::*;
import util::convert_rgb32_to_rgb24;

import glut::{create_window, display_func, init};
import glut::bindgen::{glutInitDisplayMode, glutMainLoop, glutPostRedisplay, glutSwapBuffers};

import azure::cairo::CAIRO_FORMAT_RGB24;
import CairoContext = azure::cairo_hl::Context;
import azure::azure_hl::{Color, ColorPattern, DrawTarget};
import azure::cairo_hl::ImageSurface;

import comm::{chan, port, recv, send};
import libc::c_uint;
import os::{getenv, setenv};
import task::{builder, get_opts, run_listener, set_opts};

class Renderer {
    let image: @Image;
    let mut image_layer: ImageLayer;
    let mut t: f32;
    let mut delta: f32;
    let mut render_context: option<RenderContext>;

    new() {
        let cairo_image = ImageSurface(CAIRO_FORMAT_RGB24, 500, 704);

        let draw_target = DrawTarget(cairo_image);
        draw_target.fill_rect(Rect(Point2D(50.0f32, 50.0f32), Size2D(300.0f32, 284.0f32)),
                              ColorPattern(Color(1.0f32, 1.0f32, 0.0f32, 1.0f32)));
        draw_target.flush();

        self.image = @Image(cairo_image.width() as uint, cairo_image.height() as uint, RGB24Format,
                            convert_rgb32_to_rgb24(cairo_image.data()));
        io::println(#fmt("image is %ux%u", self.image.width, self.image.height));

        self.image_layer = ImageLayer(self.image);

        self.t = 1.0f32;
        self.delta = -0.001f32;

        self.render_context = none;
    }

    fn get_display_callback(this: @Renderer) -> fn@() {
        fn@() {
            (*this).display_callback();
        }
    }

    fn display_callback() {
        alt self.render_context {
            none {
                self.render_context = some(init_render_context());
            }
            some(_) {
                // Nothing to do.
            }
        }
        let context = alt self.render_context {
            none { fail }
            some(ctx) { ctx }
        };

        let t = self.t;
        self.image_layer.common.transform = Matrix4(1.0f32 * t, 0.0f32,     0.0f32, 0.0f32,
                                                    0.0f32,     1.0f32 * t, 0.0f32, 0.0f32,
                                                    0.0f32,     0.0f32,     1.0f32, 0.0f32,
                                                    0.0f32,     0.0f32,     0.0f32, 1.0f32);

        render_scene(context, self.image_layer);

        self.t += self.delta;
        if self.t < 0.0f32 || self.t > 1.0f32 {
            self.delta = -self.delta;
        }

        glutSwapBuffers();

        glutPostRedisplay();
    }
}

#[test]
fn test_triangle_and_square() unsafe {
    let builder = builder();
    let opts = {
        sched: some({ mode: task::osmain, foreign_stack_size: none })
        with get_opts(builder)
    };
    set_opts(builder, opts);

    let port: port<()> = port();
    let chan = chan(port);
    let _result_ch: chan<()> = run_listener(builder, {
        |_port|

        let renderer = @Renderer();

        init();
        glutInitDisplayMode(0 as c_uint);
        create_window("Rust Layers");
        display_func(renderer.get_display_callback(renderer));
        glutMainLoop();

        send(chan, ());
    });
    recv(port);
}



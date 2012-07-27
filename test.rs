use glut;
use azure;
use geom;

import geom::point::Point2D;
import geom::rect::Rect;
import geom::size::Size2D;
import geom::matrix::Matrix4;
import layers::*;
import scene::*;
import rendergl::*;
import util::convert_rgb32_to_rgb24;

import glut::{DOUBLE, check_loop, create_window, destroy_window, display_func, init};
import glut::{init_display_mode, post_redisplay, swap_buffers, timer_func};

import azure::cairo::CAIRO_FORMAT_RGB24;
import CairoContext = azure::cairo_hl::Context;
import azure::azure_hl::{Color, ColorPattern, DrawTarget};
import azure::cairo_hl::ImageSurface;

import comm::{chan, peek, port, recv, send};
import libc::c_uint;
import os::{getenv, setenv};
import task::task_builder;

struct Renderer {
    mut layer: @TiledImageLayer;
    //mut layer: @ImageLayer;
    mut t: f32;
    mut delta: f32;
    mut render_context: option<RenderContext>;
    mut image: option<@Image>;

    new() {
        let cairo_image = ImageSurface(CAIRO_FORMAT_RGB24, 500, 704);

        self.image = none;

        let draw_target = DrawTarget(cairo_image);
        draw_target.fill_rect(Rect(Point2D(50.0f32, 50.0f32), Size2D(300.0f32, 284.0f32)),
                              ColorPattern(Color(1.0f32, 1.0f32, 0.0f32, 1.0f32)));
        draw_target.flush();

        let (width, height) = (cairo_image.width() as uint, cairo_image.height() as uint);
        let (tile_width, tile_height) = (width / 4, height / 4);
        let cairo_data = cairo_image.data();

        let tiles = dvec();
        for 4.timesi |y| {
            for 4.timesi |x| {
                // Extract the relevant part of the image.
                let data = dvec();

                let mut scanline_start = (y * tile_height * width + x * tile_width) * 4;
                for tile_height.times {
                    for (tile_width * 4).timesi |offset| {
                        data.push(cairo_data[scanline_start + offset]);
                    }

                    scanline_start += width * 4;
                }

                let data = convert_rgb32_to_rgb24(vec::from_mut(dvec::unwrap(data)));
                let image = @Image(tile_width, tile_height, RGB24Format, data); 
                self.image = some(image);
                tiles.push(image);
            }
        }

        let tiles = dvec::unwrap(tiles);
        self.layer = @TiledImageLayer(tiles, 4);

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
        self.layer.common.transform = Matrix4(400.0f32 * t, 0.0f32,       0.0f32, 0.0f32,
                                              0.0f32,       300.0f32 * t, 0.0f32, 0.0f32,
                                              0.0f32,       0.0f32,       1.0f32, 0.0f32,
                                              0.0f32,       0.0f32,       0.0f32, 1.0f32);

        let mut scene = Scene(TiledImageLayerKind(self.layer), Size2D(400.0f32, 300.0f32));
        //let mut scene = Scene(ImageLayerKind(self.layer), Size2D(400.0f32, 300.0f32));
        render_scene(context, scene);

        //self.t += self.delta;
        if self.t < 0.0f32 || self.t > 1.0f32 {
            self.delta = -self.delta;
        }

        swap_buffers();

        post_redisplay();
    }
}

#[test]
fn test_triangle_and_square() unsafe {
    let builder = task::task().sched_mode(task::osmain);

    let po: port<()> = port();
    let ch = chan(po);
    let _result_ch: chan<()> = do builder.spawn_listener |_po| {
        let renderer = @Renderer();

        init();
        init_display_mode(DOUBLE as c_uint);
        let window = create_window(~"Rust Layers");
        display_func(renderer.get_display_callback(renderer));

        let wakeup = port();
        let wakeup_chan = chan(wakeup);
        do timer_func(30000) {
            send(wakeup_chan, ());
        }

        loop {
            check_loop();

            if peek(wakeup) {
                recv(wakeup);
                send(ch, ());
                destroy_window(window);
                break;
            }
        }

        send(ch, ());
    };

    recv(po);
}



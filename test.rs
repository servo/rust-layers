use geom::point::Point2D;
use geom::rect::Rect;
use geom::size::Size2D;
use geom::matrix::Matrix4;
use layers::*;
use scene::*;
use rendergl::*;
use util::convert_rgb32_to_rgb24;

use azure::azure_hl::{Color, ColorPattern, DrawTarget};

use cairo::cairo::CAIRO_FORMAT_RGB24;
use CairoContext = cairo::cairo_hl::Context;
use cairo::cairo_hl::ImageSurface;

use glut::glut::{DOUBLE, check_loop, create_window, destroy_window, display_func, init};
use glut::glut::{init_display_mode, post_redisplay, swap_buffers, timer_func};

use core::comm::{Chan, Port};
use libc::c_uint;
use os::{getenv, setenv};
use task::TaskBuilder;

struct Renderer {
    mut layer: @TiledImageLayer,
    //mut layer: @ImageLayer,
    mut t: f32,
    mut delta: f32,
    mut render_context: Option<RenderContext>,
    mut image: Option<@Image>,
}

impl Renderer {
    fn get_display_callback(this: @Renderer) -> fn@() {
        fn@() {
            (*this).display_callback();
        }
    }

    fn display_callback() {
        match self.render_context {
            None => {
                self.render_context = Some(init_render_context());
            }
            Some(_) => {
                // Nothing to do.
            }
        }
        let context = match self.render_context {
            None => fail!(),
            Some(ctx) => ctx
        };

        let t = self.t;
        let transform =  Matrix4(400.0f32 * t, 0.0f32,       0.0f32, 0.0f32,
                                 0.0f32,       300.0f32 * t, 0.0f32, 0.0f32,
                                 0.0f32,       0.0f32,       1.0f32, 0.0f32,
                                 0.0f32,       0.0f32,       0.0f32, 1.0f32);
        self.layer.common.transform = transform;

        let scene = Scene(TiledImageLayerKind(self.layer), Size2D(400.0f32, 300.0f32), transform);
        render_scene(context, &scene);

        //self.t += self.delta;
        if self.t < 0.0f32 || self.t > 1.0f32 {
            self.delta = -self.delta;
        }

        swap_buffers();

        post_redisplay();
    }
}

fn Renderer() -> Renderer {
        let cairo_image = ImageSurface(CAIRO_FORMAT_RGB24, 500, 704);

        let draw_target = DrawTarget(&cairo_image);
        draw_target.fill_rect(&Rect(Point2D(50.0f32, 50.0f32), Size2D(300.0f32, 284.0f32)),
                              &ColorPattern(Color(1.0, 1.0, 0.0, 1.0)));
        draw_target.flush();

        let (width, height) = (cairo_image.width() as uint, cairo_image.height() as uint);
        let (tile_width, tile_height) = (width / 4, height / 4);
        let cairo_data = cairo_image.data();

        let tiles = DVec();
        for uint::range(0,4) |y| {
            for uint::range(0,4) |x| {
                // Extract the relevant part of the image.
                let data = DVec();

                let mut scanline_start = (y * tile_height * width + x * tile_width) * 4;
                for tile_height.times {
                    for uint::range(0, tile_width * 4) |offset| {
                        data.push(cairo_data[scanline_start + offset]);
                    }

                    scanline_start += width * 4;
                }

                let data = convert_rgb32_to_rgb24(dvec::unwrap(data));
                let image = @Image::new(BasicImageData::new(Size2D(tile_width, tile_height), tile_width, RGB24Format, data) as ImageData); 
                tiles.push(image);
            }
        }

        let tiles = dvec::unwrap(tiles);
    
    Renderer {
        image : Some(tiles[0]),
        layer : @TiledImageLayer(tiles, 4),
        t : 1.0f32,
        delta : -0.001f32,
        render_context : None
    }
}

/*
#[test]
fn test_triangle_and_square() unsafe {
    let builder = task::task().sched_mode(task::PlatformThread);

    let po: Port<()> = Port();
    let ch = Chan(&po);
    let _result_ch: Chan<()> = do builder.spawn_listener |_po| {
        let renderer = @Renderer();

        init();
        init_display_mode(DOUBLE as c_uint);
        let window = create_window(~"Rust Layers");
        display_func(renderer.get_display_callback(renderer));

        let wakeup : Port<()> = Port();
        let wakeup_chan = Chan(&wakeup);
        do timer_func(300) {
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
*/


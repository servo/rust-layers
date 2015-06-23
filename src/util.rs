// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// Miscellaneous utilities.

use std::iter::repeat;
use euclid::{Rect, Point2D, Point3D, Point4D, Matrix4, Size2D};
use std::f32;

const W_CLIPPING_PLANE: f32 = 0.00001;

#[derive(Debug)]
pub struct ScreenRect {
    pub rect: Rect<f32>,
    pub z_center: f32,
}

pub fn convert_rgb32_to_rgb24(buffer: &[u8]) -> Vec<u8> {
    let mut i = 0;
    repeat(buffer.len() * 3 / 4).map(|j| {
        match j % 3 {
            0 => {
                buffer[i + 2]
            }
            1 => {
                buffer[i + 1]
            }
            2 => {
                let val = buffer[i];
                i += 4;
                val
            }
            _ => {
                panic!()
            }
        }
    }).collect()
}

// Sutherland-Hodgman clipping algorithm
fn clip_polygon_to_near_plane(clip_space_vertices: &[Point4D<f32>; 4])
                                  -> Option<Vec<Point4D<f32>>> {
    let mut out_vertices = vec!();

    // TODO(gw): Check for trivial accept / reject if all
    // input vertices are on the same side of the near plane.

    for i in 0..clip_space_vertices.len() {
        let previous_vertex = if i == 0 {
            clip_space_vertices.last().unwrap()
        } else {
            &clip_space_vertices[i-1]
        };
        let current_vertex = &clip_space_vertices[i];

        let previous_dot = if previous_vertex.w < W_CLIPPING_PLANE { -1 } else { 1 };
        let current_dot = if current_vertex.w < W_CLIPPING_PLANE { -1 } else { 1 };

        if previous_dot * current_dot < 0 {
            let int_factor = (previous_vertex.w - W_CLIPPING_PLANE) / (previous_vertex.w - current_vertex.w);

            // TODO(gw): Impl operators on Point4D for this
            let int_point = Point4D::new(
                previous_vertex.x + int_factor * (current_vertex.x - previous_vertex.x),
                previous_vertex.y + int_factor * (current_vertex.y - previous_vertex.y),
                previous_vertex.z + int_factor * (current_vertex.z - previous_vertex.z),
                previous_vertex.w + int_factor * (current_vertex.w - previous_vertex.w),
            );

            out_vertices.push(int_point);
        }

        if current_dot > 0 {
            out_vertices.push(*current_vertex);
        }
    }

    if out_vertices.len() < 3 {
        return None
    }

    Some(out_vertices)
}

pub fn project_rect_to_screen(rect: &Rect<f32>,
                              transform: &Matrix4) -> Option<ScreenRect> {
    let mut result = None;

    let x0 = rect.min_x();
    let x1 = rect.max_x();

    let y0 = rect.min_y();
    let y1 = rect.max_y();

    let xc = (x0 + x1) * 0.5;
    let yc = (y0 + y1) * 0.5;
    let vc = Point4D::new(xc, yc, 0.0, 1.0);
    let vc = transform.transform_point4d(&vc);

    let vertices = [
        Point4D::new(x0, y0, 0.0, 1.0),
        Point4D::new(x1, y0, 0.0, 1.0),
        Point4D::new(x0, y1, 0.0, 1.0),
        Point4D::new(x1, y1, 0.0, 1.0)
    ];

    // Transform vertices to clip space
    let vertices_clip_space = [
        transform.transform_point4d(&vertices[0]),
        transform.transform_point4d(&vertices[1]),
        transform.transform_point4d(&vertices[2]),
        transform.transform_point4d(&vertices[3]),
    ];

    // Clip the resulting quad against the near-plane
    // There's no need to clip against other planes for correctness,
    // since as long as w > 0, we will get valid homogenous coords.
    // TODO(gw): Potential optimization to clip against other planes.
    let clipped_vertices = clip_polygon_to_near_plane(&vertices_clip_space);

    if let Some(clipped_vertices) = clipped_vertices {
        // Perform perspective division on the clip space vertices
        // to get homogenous space vertices. Then calculate the
        // 2d AABB for this polygon in screen space.

        let mut min_vertex = Point3D::new(f32::MAX, f32::MAX, f32::MAX);
        let mut max_vertex = Point3D::new(-f32::MAX, -f32::MAX, -f32::MAX);

        for vertex_cs in &clipped_vertices {
            // This should be enforced by the clipper above
            debug_assert!(vertex_cs.w > 0.0);
            let inv_w = 1.0 / vertex_cs.w;
            let x = vertex_cs.x * inv_w;
            let y = vertex_cs.y * inv_w;
            let z = vertex_cs.z * inv_w;

            // Calculate the min/max z-depths of this layer.
            // This is used for simple depth sorting later.
            min_vertex.x = min_vertex.x.min(x);
            max_vertex.x = max_vertex.x.max(x);
            min_vertex.y = min_vertex.y.min(y);
            max_vertex.y = max_vertex.y.max(y);
            min_vertex.z = min_vertex.z.min(z);
            max_vertex.z = max_vertex.z.max(z);
        }

        let origin = Point2D::new(min_vertex.x, min_vertex.y);
        let size = Size2D::new(max_vertex.x - min_vertex.x,
                               max_vertex.y - min_vertex.y);

        result = Some(ScreenRect {
            rect: Rect::new(origin, size),
            z_center: vc.z,
        });
    }

    result
}

// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#[crate_id = "github.com/mozilla-servo/rust-layers#layers:0.1"];

#[feature(managed_boxes)];

extern crate extra;
extern crate geom;
extern crate opengles;
extern crate std;
extern crate serialize;

#[cfg(target_os="macos")]
extern crate core_foundation;
#[cfg(target_os="macos")]
extern crate io_surface;
#[cfg(target_os="macos")]
extern crate collections;

#[cfg(target_os="linux")]
extern crate xlib;

#[cfg(target_os="android")]
extern crate egl;

pub mod layers;
pub mod color;
pub mod rendergl;
pub mod scene;
pub mod texturegl;
pub mod util;

pub mod platform {
    #[cfg(target_os="linux")]
    pub mod linux {
        pub mod surface;
    }
    #[cfg(target_os="macos")]
    pub mod macos {
        pub mod surface;
    }
    #[cfg(target_os="android")]
    pub mod android {
        pub mod surface;
    }
    pub mod surface;
}


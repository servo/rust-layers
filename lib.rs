// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#[feature(managed_boxes)];

extern mod std;

extern mod geom = "rust-geom";
extern mod opengles = "rust-opengles";

#[cfg(target_os="macos")]
extern mod core_foundation = "rust-core-foundation";
#[cfg(target_os="macos")]
extern mod io_surface = "rust-io-surface";

#[cfg(target_os="linux")]
extern mod xlib = "rust-xlib";

pub mod layers;
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
    pub mod surface;
}


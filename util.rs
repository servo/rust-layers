// Copyright 2013 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// Miscellaneous utilities.

use std::vec::from_fn;

pub fn convert_rgb32_to_rgb24(buffer: ~[u8]) -> ~[u8] {
    let mut i = 0;
    do from_fn(buffer.len() * 3 / 4) |j| {
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
                fail!()
            }
        }
    }
}


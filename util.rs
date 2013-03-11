// Miscellaneous utilities.

use core::vec::from_fn;

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


use crate::BootInfo;
use font8x8::{UnicodeFonts, BASIC_FONTS};
use core::fmt;
use spin::Mutex;

// The Global, Thread-Safe Instance of our Screen Writer
pub static WRITER: Mutex<Option<VgaWriter>> = Mutex::new(None);

pub struct VgaWriter {
    framebuffer: *mut u8,
    width: usize,
    height: usize,
    stride: usize,
    x_pos: usize,
    y_pos: usize,
}

unsafe impl Send for VgaWriter {}
unsafe impl Sync for VgaWriter {}

impl VgaWriter {
    pub unsafe fn new(info: *const BootInfo) -> Self {
        let info_ref = &*info;
        VgaWriter {
            framebuffer: info_ref.framebuffer_base as *mut u8,
            width: info_ref.width,
            height: info_ref.height,
            stride: info_ref.stride,
            x_pos: 10,
            y_pos: 10,
        }
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                b'\n' => self.newline(),
                byte => self.write_char(byte as char),
            }
        }
    }

    fn write_char(&mut self, c: char) {
        if self.x_pos + 8 >= self.width {
            self.newline();
        }
        if let Some(glyph) = BASIC_FONTS.get(c) {
            for (y, row) in glyph.iter().enumerate() {
                for x in 0..8 {
                    if (*row & (1 << x)) != 0 {
                        self.put_pixel(self.x_pos + x, self.y_pos + y, 0xFF); 
                    } else {
                        self.put_pixel(self.x_pos + x, self.y_pos + y, 0x00); 
                    }
                }
            }
        }
        self.x_pos += 8;
    }

    fn put_pixel(&mut self, x: usize, y: usize, color: u8) {
        let offset = (y * self.stride * 4) + (x * 4);
        unsafe {
            self.framebuffer.add(offset).write_volatile(color);
            self.framebuffer.add(offset + 1).write_volatile(color);
            self.framebuffer.add(offset + 2).write_volatile(color);
            self.framebuffer.add(offset + 3).write_volatile(0); 
        }
    }

    fn newline(&mut self) {
        self.y_pos += 10;
        self.x_pos = 10;
    }
}

// ---------------------------------------------------------
// NEW: Teach Rust how to format numbers & text into our Writer
// ---------------------------------------------------------
impl fmt::Write for VgaWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

// ---------------------------------------------------------
// NEW: The Custom Macro System
// ---------------------------------------------------------
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    // Lock the global Mutex, check if the writer exists, and push the text
    if let Some(writer) = WRITER.lock().as_mut() {
        writer.write_fmt(args).unwrap();
    }
}
use crate::BootInfo;
use core::ptr::write_volatile;
use alloc::vec::Vec;
use alloc::vec;

// ---------------------------------------------------------
// OFF-SCREEN CANVAS (PHASE 6)
// ---------------------------------------------------------
pub struct Canvas {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

impl Canvas {
    pub fn new(x: usize, y: usize, width: usize, height: usize, bg_color: u32) -> Self {
        Canvas {
            x, y, width, height,
            pixels: vec![bg_color; width * height],
        }
    }

    pub fn draw_pixel(&mut self, px: usize, py: usize, color: u32) {
        if px < self.width && py < self.height {
            self.pixels[py * self.width + px] = color;
        }
    }

    pub fn fill_rect(&mut self, start_x: usize, start_y: usize, w: usize, h: usize, color: u32) {
        for y in start_y..(start_y + h) {
            for x in start_x..(start_x + w) {
                self.draw_pixel(x, y, color);
            }
        }
    }

    pub fn scroll_up(&mut self, shift_y: usize, bg_color: u32) {
        let pixel_shift = shift_y * self.width;
        if pixel_shift >= self.pixels.len() {
            self.pixels.fill(bg_color);
            return;
        }
        self.pixels.copy_within(pixel_shift.., 0);
        let len = self.pixels.len();
        self.pixels[(len - pixel_shift)..].fill(bg_color);
    }
}

pub struct Compositor {
    pub framebuffer: u64,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub terminal_layer: Option<Canvas>,
}

pub static mut SERVER: Compositor = Compositor {
    framebuffer: 0, width: 0, height: 0, stride: 0, terminal_layer: None
};

pub static mut TERM_X: usize = 20;
pub static mut TERM_Y: usize = 20;

pub unsafe fn init(info: *const BootInfo) {
    let boot_info = &*info;
    SERVER.framebuffer = boot_info.framebuffer_base;
    SERVER.width = boot_info.width;
    SERVER.height = boot_info.height;
    SERVER.stride = boot_info.stride;

    // Initialize the hidden canvas for the Terminal (White Background)
    SERVER.terminal_layer = Some(Canvas::new(0, 40, SERVER.width, SERVER.height - 40, 0xFFFFFF));
}

pub unsafe fn blit_canvas(canvas: &Canvas) {
    let src_ptr = canvas.pixels.as_ptr();
    let dst_ptr = SERVER.framebuffer as *mut u32;

    for cy in 0..canvas.height {
        let screen_y = canvas.y + cy;
        if screen_y >= SERVER.height { break; }

        let src_offset = cy * canvas.width;
        let dst_offset = (screen_y * SERVER.stride) + canvas.x;

        let row_src = src_ptr.add(src_offset);
        let row_dst = dst_ptr.add(dst_offset);

        core::ptr::copy_nonoverlapping(row_src, row_dst, canvas.width);
    }
}

pub unsafe fn draw_pixel(x: usize, y: usize, color: u32) {
    if x >= SERVER.width || y >= SERVER.height { return; }
    let offset = (y * SERVER.stride) + x;
    let ptr = (SERVER.framebuffer + (offset as u64 * 4)) as *mut u32;
    write_volatile(ptr, color);
}

pub unsafe fn fill_rect(x: usize, y: usize, w: usize, h: usize, color: u32) {
    for curr_y in y..(y + h) {
        for curr_x in x..(x + w) { draw_pixel(curr_x, curr_y, color); }
    }
}

pub unsafe fn clear_screen(color: u32) {
    fill_rect(0, 0, SERVER.width, SERVER.height, color);
}

fn get_char_bitmap(c: char) -> [u8; 8] {
    match c {
        'A' => [0x18, 0x24, 0x42, 0x42, 0x7E, 0x42, 0x42, 0x00],
        'B' => [0x3C, 0x22, 0x22, 0x3C, 0x22, 0x22, 0x3C, 0x00],
        'C' => [0x3C, 0x42, 0x40, 0x40, 0x40, 0x42, 0x3C, 0x00],
        'D' => [0x38, 0x24, 0x22, 0x22, 0x22, 0x24, 0x38, 0x00],
        'E' => [0x7E, 0x40, 0x40, 0x78, 0x40, 0x40, 0x7E, 0x00],
        'F' => [0x7E, 0x40, 0x40, 0x78, 0x40, 0x40, 0x40, 0x00],
        'G' => [0x3C, 0x42, 0x40, 0x4E, 0x42, 0x42, 0x3C, 0x00],
        'H' => [0x42, 0x42, 0x42, 0x7E, 0x42, 0x42, 0x42, 0x00],
        'I' => [0x3E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x3E, 0x00],
        'J' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x44, 0x38, 0x00],
        'K' => [0x44, 0x48, 0x50, 0x60, 0x50, 0x48, 0x44, 0x00],
        'L' => [0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x7E, 0x00],
        'M' => [0x42, 0x66, 0x5A, 0x42, 0x42, 0x42, 0x42, 0x00],
        'N' => [0x42, 0x62, 0x52, 0x4A, 0x46, 0x42, 0x42, 0x00],
        'O' => [0x3C, 0x42, 0x42, 0x42, 0x42, 0x42, 0x3C, 0x00],
        'P' => [0x3C, 0x42, 0x42, 0x3C, 0x40, 0x40, 0x40, 0x00],
        'Q' => [0x3C, 0x42, 0x42, 0x42, 0x4A, 0x44, 0x3A, 0x00],
        'R' => [0x3C, 0x22, 0x22, 0x3C, 0x24, 0x22, 0x42, 0x00],
        'S' => [0x3C, 0x42, 0x40, 0x3C, 0x02, 0x42, 0x3C, 0x00],
        'T' => [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00],
        'U' => [0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x3C, 0x00],
        'V' => [0x42, 0x42, 0x42, 0x42, 0x24, 0x24, 0x18, 0x00],
        'W' => [0x42, 0x42, 0x42, 0x42, 0x5A, 0x66, 0x42, 0x00],
        'X' => [0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x42, 0x00],
        'Y' => [0x42, 0x42, 0x24, 0x18, 0x18, 0x18, 0x18, 0x00],
        'Z' => [0x7E, 0x04, 0x08, 0x10, 0x20, 0x40, 0x7E, 0x00],
        '0' => [0x3C, 0x46, 0x4A, 0x52, 0x62, 0x42, 0x3C, 0x00],
        '1' => [0x18, 0x28, 0x08, 0x08, 0x08, 0x08, 0x3E, 0x00],
        ' ' => [0x00; 8],
        ':' => [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x3C, 0x00, 0x00, 0x00, 0x00],
        '>' => [0x00, 0x08, 0x04, 0x02, 0x04, 0x08, 0x00, 0x00],
        '(' => [0x08, 0x10, 0x20, 0x20, 0x20, 0x10, 0x08, 0x00],
        ')' => [0x20, 0x10, 0x08, 0x08, 0x08, 0x10, 0x20, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00],
        '!' => [0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00],
        _   => [0xFF; 8], 
    }
}

pub unsafe fn draw_string(mut x: usize, y: usize, text: &str, color: u32, scale: usize) {
    for c in text.chars() {
        // Fix: Force uppercase for the top bar!
        let bitmap = get_char_bitmap(c.to_ascii_uppercase());
        for row in 0..8 {
            for col in 0..8 {
                if (bitmap[row] & (1 << (7 - col))) != 0 {
                    fill_rect(x + (col * scale), y + (row * scale), scale, scale, color);
                }
            }
        }
        x += 8 * scale + (2 * scale); 
    }
}

// ---------------------------------------------------------
// THE BULLETPROOF TERMINAL EMULATOR
// ---------------------------------------------------------
pub unsafe fn terminal_print(text: &str, color: u32) {
    if let Some(ref mut canvas) = SERVER.terminal_layer {
        for c in text.chars() {
            if c == '\n' {
                TERM_X = 20;
                TERM_Y += 30;
                
                if TERM_Y + 30 > canvas.height {
                    canvas.scroll_up(30, 0xFFFFFF); // Scroll and clear background with White
                    TERM_Y -= 30; 
                }
                continue;
            }
            
            // THE FIX: Convert every letter to uppercase so the font engine doesn't panic!
            let bitmap = get_char_bitmap(c.to_ascii_uppercase());
            
            for row in 0..8 {
                for col in 0..8 {
                    if (bitmap[row] & (1 << (7 - col))) != 0 {
                        canvas.fill_rect(TERM_X + (col * 3), TERM_Y + (row * 3), 3, 3, color);
                    }
                }
            }
            TERM_X += 24 + 6; 
            
            if TERM_X > canvas.width - 40 {
                TERM_X = 20;
                TERM_Y += 30;
                if TERM_Y + 30 > canvas.height {
                    canvas.scroll_up(30, 0xFFFFFF);
                    TERM_Y -= 30;
                }
            }
        }
        blit_canvas(canvas);
    }
}
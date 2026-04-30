use core::arch::naked_asm;
use core::arch::asm;
use crate::compositor::Canvas;
use alloc::boxed::Box;
use alloc::string::String;

const MSR_EFER: u32 = 0xC0000080;
const MSR_STAR: u32 = 0xC0000081;
const MSR_LSTAR: u32 = 0xC0000082;
const MSR_FMASK: u32 = 0xC0000084;

pub fn init(kernel_code_sel: u16, _kernel_data_sel: u16, user_code_sel: u16, _user_data_sel: u16) {
    unsafe {
        let mut efer: u64;
        asm!("rdmsr", in("ecx") MSR_EFER, out("eax") efer, out("edx") _);
        efer |= 1; 
        asm!("wrmsr", in("ecx") MSR_EFER, in("eax") efer as u32, in("edx") (efer >> 32) as u32);

        let star_high = (user_code_sel & 0xFFFC) - 16;
        let star: u64 = ((kernel_code_sel as u64) << 32) | ((star_high as u64) << 48);
        asm!("wrmsr", in("ecx") MSR_STAR, in("eax") star as u32, in("edx") (star >> 32) as u32);

        let lstar = syscall_entry as u64;
        asm!("wrmsr", in("ecx") MSR_LSTAR, in("eax") lstar as u32, in("edx") (lstar >> 32) as u32);

        let fmask: u64 = 0x200; 
        asm!("wrmsr", in("ecx") MSR_FMASK, in("eax") fmask as u32, in("edx") (fmask >> 32) as u32);
        
        crate::compositor::terminal_print("SYS: Fast SYSCALL Interface Armed.\n", 0x3B82F6);
    }
}

#[unsafe(naked)]
extern "C" fn syscall_entry() {
    unsafe {
        naked_asm!(
            "push rcx",
            "push r11",
            
            "push rax", "push rdi", "push rsi", "push rdx",
            "push r8", "push r9", "push r10",

            "mov rcx, rdx",  
            "mov rdx, rsi",  
            "mov rsi, rdi",  
            "mov rdi, rax",  

            "mov ax, ds",
            "push rax",
            
            "mov ax, 0x10",
            "mov ds, ax", "mov es, ax", "mov fs, ax", "mov gs, ax",

            "call {handler}",

            "mov r12, rax",

            "pop rax",
            "mov ds, ax", "mov es, ax", "mov fs, ax", "mov gs, ax",

            "pop r10", "pop r9", "pop r8",
            "pop rdx", "pop rsi", "pop rdi", "pop rax", 
            
            "mov rax, r12",

            "pop r11", "pop rcx",
            "sysretq",
            handler = sym syscall_handler
        );
    }
}

extern "C" fn syscall_handler(syscall_num: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    unsafe {
        match syscall_num {
            1 => {
                let str_ptr = arg1 as *const u8;
                let str_len = arg2 as usize;
                let color = arg3 as u32;
                let slice = core::slice::from_raw_parts(str_ptr, str_len);
                if let Ok(text) = core::str::from_utf8(slice) {
                    crate::compositor::terminal_print(text, color);
                } else {
                    crate::compositor::terminal_print("\n> SYS: SYSCALL STRING UTF8 ERROR!\n", 0xEF4444);
                }
                0
            },
            2 => {
                let x = arg1 as usize;
                let y = arg2 as usize;
                let size = arg3 as usize;
                let canvas = Box::new(Canvas::new(x, y, size, size, 0x000000));
                Box::into_raw(canvas) as u64
            },
            3 => {
                let canvas_ptr = arg1 as *const Canvas;
                if !canvas_ptr.is_null() {
                    let canvas = &*canvas_ptr;
                    crate::compositor::blit_canvas(canvas);
                }
                0
            },
            4 => {
                // --- Read NVMe Sector ---
                let lba = arg1;
                crate::compositor::terminal_print("\n> SYS: RING-3 NVME READ REQUEST ACCEPTED.\n", 0xF59E0B);
                
                if let Some(ref mut drive) = crate::NVME_DRIVE {
                    let data = drive.read_sector(lba);
                    let mut safe_buffer: Box<[u8; 512]> = Box::new([0; 512]);
                    safe_buffer.copy_from_slice(&data);
                    
                    crate::compositor::terminal_print("> SYS: NVME DMA COMPLETE. PASSING POINTER TO RING-3.\n", 0x10B981);
                    if let Some(ref canvas) = crate::compositor::SERVER.terminal_layer { crate::compositor::blit_canvas(canvas); }
                    
                    Box::into_raw(safe_buffer) as u64 
                } else {
                    crate::compositor::terminal_print("> SYS: NVME DRIVE NOT FOUND!\n", 0xEF4444);
                    0
                }
            },
            5 => {
                // --- Send Network Packet ---
                let str_ptr = arg1 as *const u8;
                let str_len = arg2 as usize;
                let slice = core::slice::from_raw_parts(str_ptr, str_len);
                
                if let Ok(text) = core::str::from_utf8(slice) {
                    let log = alloc::format!("\n> SYS: RING-3 NETWORK REQUEST. PAYLOAD: '{}'\n", text);
                    crate::compositor::terminal_print(&log, 0xF59E0B);
                    
                    if let Some(ref mut nic) = crate::NET_CARD {
                        // The TCP stack isn't built yet, so we use arp_request to prove 
                        // we can trigger the hardware TX ring from User Space!
                        let dest_ip = [10, 0, 2, 2];
                        nic.arp_request(dest_ip);
                        
                        crate::compositor::terminal_print("> SYS: E1000 HARDWARE TRANSMIT (TX) COMPLETE!\n", 0x10B981);
                    } else {
                        crate::compositor::terminal_print("> SYS: E1000 NETWORK CARD NOT FOUND!\n", 0xEF4444);
                    }
                }
                if let Some(ref canvas) = crate::compositor::SERVER.terminal_layer { crate::compositor::blit_canvas(canvas); }
                0
            },
            _ => {
                crate::compositor::terminal_print("\n> SYS: UNKNOWN SYSCALL TRIGGERED!\n", 0xEF4444);
                0
            }
        }
    }
}
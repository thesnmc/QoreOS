use core::arch::naked_asm;
use core::arch::asm;

// MSR Addresses for Syscall Configuration
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

            // --- THE ABI ALIGNMENT FIX ---
            // Shift registers FIRST before we overwrite RAX with the Data Segment!
            "mov rcx, rdx",  // arg4 = arg3 (color)
            "mov rdx, rsi",  // arg3 = arg2 (len)
            "mov rsi, rdi",  // arg2 = arg1 (ptr)
            "mov rdi, rax",  // arg1 = syscall_num

            // Now safely save Ring-3 Data Segments
            "mov ax, ds",
            "push rax",
            
            // Load Ring-0 Kernel Data Segment (0x10)
            "mov ax, 0x10",
            "mov ds, ax", "mov es, ax", "mov fs, ax", "mov gs, ax",

            "call {handler}",

            "pop rax",
            "mov ds, ax", "mov es, ax", "mov fs, ax", "mov gs, ax",

            "pop r10", "pop r9", "pop r8",
            "pop rdx", "pop rsi", "pop rdi", "pop rax", // Pops the ORIGINAL rax (1) back perfectly!
            
            "pop r11", "pop rcx",
            "sysretq",
            handler = sym syscall_handler
        );
    }
}

extern "C" fn syscall_handler(syscall_num: u64, arg1: u64, arg2: u64, arg3: u64) {
    unsafe {
        if syscall_num == 1 {
            let str_ptr = arg1 as *const u8;
            let str_len = arg2 as usize;
            let color = arg3 as u32;
            
            let slice = core::slice::from_raw_parts(str_ptr, str_len);
            if let Ok(text) = core::str::from_utf8(slice) {
                crate::compositor::terminal_print(text, color);
            } else {
                crate::compositor::terminal_print("\n> SYS: SYSCALL STRING UTF8 ERROR!\n", 0xEF4444);
            }
        } else {
            crate::compositor::terminal_print("\n> SYS: UNKNOWN SYSCALL TRIGGERED!\n", 0xEF4444);
        }

        if let Some(ref canvas) = crate::compositor::SERVER.terminal_layer {
            crate::compositor::blit_canvas(canvas);
        }
    }
}

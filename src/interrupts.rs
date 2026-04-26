use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};
use lazy_static::lazy_static;
use crate::println;
use crate::gdt;
use x86_64::instructions::port::Port;

const TIMER_INT: u8 = 32;
const KEYBOARD_INT: u8 = 33;
const SYSCALL_INT: u8 = 0x80;

pub static mut LAPIC_BASE: u64 = 0;

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
            idt[SYSCALL_INT as usize].set_handler_fn(syscall_handler)
                .set_privilege_level(x86_64::PrivilegeLevel::Ring3);
            idt[KEYBOARD_INT as usize].set_handler_fn(keyboard_interrupt_handler)
                .set_privilege_level(x86_64::PrivilegeLevel::Ring3);
        }
        idt[TIMER_INT as usize].set_handler_fn(timer_interrupt_handler);
        idt
    };
}

pub fn init_idt() { IDT.load(); }

pub unsafe fn init_apic(lapic: u64, ioapic: u64) {
    LAPIC_BASE = lapic;
    Port::<u8>::new(0x21).write(0xFF);
    Port::<u8>::new(0xA1).write(0xFF);

    if lapic == 0 || ioapic == 0 { return; }

    let siv_ptr = (lapic + 0xF0) as *mut u32;
    core::ptr::write_volatile(siv_ptr, core::ptr::read_volatile(siv_ptr) | 0x100 | 0xFF);

    let ioregsel = ioapic as *mut u32;
    let iowin = (ioapic + 0x10) as *mut u32;
    core::ptr::write_volatile(ioregsel, 0x12);
    core::ptr::write_volatile(iowin, 33); 
    core::ptr::write_volatile(ioregsel, 0x13);
    core::ptr::write_volatile(iowin, 0); 
}

fn notify_end_of_interrupt(_int_id: u8) {
    unsafe {
        if LAPIC_BASE != 0 { core::ptr::write_volatile((LAPIC_BASE + 0xB0) as *mut u32, 0); }
    }
}

extern "x86-interrupt" fn breakpoint_handler(_stack_frame: InterruptStackFrame) {}
extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, _error_code: u64) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}
extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    notify_end_of_interrupt(TIMER_INT);
}

extern "x86-interrupt" fn syscall_handler(_stack_frame: InterruptStackFrame) {
    unsafe {
        crate::compositor::terminal_print("\nSYS_NET_TX: UDP BROADCAST FIRED!\n", 0xEF4444);
        if let Some(ref mut nic) = crate::NET_CARD { nic.udp_broadcast(); }
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut port = Port::<u8>::new(0x60);
    let scancode = unsafe { port.read() };
    
    let char_opt = match scancode {
        0x10 => Some("Q"), 0x11 => Some("W"), 0x12 => Some("E"), 0x13 => Some("R"),
        0x14 => Some("T"), 0x15 => Some("Y"), 0x16 => Some("U"), 0x17 => Some("I"),
        0x18 => Some("O"), 0x19 => Some("P"), 0x1E => Some("A"), 0x1F => Some("S"),
        0x20 => Some("D"), 0x21 => Some("F"), 0x22 => Some("G"), 0x23 => Some("H"),
        0x24 => Some("J"), 0x25 => Some("K"), 0x26 => Some("L"), 0x2C => Some("Z"),
        0x2D => Some("X"), 0x2E => Some("C"), 0x2F => Some("V"), 0x30 => Some("B"),
        0x31 => Some("N"), 0x32 => Some("M"), 0x39 => Some(" "), 
        0x1C => {
            unsafe { core::arch::asm!("int 0x80"); } 
            None
        }
        _ => None,
    };

    if let Some(c) = char_opt {
        unsafe { crate::compositor::terminal_print(c, 0x10B981); }
    }
    notify_end_of_interrupt(KEYBOARD_INT);
}
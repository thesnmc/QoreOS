use core::arch::naked_asm;

#[unsafe(naked)]
pub extern "C" fn drop_to_usermode(
    code_selector: u64,   // rdi  <-- CHANGED TO u64
    data_selector: u64,   // rsi  <-- CHANGED TO u64
    instruction_ptr: u64, // rdx
    stack_ptr: u64        // rcx
) -> ! {
    naked_asm!(
        "mov ds, si",
        "mov es, si",
        "mov fs, si",
        "mov gs, si",
        
        "push rsi",      // SS
        "push rcx",      // RSP
        "push 0x002",    // RFLAGS 
        "push rdi",      // CS
        "push rdx",      // RIP
        
        "iretq"
    );
}
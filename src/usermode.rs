use core::arch::naked_asm;

/// The absolute edge of the cliff. 
/// Violently drops the CPU from Ring-0 down to Ring-3.
#[unsafe(naked)]
pub extern "C" fn drop_to_usermode(
    code_selector: u16,   // rdi
    data_selector: u16,   // rsi
    instruction_ptr: u64, // rdx
    stack_ptr: u64        // rcx
) -> ! {
    naked_asm!(
        // 1. Load User Data Segment into hardware segment registers
        "mov ds, si",
        "mov es, si",
        "mov fs, si",
        "mov gs, si",
        
        // 2. Build the exact hardware Interrupt Frame for iretq
        "push rsi",      // SS (Stack Segment = User Data Selector)
        "push rcx",      // RSP (User Stack Pointer)
        "push 0x202",    // RFLAGS (0x202 = Interrupts Enabled)
        "push rdi",      // CS (Code Segment = User Code Selector)
        "push rdx",      // RIP (Instruction Pointer to User App)
        
        // 3. Pull the ripcord
        "iretq"
    );
}
use linked_list_allocator::LockedHeap;
use crate::println;

// This tells the Rust compiler: "When someone calls Box::new() or Vec::new(), use this!"
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

// Carve out exactly 2 Megabytes of RAM for our kernel's brain
const HEAP_SIZE: usize = 2 * 1024 * 1024;

// By declaring this as static, it gets permanently baked into the kernel's memory footprint
static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

pub fn init_heap() {
    unsafe {
        // We hand the raw memory pointer to our allocator
        ALLOCATOR.lock().init(HEAP_MEMORY.as_mut_ptr(), HEAP_SIZE);
    }
    println!(">>> VOLATILE HEAP ALLOCATOR ONLINE: {} MB Reserved <<<", HEAP_SIZE / 1024 / 1024);
}
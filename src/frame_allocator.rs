use crate::BootInfo;
use crate::UefiMemoryDescriptor;
use crate::println;

const PAGE_SIZE: u64 = 4096;
// 1024 u64s = 65,536 individual bits. 
// 65,536 bits * 4 KiB per frame = 268 Megabytes of tracking capacity.
const BITMAP_SIZE: usize = 1024; 

pub struct BitmapAllocator {
    bitmap: [u64; BITMAP_SIZE],
    highest_frame: usize,
    free_frames: usize,
}

// The master state of all RAM chips.
// A bit set to '0' means Reserved/Allocated. A bit set to '1' means Free!
pub static mut FRAME_ALLOCATOR: BitmapAllocator = BitmapAllocator {
    bitmap: [0; BITMAP_SIZE], 
    highest_frame: 0,
    free_frames: 0,
};

pub unsafe fn init(boot_info: *const BootInfo) {
    println!("Initializing Physical Frame Bitmap Allocator...");
    let info = &*boot_info;

    let num_entries = info.memory_map_size / info.memory_map_desc_size;
    let mut total_free = 0;

    for i in 0..num_entries {
        let entry_ptr = (info.memory_map_addr as usize + i * info.memory_map_desc_size) as *const UefiMemoryDescriptor;
        let descriptor = &*entry_ptr;

        // Type 7 is "EfiConventionalMemory" -> Raw, untouched RAM.
        if descriptor.ty == 7 {
            let start_frame = (descriptor.physical_start / PAGE_SIZE) as usize;
            let end_frame = start_frame + descriptor.number_of_pages as usize;

            for frame_idx in start_frame..end_frame {
                // Ensure we don't overflow our 268 MB tracking limit
                if frame_idx < BITMAP_SIZE * 64 {
                    // Calculate which u64 array index, and which exact bit inside it!
                    let array_idx = frame_idx / 64;
                    let bit_idx = frame_idx % 64;
                    
                    // FLIP THE BIT TO 1 (FREE)
                    FRAME_ALLOCATOR.bitmap[array_idx] |= 1 << bit_idx;
                    FRAME_ALLOCATOR.free_frames += 1;
                    total_free += 1;

                    if frame_idx > FRAME_ALLOCATOR.highest_frame {
                        FRAME_ALLOCATOR.highest_frame = frame_idx;
                    }
                }
            }
        }
    }
    println!(">>> BITMAP ALLOCATOR ONLINE <<<");
    println!("Successfully mapped {} individual Free Frames ({} MB)", total_free, (total_free * 4096) / (1024 * 1024));
    println!("-----------------------------------");
}

// ---------------------------------------------------------
// NEW: The core function to grab RAM!
// ---------------------------------------------------------
pub unsafe fn allocate_frame() -> Option<u64> {
    for i in 0..=FRAME_ALLOCATOR.highest_frame {
        let array_idx = i / 64;
        let bit_idx = i % 64;

        // Is the bit a 1?
        if (FRAME_ALLOCATOR.bitmap[array_idx] & (1 << bit_idx)) != 0 {
            // Found one! Flip it back to 0 so no one else uses it.
            FRAME_ALLOCATOR.bitmap[array_idx] &= !(1 << bit_idx);
            FRAME_ALLOCATOR.free_frames -= 1;
            
            // Return the absolute physical memory address of the chip
            return Some(i as u64 * PAGE_SIZE);
        }
    }
    None // Out of memory!
}
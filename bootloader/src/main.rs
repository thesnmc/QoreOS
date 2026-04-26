#![no_main]
#![no_std]

use uefi::prelude::*;
use uefi::proto::console::gop::GraphicsOutput;
use xmas_elf::ElfFile;

#[repr(C)]
pub struct BootInfo {
    pub framebuffer_base: u64,
    pub framebuffer_size: usize,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub memory_map_size: usize,
    pub acpi2_rsdp_addr: u64, 
    // ---------------------------------------------------------
    // NEW: The Master Map to the Hardware RAM!
    // ---------------------------------------------------------
    pub memory_map_addr: u64,       // Physical pointer to the RAM Map
    pub memory_map_desc_size: usize,// The byte-size of each entry in the Map
}

#[entry]
fn efi_main(image: Handle, mut system_table: SystemTable<Boot>) -> Status {
    uefi_services::init(&mut system_table).unwrap();
    let boot_services = system_table.boot_services();

    log::info!("EdgeCore Bootloader initialized.");

    // 1. Find the ACPI 2.0 Hardware Directory Address
    let mut acpi_address = 0;
    for entry in system_table.config_table() {
        if entry.guid == uefi::table::cfg::ACPI2_GUID {
            acpi_address = entry.address as u64;
            break;
        }
    }

    // 2. Pack the backpack
    let mut boot_info = BootInfo {
        framebuffer_base: 0,
        framebuffer_size: 0,
        width: 0,
        height: 0,
        stride: 0,
        memory_map_size: 0,
        acpi2_rsdp_addr: acpi_address, 
        memory_map_addr: 0,       // Initialize our new variables
        memory_map_desc_size: 0,
    };

    // 3. Hijack the Framebuffer
    if let Ok(gop_handle) = boot_services.get_handle_for_protocol::<GraphicsOutput>() {
        if let Ok(mut gop) = boot_services.open_protocol_exclusive::<GraphicsOutput>(gop_handle) {
            let mode_info = gop.current_mode_info();
            let mut framebuffer = gop.frame_buffer();
            
            boot_info.framebuffer_base = framebuffer.as_mut_ptr() as u64;
            boot_info.framebuffer_size = framebuffer.size();
            boot_info.width = mode_info.resolution().0;
            boot_info.height = mode_info.resolution().1;
            boot_info.stride = mode_info.stride();
            
            unsafe { core::ptr::write_bytes(framebuffer.as_mut_ptr(), 0x11, framebuffer.size()); }
        }
    }

    // ---------------------------------------------------------
    // 4. NEW: Extract the Master Memory Map!
    // ---------------------------------------------------------
    // Find out exactly how big the memory map is right now
    let mmap_info = boot_services.memory_map_size();
    let mmap_alloc_size = mmap_info.map_size + 4096; // Add 4KB of padding for safety
    
    // Command UEFI to allocate an empty bucket of RAM for us
    let mmap_ptr = boot_services.allocate_pool(uefi::table::boot::MemoryType::LOADER_DATA, mmap_alloc_size).unwrap();
    let mmap_slice = unsafe { core::slice::from_raw_parts_mut(mmap_ptr, mmap_alloc_size) };
    
    // Tell UEFI to dump the entire physical hardware layout into our bucket!
    boot_services.memory_map(mmap_slice).unwrap();

    // Pack the physical coordinates of the map into our backpack
    boot_info.memory_map_addr = mmap_ptr as u64;
    boot_info.memory_map_size = mmap_info.map_size;
    boot_info.memory_map_desc_size = mmap_info.entry_size;


    // 5. Load the Kernel into Memory
    let kernel_bytes = include_bytes!("../../edgecore_kernel/target/x86_64-edgecore/debug/edgecore_kernel");
    let elf = ElfFile::new(kernel_bytes).expect("Failed to parse Kernel ELF!");

    for ph in elf.program_iter() {
        if let Ok(xmas_elf::program::Type::Load) = ph.get_type() {
            let offset = ph.offset() as usize;
            let size = ph.file_size() as usize;
            let virt_addr = ph.virtual_addr() as usize;
            
            unsafe {
                core::ptr::copy_nonoverlapping(
                    kernel_bytes.as_ptr().add(offset),
                    virt_addr as *mut u8,
                    size,
                );
            }
        }
    }

    // 6. The Jump
    let entry_point = elf.header.pt2.entry_point() as usize;
    let kernel_main: extern "sysv64" fn(*const BootInfo) -> ! = unsafe { core::mem::transmute(entry_point) };
    
    log::info!("Executing Ring 0 Handoff...");
    kernel_main(&boot_info);
}
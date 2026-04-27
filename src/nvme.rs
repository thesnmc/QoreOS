use core::ptr::{read_volatile, write_volatile};
use alloc::alloc::{alloc_zeroed, Layout};

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct NvmeCmd {
    pub cdw0: u32, pub nsid: u32, pub reserved: u64, pub mptr: u64,
    pub prp1: u64, pub prp2: u64, pub cdw10: u32, pub cdw11: u32,
    pub cdw12: u32, pub cdw13: u32, pub cdw14: u32, pub cdw15: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct NvmeCompletion {
    pub cdw0: u32, pub reserved: u32, pub sqhead: u16, pub sqid: u16,
    pub cid: u16, pub status: u16,
}

// --- UPGRADE: Added permanent I/O Queue Pointers and State Trackers ---
pub struct Nvme {
    bar0: u64,
    pub asq_ptr: u64,
    pub acq_ptr: u64,
    pub iosq_ptr: u64,
    pub iocq_ptr: u64,
    pub iosq_tail: u32,
    pub iocq_head: u32,
}

impl Nvme {
    pub fn new(bar0: u64) -> Self {
        Nvme { bar0, asq_ptr: 0, acq_ptr: 0, iosq_ptr: 0, iocq_ptr: 0, iosq_tail: 0, iocq_head: 0 }
    }

    unsafe fn read_reg(&self, offset: u64) -> u32 { read_volatile((self.bar0 + offset) as *const u32) }
    unsafe fn write_reg(&self, offset: u64, val: u32) { write_volatile((self.bar0 + offset) as *mut u32, val); }
    unsafe fn write_reg64(&self, offset: u64, val: u64) { write_volatile((self.bar0 + offset) as *mut u64, val); }

    pub fn init(&mut self) {
        let test_read = unsafe { self.read_reg(0x1C) };
        if test_read == 0xFFFFFFFF { return; }

        let mut cc = unsafe { self.read_reg(0x14) };
        cc &= !1; 
        unsafe { self.write_reg(0x14, cc); }

        let mut timeout = 0;
        while unsafe { self.read_reg(0x1C) & 1 } != 0 { 
            unsafe { core::arch::asm!("nop"); } 
            timeout += 1;
            if timeout > 10_000_000 { return; }
        }

        let num_entries = 16;
        // Allocate both Admin AND I/O Queues at Boot
        self.asq_ptr = unsafe { alloc_zeroed(Layout::from_size_align(num_entries * 64, 4096).unwrap()) } as u64;
        self.acq_ptr = unsafe { alloc_zeroed(Layout::from_size_align(num_entries * 16, 4096).unwrap()) } as u64;
        self.iosq_ptr = unsafe { alloc_zeroed(Layout::from_size_align(num_entries * 64, 4096).unwrap()) } as u64;
        self.iocq_ptr = unsafe { alloc_zeroed(Layout::from_size_align(num_entries * 16, 4096).unwrap()) } as u64;

        let aqa = ((num_entries as u32 - 1) << 16) | (num_entries as u32 - 1);
        unsafe { self.write_reg(0x24, aqa); }

        unsafe { 
            self.write_reg64(0x28, self.asq_ptr);
            self.write_reg64(0x30, self.acq_ptr);
        }

        let cc_new = (1 << 0) | (6 << 16) | (4 << 20); 
        unsafe { self.write_reg(0x14, cc_new); }

        timeout = 0;
        while unsafe { self.read_reg(0x1C) & 1 } == 0 { 
            unsafe { core::arch::asm!("nop"); } 
            timeout += 1;
            if timeout > 10_000_000 { return; }
        }

        // --- UPGRADE: Create the I/O Queues EXACTLY ONCE here ---
        let asq = unsafe { core::slice::from_raw_parts_mut(self.asq_ptr as *mut NvmeCmd, 16) };
        let acq = unsafe { core::slice::from_raw_parts_mut(self.acq_ptr as *mut NvmeCompletion, 16) };

        asq[0] = NvmeCmd { cdw0: 0x05 | (1 << 16), nsid: 0, reserved: 0, mptr: 0, prp1: self.iocq_ptr, prp2: 0, cdw10: (15 << 16) | 1, cdw11: 1, cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0 };
        unsafe { self.write_reg(0x1000, 1); } 
        timeout = 0; while (acq[0].status & 0x01) == 0 { timeout += 1; if timeout > 10_000_000 { break; } }
        unsafe { self.write_reg(0x1004, 1); } 

        asq[1] = NvmeCmd { cdw0: 0x01 | (2 << 16), nsid: 0, reserved: 0, mptr: 0, prp1: self.iosq_ptr, prp2: 0, cdw10: (15 << 16) | 1, cdw11: (1 << 16) | 1, cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0 };
        unsafe { self.write_reg(0x1000, 2); } 
        timeout = 0; while (acq[1].status & 0x01) == 0 { timeout += 1; if timeout > 10_000_000 { break; } }
        unsafe { self.write_reg(0x1004, 2); }

        unsafe { crate::compositor::terminal_print("SYS: NVMe Admin Rings DMA Mapped. Drive READY!\n", 0x10B981); }
    }

    pub fn identify_controller(&mut self) {
        let buffer_layout = Layout::from_size_align(4096, 4096).unwrap();
        let data_ptr = unsafe { alloc_zeroed(buffer_layout) } as u64;

        let asq = unsafe { core::slice::from_raw_parts_mut(self.asq_ptr as *mut NvmeCmd, 16) };
        // Shift to index 2 because indices 0 and 1 were used to create the I/O queues!
        asq[2] = NvmeCmd { cdw0: 0x06 | (0xAA << 16), nsid: 0, reserved: 0, mptr: 0, prp1: data_ptr, prp2: 0, cdw10: 1, cdw11: 0, cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0 };

        unsafe { self.write_reg(0x1000, 3); }

        let acq = unsafe { core::slice::from_raw_parts_mut(self.acq_ptr as *mut NvmeCompletion, 16) };
        let mut timeout = 0;
        while (acq[2].status & 0x01) == 0 {
            unsafe { core::arch::asm!("nop"); }
            timeout += 1;
            if timeout > 10_000_000 { return; }
        }

        let data = unsafe { core::slice::from_raw_parts(data_ptr as *const u8, 4096) };
        
        let mut model_bytes = [0u8; 40];
        let mut actual_len = 0;
        for i in 0..40 {
            let b = data[24 + i];
            if b >= 32 && b <= 126 { 
                model_bytes[actual_len] = b;
                actual_len += 1;
            }
        }

        let mut serial_bytes = [0u8; 20];
        let mut serial_len = 0;
        for i in 0..20 {
            let b = data[4 + i];
            if b >= 32 && b <= 126 {
                serial_bytes[serial_len] = b;
                serial_len += 1;
            }
        }

        if let Ok(model_str) = core::str::from_utf8(&model_bytes[..actual_len]) {
            if let Ok(serial_str) = core::str::from_utf8(&serial_bytes[..serial_len]) {
                unsafe { 
                    crate::compositor::terminal_print("\n> SILICON IDENTIFIED -> ", 0x3B82F6); 
                    crate::compositor::terminal_print(model_str, 0x10B981); 
                    crate::compositor::terminal_print(" (SN: ", 0x3B82F6);
                    crate::compositor::terminal_print(serial_str, 0x10B981); 
                    crate::compositor::terminal_print(")\n", 0x3B82F6);
                }
            }
        }

        unsafe { self.write_reg(0x1004, 3); }
    }

    // --- UPGRADE: Stateful DMA Engine ---
    pub fn read_sector(&mut self, lba: u64) -> [u8; 512] {
        let data_ptr = unsafe { alloc_zeroed(Layout::from_size_align(4096, 4096).unwrap()) } as u64;
        let iosq = unsafe { core::slice::from_raw_parts_mut(self.iosq_ptr as *mut NvmeCmd, 16) };
        let iocq = unsafe { core::slice::from_raw_parts_mut(self.iocq_ptr as *mut NvmeCompletion, 16) };
        
        let cmd_idx = self.iosq_tail as usize;
        iocq[cmd_idx].status = 0; // Clear previous completion status
        
        let cdw10 = (lba & 0xFFFFFFFF) as u32;
        let cdw11 = (lba >> 32) as u32;
        
        let cid = cmd_idx as u32;
        iosq[cmd_idx] = NvmeCmd { cdw0: 0x02 | (cid << 16), nsid: 1, reserved: 0, mptr: 0, prp1: data_ptr, prp2: 0, cdw10, cdw11, cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0 };
        
        // Ring the Doorbell
        self.iosq_tail = (self.iosq_tail + 1) % 16;
        unsafe { self.write_reg(0x1008, self.iosq_tail); } 
        
        // Wait for Hardware Completion
        let mut timeout = 0;
        let cq_idx = self.iocq_head as usize;
        while (iocq[cq_idx].status & 0x01) == 0 {
            unsafe { core::arch::asm!("nop"); }
            timeout += 1;
            if timeout > 20_000_000 { break; }
        }
        
        // Acknowledge Hardware Completion
        self.iocq_head = (self.iocq_head + 1) % 16;
        unsafe { self.write_reg(0x100C, self.iocq_head); } 
        
        let mut sector = [0u8; 512];
        let raw_data = unsafe { core::slice::from_raw_parts(data_ptr as *const u8, 512) };
        sector.copy_from_slice(raw_data);
        sector
    }
}
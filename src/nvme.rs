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

pub struct Nvme {
    bar0: u64,
    pub asq_ptr: u64,
    pub acq_ptr: u64,
}

impl Nvme {
    pub fn new(bar0: u64) -> Self {
        Nvme { bar0, asq_ptr: 0, acq_ptr: 0 }
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
        let asq_layout = Layout::from_size_align(num_entries * 64, 4096).unwrap();
        self.asq_ptr = unsafe { alloc_zeroed(asq_layout) } as u64;

        let acq_layout = Layout::from_size_align(num_entries * 16, 4096).unwrap();
        self.acq_ptr = unsafe { alloc_zeroed(acq_layout) } as u64;

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

        unsafe { crate::compositor::terminal_print("SYS: NVMe Admin Rings DMA Mapped. Drive READY!\n", 0x10B981); }
    }

    pub fn identify_controller(&mut self) {
        let buffer_layout = Layout::from_size_align(4096, 4096).unwrap();
        let data_ptr = unsafe { alloc_zeroed(buffer_layout) } as u64;

        let asq = unsafe { core::slice::from_raw_parts_mut(self.asq_ptr as *mut NvmeCmd, 16) };
        asq[0] = NvmeCmd { cdw0: 0, nsid: 0, reserved: 0, mptr: 0, prp1: 0, prp2: 0, cdw10: 0, cdw11: 0, cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0 };
        asq[0].cdw0 = 0x06 | (0xAA << 16); 
        asq[0].prp1 = data_ptr; 
        asq[0].cdw10 = 1; 

        unsafe { self.write_reg(0x1000, 1); }

        let acq = unsafe { core::slice::from_raw_parts_mut(self.acq_ptr as *mut NvmeCompletion, 16) };
        let mut timeout = 0;
        while (acq[0].status & 0x01) == 0 {
            unsafe { core::arch::asm!("nop"); }
            timeout += 1;
            if timeout > 10_000_000 { return; }
        }

        let data = unsafe { core::slice::from_raw_parts(data_ptr as *const u8, 4096) };
        
        // Extract Model Number (Bytes 24 to 63)
        let mut model_bytes = [0u8; 40];
        let mut actual_len = 0;
        for i in 0..40 {
            let b = data[24 + i];
            if b >= 32 && b <= 126 { 
                model_bytes[actual_len] = b;
                actual_len += 1;
            }
        }

        // Extract Serial Number (Bytes 4 to 23)
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
                    crate::compositor::terminal_print("\n> SILICON IDENTIFIED -> ", 0x3B82F6); // Blue
                    crate::compositor::terminal_print(model_str, 0x10B981); // GREEN INK!
                    crate::compositor::terminal_print(" (SN: ", 0x3B82F6);
                    crate::compositor::terminal_print(serial_str, 0x10B981); // GREEN INK!
                    crate::compositor::terminal_print(")\n", 0x3B82F6);
                }
            }
        }

        unsafe { self.write_reg(0x1004, 1); }
    }

    // --- THE MISSING DMA FUNCTION ---
    pub fn read_sector_zero(&mut self) -> [u8; 512] {
        let io_sq_ptr = unsafe { alloc_zeroed(Layout::from_size_align(128, 4096).unwrap()) } as u64;
        let io_cq_ptr = unsafe { alloc_zeroed(Layout::from_size_align(32, 4096).unwrap()) } as u64;
        let data_ptr = unsafe { alloc_zeroed(Layout::from_size_align(4096, 4096).unwrap()) } as u64;

        let asq = unsafe { core::slice::from_raw_parts_mut(self.asq_ptr as *mut NvmeCmd, 16) };
        let acq = unsafe { core::slice::from_raw_parts_mut(self.acq_ptr as *mut NvmeCompletion, 16) };

        // 1. Create I/O Completion Queue (CQID = 1)
        asq[1] = NvmeCmd { cdw0: 0x05 | (1 << 16), nsid: 0, reserved: 0, mptr: 0, prp1: io_cq_ptr, prp2: 0, cdw10: (1 << 16) | 1, cdw11: 1, cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0 };
        unsafe { self.write_reg(0x1000, 2); } 
        let mut timeout = 0; while (acq[1].status & 0x01) == 0 { timeout += 1; if timeout > 10_000_000 { break; } }
        unsafe { self.write_reg(0x1004, 2); } 

        // 2. Create I/O Submission Queue (SQID = 1, CQID = 1)
        asq[2] = NvmeCmd { cdw0: 0x01 | (2 << 16), nsid: 0, reserved: 0, mptr: 0, prp1: io_sq_ptr, prp2: 0, cdw10: (1 << 16) | 1, cdw11: (1 << 16) | 1, cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0 };
        unsafe { self.write_reg(0x1000, 3); } 
        timeout = 0; while (acq[2].status & 0x01) == 0 { timeout += 1; if timeout > 10_000_000 { break; } }
        unsafe { self.write_reg(0x1004, 3); }

        // 3. Submit READ Command to I/O SQ (Opcode 0x02, NSID=1, LBA=0)
        let iosq = unsafe { core::slice::from_raw_parts_mut(io_sq_ptr as *mut NvmeCmd, 2) };
        let iocq = unsafe { core::slice::from_raw_parts_mut(io_cq_ptr as *mut NvmeCompletion, 2) };
        
        iosq[0] = NvmeCmd { cdw0: 0x02, nsid: 1, reserved: 0, mptr: 0, prp1: data_ptr, prp2: 0, cdw10: 0, cdw11: 0, cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0 };
        unsafe { self.write_reg(0x1008, 1); } // IOSQ Doorbell Tail
        
        timeout = 0; while (iocq[0].status & 0x01) == 0 { timeout += 1; if timeout > 10_000_000 { break; } }
        unsafe { self.write_reg(0x100C, 1); } // IOCQ Doorbell Head

        // 4. Return Sector Data
        let mut sector = [0u8; 512];
        let raw_data = unsafe { core::slice::from_raw_parts(data_ptr as *const u8, 512) };
        sector.copy_from_slice(raw_data);
        sector
    }
}
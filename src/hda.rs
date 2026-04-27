use core::ptr::{read_volatile, write_volatile};
use alloc::alloc::{alloc_zeroed, Layout};

pub struct IntelHda {
    bar0: u64,
    pub corb_ptr: u64,
    pub rirb_ptr: u64,
}

impl IntelHda {
    pub fn new(bar0: u64) -> Self {
        IntelHda { bar0, corb_ptr: 0, rirb_ptr: 0 }
    }

    unsafe fn read_reg(&self, offset: u64) -> u32 { read_volatile((self.bar0 + offset) as *const u32) }
    unsafe fn read_reg16(&self, offset: u64) -> u16 { read_volatile((self.bar0 + offset) as *const u16) }
    unsafe fn read_reg8(&self, offset: u64) -> u8 { read_volatile((self.bar0 + offset) as *const u8) }
    unsafe fn write_reg(&self, offset: u64, val: u32) { write_volatile((self.bar0 + offset) as *mut u32, val); }
    unsafe fn write_reg16(&self, offset: u64, val: u16) { write_volatile((self.bar0 + offset) as *mut u16, val); }
    unsafe fn write_reg8(&self, offset: u64, val: u8) { write_volatile((self.bar0 + offset) as *mut u8, val); }

    pub fn init(&mut self) {
        let _gcap = unsafe { self.read_reg(0x00) };
        unsafe { crate::compositor::terminal_print("\n> HDA: AUDIO SILICON DETECTED AND MAPPED.\n", 0x3B82F6); }
        
        // 1. Reset Controller
        unsafe { self.write_reg(0x08, 1); }
        let mut timeout = 0;
        while unsafe { self.read_reg(0x08) & 1 } == 0 {
            unsafe { core::arch::asm!("nop"); }
            timeout += 1;
            if timeout > 10_000_000 {
                unsafe { crate::compositor::terminal_print("> HDA ERROR: CONTROLLER RESET TIMEOUT!\n", 0xEF4444); }
                return;
            }
        }
        
        for _ in 0..10_000 { unsafe { core::arch::asm!("nop"); } }
        
        unsafe { 
            crate::compositor::terminal_print("> HDA: CONTROLLER AWAKE. ALLOCATING DMA RINGS...\n", 0x3B82F6); 
            
            // 2. Allocate CORB/RIRB
            let layout = Layout::from_size_align(4096, 4096).unwrap();
            self.corb_ptr = alloc_zeroed(layout) as u64;
            self.rirb_ptr = alloc_zeroed(layout) as u64;

            self.write_reg8(0x4C, 0); 
            self.write_reg8(0x5C, 0); 

            self.write_reg(0x40, (self.corb_ptr & 0xFFFFFFFF) as u32);
            self.write_reg(0x44, (self.corb_ptr >> 32) as u32);
            self.write_reg(0x50, (self.rirb_ptr & 0xFFFFFFFF) as u32);
            self.write_reg(0x54, (self.rirb_ptr >> 32) as u32);

            self.write_reg8(0x4E, 0x02);
            self.write_reg8(0x5E, 0x02);

            self.write_reg16(0x4A, 1 << 15); 
            for _ in 0..100 { core::arch::asm!("nop"); }
            self.write_reg16(0x4A, 0); 
            self.write_reg16(0x48, 0); 
            self.write_reg16(0x58, 1 << 15); 
            
            self.write_reg8(0x4C, 1 << 1); 
            self.write_reg8(0x5C, 1 << 1); 

            crate::compositor::terminal_print("> HDA: CORB & RIRB RINGS DMA MAPPED AND RUNNING.\n", 0x10B981);
            
            let statests = self.read_reg16(0x0E); 
            if statests != 0 {
                crate::compositor::terminal_print("> HDA: EXTERNAL AUDIO CODEC DETECTED ON LINK!\n", 0x10B981);
            }
        }
    }

    // --- Send a Command to the DAC via the CORB Ring ---
    pub fn send_corb_cmd(&mut self, cmd: u32) {
        let corb = unsafe { core::slice::from_raw_parts_mut(self.corb_ptr as *mut u32, 256) };
        let mut wp = unsafe { self.read_reg16(0x48) };
        wp = (wp + 1) % 256;
        corb[wp as usize] = cmd;
        unsafe { self.write_reg16(0x48, wp); }
    }

    // --- The DMA Audio Extraction Pipeline ---
    pub fn play_tone(&mut self) {
        // 1. Synthesize 440Hz Square Wave in RAM (64 KB buffer)
        let buffer_size = 65536; 
        let audio_buf = unsafe { alloc_zeroed(Layout::from_size_align(buffer_size, 4096).unwrap()) } as *mut i16;
        let samples = buffer_size / 2; // 16-bit samples
        
        // UPGRADE: Adjusted period for 48kHz sample rate
        let period = 109; 
        
        for i in 0..samples {
            // High for half period, Low for half period
            let val = if (i % period) < (period / 2) { 8000 } else { -8000 };
            unsafe { *audio_buf.add(i) = val; }
        }

        // 2. Build Buffer Descriptor List (BDL) - Points the hardware to our wave buffer
        let bdl_ptr = unsafe { alloc_zeroed(Layout::from_size_align(4096, 128).unwrap()) } as *mut u32;
        unsafe {
            *bdl_ptr.add(0) = (audio_buf as u64 & 0xFFFFFFFF) as u32; // Lower 32 bits
            *bdl_ptr.add(1) = ((audio_buf as u64) >> 32) as u32;      // Upper 32 bits
            *bdl_ptr.add(2) = buffer_size as u32;                     // Length
            *bdl_ptr.add(3) = 0;                                      // Interrupts off
        }

        // 3. Find Output Stream 1 (OS1) Memory Offset
        let gcap = unsafe { self.read_reg(0x00) };
        let iss = (gcap >> 8) & 0x0F;
        let stream_offset = 0x80 + (iss * 0x20) as u64; 

        // Ensure stream is stopped and reset
        unsafe { self.write_reg8(stream_offset, 0); } 
        unsafe { self.write_reg8(stream_offset, 1); } 
        let mut timeout = 0; while unsafe { self.read_reg8(stream_offset) & 1 } == 0 { timeout += 1; if timeout > 10000 { break; } }
        unsafe { self.write_reg8(stream_offset, 0); } 
        timeout = 0; while unsafe { self.read_reg8(stream_offset) & 1 } != 0 { timeout += 1; if timeout > 10000 { break; } }

        // Configure DMA Stream Parameters
        unsafe {
            self.write_reg(stream_offset + 0x18, (bdl_ptr as u64 & 0xFFFFFFFF) as u32); // BDLPL
            self.write_reg(stream_offset + 0x1C, ((bdl_ptr as u64) >> 32) as u32);      // BDLPU
            self.write_reg(stream_offset + 0x08, buffer_size as u32);                   // CBL (Cyclic Length)
            self.write_reg16(stream_offset + 0x0C, 0);                                  // LVI (Only 1 BDL entry)
            
            // UPGRADE: Set format to 48kHz, 16-bit, Stereo (0x0011) instead of 44.1kHz (0x4011)
            self.write_reg16(stream_offset + 0x12, 0x0011);                             
        }

        // 4. Send Hardware Codec Commands via CORB Mailbox
        self.send_corb_cmd(0x00270500); // Node 2 (DAC): Power State D0 (Fully On)
        self.send_corb_cmd(0x00370500); // Node 3 (Pin): Power State D0 (Fully On)
        self.send_corb_cmd(0x00270610); // Assign DAC to Output Stream 1, Channel 0
        
        // UPGRADE: Tell DAC to expect 48kHz (0x0011)
        self.send_corb_cmd(0x00220011); 
        
        self.send_corb_cmd(0x0023B07F); // Unmute DAC, Max Volume
        self.send_corb_cmd(0x00370740); // Enable Pin Output Signal
        self.send_corb_cmd(0x0033B07F); // Unmute Pin, Max Volume

        // UPGRADE: Give the DAC a bit more time to parse commands
        for _ in 0..100_000 { unsafe { core::arch::asm!("nop"); } }

        // 5. Fire the DMA Engine!
        let mut sdctl = unsafe { self.read_reg8(stream_offset) };
        sdctl |= 1 << 4; // Assign Stream Number 1
        unsafe { self.write_reg8(stream_offset, sdctl); }
        
        // UPGRADE THE FIX: Bit 1 is RUN. Bit 0 is RESET. We MUST use (1 << 1).
        unsafe { self.write_reg8(stream_offset, sdctl | (1 << 1)); }

        unsafe { crate::compositor::terminal_print("> HDA: DMA OUTPUT FIRED. 440HZ TONE STREAMING!\n", 0xF59E0B); }
    }
}
use x86_64::instructions::port::Port;
use core::sync::atomic::{AtomicI32, AtomicU8, Ordering};

// ---------------------------------------------------------
// THE LOCK-FREE STATE ENGINE
// ---------------------------------------------------------
// These atomics allow the CPU Interrupt to inject coordinates at light-speed
// without ever using a Mutex, meaning the UI can read them with zero latency.
pub static MOUSE_X: AtomicI32 = AtomicI32::new(400); // Start in middle of an 800px screen
pub static MOUSE_Y: AtomicI32 = AtomicI32::new(300);
pub static MOUSE_BTN: AtomicU8 = AtomicU8::new(0);

static mut MOUSE_CYCLE: u8 = 0;
static mut MOUSE_PACKET: [u8; 3] = [0; 3];

// ---------------------------------------------------------
// HARDWARE INITIALIZATION
// ---------------------------------------------------------
pub fn init() {
    let mut cmd_port = Port::<u8>::new(0x64);
    let mut data_port = Port::<u8>::new(0x60);

    unsafe {
        // 1. Enable the Auxiliary Device (The Mouse Port)
        wait_write();
        cmd_port.write(0xA8);

        // 2. Tell the motherboard to route Mouse interrupts to IRQ 12
        wait_write();
        cmd_port.write(0x20); // Read Configuration Byte
        wait_read();
        let mut status = data_port.read();
        status |= 1 << 1; // Set bit 1 (Enable IRQ 12)
        status &= !(1 << 5); // Clear bit 5 (Disable Mouse Clock line)
        wait_write();
        cmd_port.write(0x60); // Write Configuration Byte
        wait_write();
        data_port.write(status);

        // 3. Tell the mouse to use default settings
        mouse_write(0xF6);
        mouse_read(); // Acknowledge (0xFA)

        // 4. Enable Data Reporting (Start firing IRQ 12!)
        mouse_write(0xF4);
        mouse_read(); // Acknowledge (0xFA)
        
        crate::compositor::terminal_print("SYS: PS/2 Mouse Controller Awake. IRQ 12 Routed.\n", 0x10B981);
    }
}

// ---------------------------------------------------------
// THE INTERRUPT PACKET PROCESSOR
// ---------------------------------------------------------
// This is called directly by the CPU every time a byte arrives
pub fn process_packet_byte(byte: u8) {
    unsafe {
        match MOUSE_CYCLE {
            0 => {
                // Byte 0 must have bit 3 set to be valid
                if (byte & 0x08) != 0 {
                    MOUSE_PACKET[0] = byte;
                    MOUSE_CYCLE += 1;
                }
            }
            1 => {
                MOUSE_PACKET[1] = byte;
                MOUSE_CYCLE += 1;
            }
            2 => {
                MOUSE_PACKET[2] = byte;
                
                // We have a full 3-byte packet! Let's decode it.
                let state = MOUSE_PACKET[0];
                let mut dx = MOUSE_PACKET[1] as i32;
                let mut dy = MOUSE_PACKET[2] as i32;

                // Handle the 9-bit negative sign extensions
                if (state & (1 << 4)) != 0 { dx |= !0xFF; }
                if (state & (1 << 5)) != 0 { dy |= !0xFF; }

                // Note: PS/2 Y-axis goes UP, but our screen Y-axis goes DOWN.
                dy = -dy; 

                // Inject directly into the lock-free atomics!
                MOUSE_X.fetch_add(dx, Ordering::Relaxed);
                MOUSE_Y.fetch_add(dy, Ordering::Relaxed);
                
                // Left Click = Bit 0, Right Click = Bit 1
                MOUSE_BTN.store(state & 0x03, Ordering::Relaxed);

                // Reset for the next movement
                MOUSE_CYCLE = 0;
            }
            _ => MOUSE_CYCLE = 0,
        }
    }
}

// ---------------------------------------------------------
// HARDWARE HELPER FUNCTIONS
// ---------------------------------------------------------
unsafe fn wait_write() {
    let mut cmd = Port::<u8>::new(0x64);
    for _ in 0..100_000 { if (cmd.read() & 2) == 0 { return; } }
}

unsafe fn wait_read() {
    let mut cmd = Port::<u8>::new(0x64);
    for _ in 0..100_000 { if (cmd.read() & 1) == 1 { return; } }
}

unsafe fn mouse_write(data: u8) {
    let mut cmd = Port::<u8>::new(0x64);
    let mut data_port = Port::<u8>::new(0x60);
    wait_write();
    cmd.write(0xD4); // Tell controller we are talking to the mouse
    wait_write();
    data_port.write(data);
}

unsafe fn mouse_read() -> u8 {
    let mut data_port = Port::<u8>::new(0x60);
    wait_read();
    data_port.read()
}
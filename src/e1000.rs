use crate::println;
use core::ptr::{read_volatile, write_volatile};
use alloc::vec::Vec;
use alloc::vec;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct RxDescriptor {
    pub buffer_address: u64, 
    pub length: u16,         
    pub checksum: u16,       
    pub status: u8,          
    pub errors: u8,
    pub special: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct TxDescriptor {
    pub buffer_address: u64,
    pub length: u16,
    pub cso: u8,
    pub cmd: u8,
    pub status: u8,
    pub css: u8,
    pub special: u16,
}

pub struct E1000 {
    bar0_address: u64,
    rx_ring_ptr: u64,
    current_rx_bucket: usize,
    tx_ring_ptr: u64,
    current_tx_bucket: usize,
}

impl E1000 {
    pub fn new(bar0: u64) -> Self {
        E1000 { 
            bar0_address: bar0, rx_ring_ptr: 0, current_rx_bucket: 0, tx_ring_ptr: 0, current_tx_bucket: 0,
        }
    }

    unsafe fn write_register(&self, offset: u64, value: u32) {
        let ptr = (self.bar0_address + offset) as *mut u32;
        write_volatile(ptr, value);
    }

    pub fn init_rx_ring(&mut self) {
        let num_descriptors = 32;
        let mut rx_ring: Vec<RxDescriptor> = Vec::with_capacity(num_descriptors);
        for _ in 0..num_descriptors {
            let buffer: Vec<u8> = vec![0; 2048];
            let buffer_ptr = alloc::boxed::Box::leak(buffer.into_boxed_slice()).as_ptr() as u64;
            rx_ring.push(RxDescriptor {
                buffer_address: buffer_ptr, length: 0, checksum: 0, status: 0, errors: 0, special: 0,
            });
        }
        let ring_ptr = alloc::boxed::Box::leak(rx_ring.into_boxed_slice()).as_ptr() as u64;
        self.rx_ring_ptr = ring_ptr;
        unsafe {
            self.write_register(0x2800, (ring_ptr & 0xFFFFFFFF) as u32);
            self.write_register(0x2808, (num_descriptors * 16) as u32);
            self.write_register(0x2810, 0); 
            self.write_register(0x2818, (num_descriptors - 1) as u32); 
            self.write_register(0x0100, (1 << 1) | (1 << 15));
        }
        println!(">>> RECEIVE DMA ACTIVE <<<");
    }

    pub fn init_tx_ring(&mut self) {
        let num_descriptors = 8;
        let mut tx_ring: Vec<TxDescriptor> = Vec::with_capacity(num_descriptors);
        for _ in 0..num_descriptors {
            tx_ring.push(TxDescriptor {
                buffer_address: 0, length: 0, cso: 0, cmd: 0, status: 0, css: 0, special: 0,
            });
        }
        let ring_ptr = alloc::boxed::Box::leak(tx_ring.into_boxed_slice()).as_ptr() as u64;
        self.tx_ring_ptr = ring_ptr;
        unsafe {
            self.write_register(0x3800, (ring_ptr & 0xFFFFFFFF) as u32); 
            self.write_register(0x3808, (num_descriptors * 16) as u32);  
            self.write_register(0x3810, 0); 
            self.write_register(0x3818, 0); 
            self.write_register(0x0400, (1 << 1) | (1 << 3));
        }
        println!(">>> TRANSMIT DMA ACTIVE <<<");
    }

    pub fn shout(&mut self) {
        let mut frame: Vec<u8> = vec![
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 
            0x52, 0x54, 0x00, 0x12, 0x34, 0x56, 
            0x08, 0x06,                         
            0x00, 0x01, 0x08, 0x00, 0x06, 0x04, 0x00, 0x01, 
            0x52, 0x54, 0x00, 0x12, 0x34, 0x56, 
            10, 0, 2, 15,                       
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 
            10, 0, 2, 2,                        
        ];
        frame.resize(60, 0);
        let frame_ptr = alloc::boxed::Box::leak(frame.into_boxed_slice()).as_ptr() as u64;
        let descriptor_addr = self.tx_ring_ptr + (self.current_tx_bucket as u64 * 16);
        let descriptor_ptr = descriptor_addr as *mut TxDescriptor;
        unsafe {
            (*descriptor_ptr).buffer_address = frame_ptr;
            (*descriptor_ptr).length = 60;
            (*descriptor_ptr).cmd = (1 << 0) | (1 << 1) | (1 << 3);
        }
        self.current_tx_bucket = (self.current_tx_bucket + 1) % 8;
        unsafe { self.write_register(0x3818, self.current_tx_bucket as u32); }
        println!(">>> SHOUTING INTO THE VOID (Broadcast ARP Transmitted!) <<<");
    }

    pub fn ping(&mut self, target_mac: [u8; 6]) {
        let mut frame: Vec<u8> = vec![
            target_mac[0], target_mac[1], target_mac[2], target_mac[3], target_mac[4], target_mac[5],
            0x52, 0x54, 0x00, 0x12, 0x34, 0x56, 
            0x08, 0x00, 
            0x45, 0x00, 0x00, 0x3C, 
            0x12, 0x34, 0x40, 0x00, 
            0x40, 0x01, 0x10, 0x7D, 
            10, 0, 2, 15,           
            10, 0, 2, 2,            
            0x08, 0x00, 0x95, 0x5D, 
            0x00, 0x01, 0x00, 0x01, 
            0x45, 0x64, 0x67, 0x65, 0x43, 0x6F, 0x72, 0x65, 
            0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,0,
        ];

        let frame_ptr = alloc::boxed::Box::leak(frame.into_boxed_slice()).as_ptr() as u64;
        let descriptor_addr = self.tx_ring_ptr + (self.current_tx_bucket as u64 * 16);
        let descriptor_ptr = descriptor_addr as *mut TxDescriptor;
        unsafe {
            (*descriptor_ptr).buffer_address = frame_ptr;
            (*descriptor_ptr).length = 74;
            (*descriptor_ptr).cmd = (1 << 0) | (1 << 1) | (1 << 3);
        }
        self.current_tx_bucket = (self.current_tx_bucket + 1) % 8;
        unsafe { self.write_register(0x3818, self.current_tx_bucket as u32); }
    }

    // ---------------------------------------------------------
    // NEW: The Stateless UDP Cannon
    // ---------------------------------------------------------
    pub fn udp_broadcast(&mut self) {
        let mut frame: Vec<u8> = vec![
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // Dst MAC (Broadcast)
            0x52, 0x54, 0x00, 0x12, 0x34, 0x56, // Src MAC
            0x08, 0x00,                         // EtherType: IPv4

            0x45, 0x00, 0x00, 0x3A, // IPv4 Length: 58 bytes
            0x00, 0x00, 0x40, 0x00, // ID, Flags
            0x40, 0x11, 0x00, 0x00, // TTL, Protocol (17 = UDP)
            10, 0, 2, 15,           // Src IP
            255, 255, 255, 255,     // Dst IP

            0x1A, 0x0A, // Src Port 6666
            0x1A, 0x0B, // Dst Port 6667
            0x00, 0x26, // UDP Length: 38 bytes
            0x00, 0x00, // Checksum

            // Payload: "EDGECORE SOVEREIGN NODE ONLINE"
            b'E',b'D',b'G',b'E',b'C',b'O',b'R',b'E',b' ',
            b'S',b'O',b'V',b'E',b'R',b'E',b'I',b'G',b'N',b' ',
            b'N',b'O',b'D',b'E',b' ',
            b'O',b'N',b'L',b'I',b'N',b'E'
        ];

        let frame_ptr = alloc::boxed::Box::leak(frame.into_boxed_slice()).as_ptr() as u64;
        let descriptor_addr = self.tx_ring_ptr + (self.current_tx_bucket as u64 * 16);
        let descriptor_ptr = descriptor_addr as *mut TxDescriptor;
        unsafe {
            (*descriptor_ptr).buffer_address = frame_ptr;
            (*descriptor_ptr).length = 72;
            (*descriptor_ptr).cmd = (1 << 0) | (1 << 1) | (1 << 3);
        }
        self.current_tx_bucket = (self.current_tx_bucket + 1) % 8;
        unsafe { self.write_register(0x3818, self.current_tx_bucket as u32); }
    }

    pub fn poll(&mut self) -> bool {
        let descriptor_addr = self.rx_ring_ptr + (self.current_rx_bucket as u64 * 16);
        let descriptor = unsafe { core::ptr::read_volatile(descriptor_addr as *const RxDescriptor) };

        if (descriptor.status & 1) != 0 {
            let packet_length = descriptor.length;
            let packet_address = descriptor.buffer_address;
            let packet_data_ptr = packet_address as *const u8;
            let packet_slice = unsafe { core::slice::from_raw_parts(packet_data_ptr, packet_length as usize) };
            
            let ethertype = ((packet_slice[12] as u16) << 8) | (packet_slice[13] as u16);
            
            match ethertype {
                0x0806 => {
                    let mut target_mac = [0u8; 6];
                    target_mac.copy_from_slice(&packet_slice[6..12]); 
                    self.current_rx_bucket = (self.current_rx_bucket + 1) % 32;
                    self.ping(target_mac);
                    return true;
                },
                _ => {}
            }
            self.current_rx_bucket = (self.current_rx_bucket + 1) % 32;
            return true; 
        }
        false 
    }
}
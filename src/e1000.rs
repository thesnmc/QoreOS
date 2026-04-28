use core::ptr::{write_volatile, read_volatile};
use alloc::vec::Vec;
use alloc::vec;

#[derive(Copy, Clone)]
pub struct ArpEntry {
    pub ip: [u8; 4],
    pub mac: [u8; 6],
    pub active: bool,
}

pub struct ArpCache {
    entries: [ArpEntry; 16],
}

impl ArpCache {
    pub const fn new() -> Self {
        ArpCache {
            entries: [ArpEntry { ip: [0; 4], mac: [0; 6], active: false }; 16],
        }
    }

    pub fn insert(&mut self, ip: [u8; 4], mac: [u8; 6]) {
        for entry in self.entries.iter_mut() {
            if entry.active && entry.ip == ip {
                entry.mac = mac;
                return;
            }
        }
        for entry in self.entries.iter_mut() {
            if !entry.active {
                entry.ip = ip;
                entry.mac = mac;
                entry.active = true;
                return;
            }
        }
    }

    pub fn lookup(&self, ip: &[u8; 4]) -> Option<[u8; 6]> {
        for entry in self.entries.iter() {
            if entry.active && entry.ip == *ip {
                return Some(entry.mac);
            }
        }
        None
    }
}

pub static mut ARP_TABLE: ArpCache = ArpCache::new();

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
    pub tx_ring_ptr: u64,           // <--- UPGRADED: Now public for net.rs
    pub current_tx_bucket: usize,   // <--- UPGRADED: Now public for net.rs
    pub mac_address: [u8; 6],
}

impl E1000 {
    pub fn new(bar0: u64) -> Self {
        let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]; 
        E1000 { 
            bar0_address: bar0, rx_ring_ptr: 0, current_rx_bucket: 0, tx_ring_ptr: 0, current_tx_bucket: 0, mac_address: mac,
        }
    }

    unsafe fn read_register(&self, offset: u64) -> u32 {
        read_volatile((self.bar0_address + offset) as *const u32)
    }

    // <--- UPGRADED: Now public for net.rs
    pub unsafe fn write_register(&self, offset: u64, value: u32) {
        let ptr = (self.bar0_address + offset) as *mut u32;
        write_volatile(ptr, value);
    }

    pub fn init_rx_ring(&mut self) {
        unsafe {
            let mut ctrl = self.read_register(0x0000);
            ctrl |= 1 << 6; // Flip Bit 6: SLU (Set Link Up)
            self.write_register(0x0000, ctrl);
        }

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
            
            let mac = self.mac_address;
            let ral = (mac[0] as u32) | ((mac[1] as u32) << 8) | ((mac[2] as u32) << 16) | ((mac[3] as u32) << 24);
            let rah = (mac[4] as u32) | ((mac[5] as u32) << 8) | (1 << 31);
            self.write_register(0x5400, ral); 
            self.write_register(0x5404, rah); 

            // Enable Unicast + Promiscuous Mode
            self.write_register(0x0100, (1 << 1) | (1 << 3) | (1 << 4) | (1 << 15));
        }
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
            
            let tap_msg = alloc::format!("> NET [RAW]: HARDWARE TRIGGERED! LEN: {} ETHERTYPE: 0x{:04X}\n", packet_length, ethertype);
            unsafe { crate::compositor::terminal_print(&tap_msg, 0x8B5CF6); } 

            match ethertype {
                0x0806 => {
                    let hardware_type = ((packet_slice[14] as u16) << 8) | (packet_slice[15] as u16);
                    let protocol_type = ((packet_slice[16] as u16) << 8) | (packet_slice[17] as u16);
                    let opcode = ((packet_slice[20] as u16) << 8) | (packet_slice[21] as u16);

                    if hardware_type == 1 && protocol_type == 0x0800 {
                        let mut sender_mac = [0u8; 6];
                        sender_mac.copy_from_slice(&packet_slice[22..28]);
                        
                        let mut sender_ip = [0u8; 4];
                        sender_ip.copy_from_slice(&packet_slice[28..32]);
                        
                        let mut target_ip = [0u8; 4];
                        target_ip.copy_from_slice(&packet_slice[38..42]);

                        unsafe { 
                            ARP_TABLE.insert(sender_ip, sender_mac); 
                        }

                        if opcode == 1 {
                            let sniffer_msg = alloc::format!("> NET: ROUTER ASKING FOR IP {}.{}.{}.{}\n", target_ip[0], target_ip[1], target_ip[2], target_ip[3]);
                            unsafe { crate::compositor::terminal_print(&sniffer_msg, 0xF59E0B); }

                            if target_ip == [10, 0, 2, 15] {
                                unsafe { crate::compositor::terminal_print("> NET: THAT IS US! TRANSMITTING ARP REPLY...\n", 0x10B981); }
                                
                                let mut reply: Vec<u8> = vec![
                                    sender_mac[0], sender_mac[1], sender_mac[2], sender_mac[3], sender_mac[4], sender_mac[5],
                                    self.mac_address[0], self.mac_address[1], self.mac_address[2], self.mac_address[3], self.mac_address[4], self.mac_address[5],
                                    0x08, 0x06,
                                    0x00, 0x01, 
                                    0x08, 0x00, 
                                    0x06, 0x04, 
                                    0x00, 0x02, 
                                    self.mac_address[0], self.mac_address[1], self.mac_address[2], self.mac_address[3], self.mac_address[4], self.mac_address[5],
                                    10, 0, 2, 15,
                                    sender_mac[0], sender_mac[1], sender_mac[2], sender_mac[3], sender_mac[4], sender_mac[5],
                                    sender_ip[0], sender_ip[1], sender_ip[2], sender_ip[3],
                                ];
                                
                                reply.resize(60, 0); 
                                
                                let frame_ptr = alloc::boxed::Box::leak(reply.into_boxed_slice()).as_ptr() as u64;
                                let descriptor_addr = self.tx_ring_ptr + (self.current_tx_bucket as u64 * 16);
                                let descriptor_ptr = descriptor_addr as *mut TxDescriptor;
                                unsafe {
                                    (*descriptor_ptr).buffer_address = frame_ptr;
                                    (*descriptor_ptr).length = 60;
                                    (*descriptor_ptr).cmd = (1 << 0) | (1 << 1) | (1 << 3);
                                }
                                self.current_tx_bucket = (self.current_tx_bucket + 1) % 8;
                                unsafe { self.write_register(0x3818, self.current_tx_bucket as u32); }
                            }
                        }
                    }
                },
                0x0800 => {
                    crate::net::handle_incoming_packet(packet_slice);
                }
                _ => {}
            }
            
            unsafe {
                let desc_mut = descriptor_addr as *mut RxDescriptor;
                (*desc_mut).status = 0; 
            }
            
            // --- THE RING BUFFER COLLISION FIX ---
            let old_bucket = self.current_rx_bucket; 
            self.current_rx_bucket = (self.current_rx_bucket + 1) % 32; 
            unsafe { self.write_register(0x2818, old_bucket as u32); }
            
            return true; 
        }
        false 
    }

    pub fn udp_broadcast(&mut self) {
        // --- RESTORED ORIGINAL FEATURE: PROPER UDP BROADCAST ---
        let frame: Vec<u8> = vec![ 
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 
            0x52, 0x54, 0x00, 0x12, 0x34, 0x56, 
            0x08, 0x00,                         
            0x45, 0x00, 0x00, 0x3A, 
            0x00, 0x00, 0x40, 0x00, 
            0x40, 0x11, 0x00, 0x00, 
            10, 0, 2, 15,           
            255, 255, 255, 255,     
            0x1A, 0x0A, 
            0x1A, 0x0B, 
            0x00, 0x26, 
            0x00, 0x00, 
            b'Q',b'O',b'R',b'E',b'O',b'S',b' ',
            b'N',b'O',b'D',b'E',b' ',
            b'O',b'N',b'L',b'I',b'N',b'E'
        ];

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
        
        unsafe { crate::compositor::terminal_print("\nSYS NET TX: UDP BROADCAST FIRED!\n", 0xEF4444); }
    }

    // --- NEW DEDICATED FIREWALL HANDSHAKE ---
    pub fn qemu_router_handshake(&mut self) {
        unsafe { crate::compositor::terminal_print("\n> NET: INITIATING QEMU ROUTER HANDSHAKE (10.0.2.2)...\n", 0xF59E0B); }
        self.arp_request([10, 0, 2, 2]); 
    }

    pub fn arp_request(&mut self, target_ip: [u8; 4]) {
        let mut frame: Vec<u8> = vec![
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 
            self.mac_address[0], self.mac_address[1], self.mac_address[2], 
            self.mac_address[3], self.mac_address[4], self.mac_address[5], 
            0x08, 0x06, 
            0x00, 0x01, 
            0x08, 0x00, 
            0x06,       
            0x04,       
            0x00, 0x01, 
            self.mac_address[0], self.mac_address[1], self.mac_address[2], 
            self.mac_address[3], self.mac_address[4], self.mac_address[5], 
            10, 0, 2, 15, 
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            target_ip[0], target_ip[1], target_ip[2], target_ip[3],
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
        
        unsafe { crate::compositor::terminal_print("\nNET: ARP Request Broadcasted to Network.\n", 0x3B82F6); }
    }
}
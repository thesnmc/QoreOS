use core::mem::size_of;
use alloc::vec::Vec;
use alloc::vec;

#[repr(C, packed)]
pub struct EthernetHeader {
    pub dest_mac: [u8; 6],
    pub src_mac: [u8; 6],
    pub ethertype: u16, // Network byte order (Big Endian)
}

#[repr(C, packed)]
pub struct Ipv4Header {
    pub version_ihl: u8,
    pub dscp_ecn: u8,
    pub total_length: u16,
    pub identification: u16,
    pub flags_fragment_offset: u16,
    pub ttl: u8,
    pub protocol: u8, // We are looking for 0x06 (TCP)
    pub header_checksum: u16,
    pub src_ip: [u8; 4],
    pub dest_ip: [u8; 4],
}

#[repr(C, packed)]
pub struct TcpHeader {
    pub src_port: u16,
    pub dest_port: u16,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset_flags: u16, 
    pub window_size: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
}

// The EdgeCore Protocol Analyzer
pub fn handle_incoming_packet(packet: &[u8]) {
    if packet.len() < size_of::<EthernetHeader>() { return; }

    // 1. Peel the Ethernet Layer
    let eth = unsafe { &*(packet.as_ptr() as *const EthernetHeader) };
    let ethertype = u16::from_be(eth.ethertype);

    // 0x0800 signifies an IPv4 Packet
    if ethertype == 0x0800 {
        let ip_offset = size_of::<EthernetHeader>();
        if packet.len() < ip_offset + size_of::<Ipv4Header>() { return; }

        // 2. Peel the IPv4 Layer
        let ip = unsafe { &*(packet[ip_offset..].as_ptr() as *const Ipv4Header) };
        
        // Check if it's meant for our Unikernel IP (10.0.2.15)
        if ip.dest_ip != [10, 0, 2, 15] { return; }

        // 0x06 signifies a TCP Segment
        if ip.protocol == 0x06 {
            let ihl = (ip.version_ihl & 0x0F) * 4;
            let tcp_offset = ip_offset + ihl as usize;
            
            if packet.len() < tcp_offset + size_of::<TcpHeader>() { return; }
            
            // 3. Peel the TCP Layer
            let tcp = unsafe { &*(packet[tcp_offset..].as_ptr() as *const TcpHeader) };
            
            let src_port = u16::from_be(tcp.src_port);
            let dest_port = u16::from_be(tcp.dest_port);
            let seq_num = u32::from_be(tcp.seq_num); // Extract Host's Sequence Number
            let flags = u16::from_be(tcp.data_offset_flags) & 0x01FF;

            let is_syn = (flags & 0x02) != 0;
            let _is_ack = (flags & 0x10) != 0; // Prefixed with _ to silence warnings

            unsafe {
                crate::compositor::terminal_print(
                    &alloc::format!("> NET: TCP SEGMENT RCV'D. PORT {} -> {}\n", src_port, dest_port), 
                    0x3B82F6
                );
                
                if is_syn {
                    crate::compositor::terminal_print("> NET: TCP [SYN] DETECTED! INITIATING 3-WAY HANDSHAKE...\n", 0xF59E0B);
                    
                    // --- THE FIX: Listen on Port 80, because QEMU translates 8080 to 80! ---
                    if dest_port == 80 {
                        // --- FIRE THE SYN-ACK ---
                        // 1. Look up the Router's MAC address from our ARP Cache
                        let mut target_mac = [0u8; 6];
                        if let Some(mac) = crate::e1000::ARP_TABLE.lookup(&ip.src_ip) {
                            target_mac = mac;
                        } else {
                            target_mac = [0xFF; 6]; // Fallback
                        }

                        // 2. Transmit the payload
                        send_tcp_syn_ack(target_mac, ip.src_ip, src_port, seq_num);
                    }
                }
            }
        }
    }
}

// --- THE QOREOS TRANSMISSION ENGINE ---
pub fn send_tcp_syn_ack(target_mac: [u8; 6], target_ip: [u8; 4], target_port: u16, client_seq: u32) {
    let mut frame: Vec<u8> = vec![0; 58]; // Minimum Ethernet frame size

    let my_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    let my_ip = [10, 0, 2, 15];
    let my_port: u16 = 80; // --- THE FIX: Respond from Port 80 so the checksums match! ---
    
    let my_seq: u32 = 0xCAFEBABE; // Our custom Initial Sequence Number
    let ack_num = client_seq.wrapping_add(1); // Acknowledge the host's SYN

    // 1. ETHERNET HEADER (14 Bytes)
    frame[0..6].copy_from_slice(&target_mac);
    frame[6..12].copy_from_slice(&my_mac);
    frame[12] = 0x08; frame[13] = 0x00; // IPv4

    // 2. IPv4 HEADER (20 Bytes)
    frame[14] = 0x45; // Version 4, IHL 5
    frame[15] = 0x00; // DSCP
    
    let total_len = 20 + 24; // IPv4 + TCP (with MSS option)
    frame[16] = (total_len >> 8) as u8;
    frame[17] = total_len as u8;
    
    frame[18] = 0x00; frame[19] = 0x00; // ID
    frame[20] = 0x40; frame[21] = 0x00; // Flags/Frag (Don't Fragment)
    frame[22] = 64;   // TTL
    frame[23] = 6;    // Protocol (TCP)
    
    frame[26..30].copy_from_slice(&my_ip);
    frame[30..34].copy_from_slice(&target_ip);

    // Compute IPv4 Checksum
    let mut ip_chk: u32 = 0;
    for i in (14..34).step_by(2) {
        if i == 24 { continue; } // Skip checksum field itself
        ip_chk += ((frame[i] as u32) << 8) | (frame[i+1] as u32);
    }
    while (ip_chk >> 16) > 0 { ip_chk = (ip_chk & 0xFFFF) + (ip_chk >> 16); }
    let final_ip_chk = (!ip_chk) as u16;
    frame[24] = (final_ip_chk >> 8) as u8;
    frame[25] = final_ip_chk as u8;

    // 3. TCP HEADER (24 Bytes)
    let tcp_start = 34;
    frame[tcp_start] = (my_port >> 8) as u8;
    frame[tcp_start + 1] = my_port as u8;
    
    frame[tcp_start + 2] = (target_port >> 8) as u8;
    frame[tcp_start + 3] = target_port as u8;

    frame[tcp_start + 4..tcp_start + 8].copy_from_slice(&my_seq.to_be_bytes());
    frame[tcp_start + 8..tcp_start + 12].copy_from_slice(&ack_num.to_be_bytes());

    frame[tcp_start + 12] = 0x60; // Data offset: 6 words (24 bytes)
    frame[tcp_start + 13] = 0x12; // Flags: SYN (0x02) + ACK (0x10)
    
    frame[tcp_start + 14] = 0xFA; frame[tcp_start + 15] = 0xF0; // Window size (64240)
    
    // TCP Option: Maximum Segment Size (MSS) = 1460
    frame[tcp_start + 20] = 0x02; // Option Kind: MSS
    frame[tcp_start + 21] = 0x04; // Option Length: 4
    frame[tcp_start + 22] = 0x05; frame[tcp_start + 23] = 0xB4; // MSS Value

    // Compute TCP Pseudo-Header Checksum
    let mut tcp_chk: u32 = 0;
    for i in (26..34).step_by(2) { tcp_chk += ((frame[i] as u32) << 8) | (frame[i+1] as u32); } // IPs
    tcp_chk += 6;  // Protocol
    tcp_chk += 24; // TCP Length

    for i in (tcp_start..tcp_start + 24).step_by(2) {
        if i == tcp_start + 16 { continue; } // Skip checksum field
        tcp_chk += ((frame[i] as u32) << 8) | (frame[i+1] as u32);
    }

    while (tcp_chk >> 16) > 0 { tcp_chk = (tcp_chk & 0xFFFF) + (tcp_chk >> 16); }
    let final_tcp_chk = (!tcp_chk) as u16;
    
    frame[tcp_start + 16] = (final_tcp_chk >> 8) as u8;
    frame[tcp_start + 17] = final_tcp_chk as u8;

    // --- INJECT INTO HARDWARE TX RING ---
    unsafe {
        if let Some(ref mut nic) = crate::NET_CARD {
            let frame_ptr = alloc::boxed::Box::leak(frame.into_boxed_slice()).as_ptr() as u64;
            let descriptor_addr = nic.tx_ring_ptr + (nic.current_tx_bucket as u64 * 16);
            let descriptor_ptr = descriptor_addr as *mut crate::e1000::TxDescriptor;
            
            (*descriptor_ptr).buffer_address = frame_ptr;
            (*descriptor_ptr).length = 58;
            (*descriptor_ptr).cmd = (1 << 0) | (1 << 1) | (1 << 3); // EOP | IFCS | RS
            
            nic.current_tx_bucket = (nic.current_tx_bucket + 1) % 8;
            nic.write_register(0x3818, nic.current_tx_bucket as u32);
        }
        crate::compositor::terminal_print("> NET: [SYN-ACK] TRANSMITTED TO HOST.\n", 0x10B981);
    }
}
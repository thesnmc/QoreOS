use core::mem::size_of;
use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;

// --- GLOBAL PAYLOAD LINK ---
pub static mut HTTP_PAYLOAD: Option<String> = None;

#[repr(C, packed)]
pub struct EthernetHeader {
    pub dest_mac: [u8; 6],
    pub src_mac: [u8; 6],
    pub ethertype: u16, 
}

#[repr(C, packed)]
pub struct Ipv4Header {
    pub version_ihl: u8,
    pub dscp_ecn: u8,
    pub total_length: u16,
    pub identification: u16,
    pub flags_fragment_offset: u16,
    pub ttl: u8,
    pub protocol: u8, 
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

pub fn handle_incoming_packet(packet: &[u8]) {
    if packet.len() < size_of::<EthernetHeader>() { return; }

    let eth = unsafe { &*(packet.as_ptr() as *const EthernetHeader) };
    let ethertype = u16::from_be(eth.ethertype);

    if ethertype == 0x0800 {
        let ip_offset = size_of::<EthernetHeader>();
        if packet.len() < ip_offset + size_of::<Ipv4Header>() { return; }

        let ip = unsafe { &*(packet[ip_offset..].as_ptr() as *const Ipv4Header) };
        if ip.dest_ip != [10, 0, 2, 15] { return; }

        if ip.protocol == 0x06 {
            let ihl = (ip.version_ihl & 0x0F) * 4;
            let tcp_offset = ip_offset + ihl as usize;
            
            if packet.len() < tcp_offset + size_of::<TcpHeader>() { return; }
            
            let tcp = unsafe { &*(packet[tcp_offset..].as_ptr() as *const TcpHeader) };
            
            let src_port = u16::from_be(tcp.src_port);
            let dest_port = u16::from_be(tcp.dest_port);
            let seq_num = u32::from_be(tcp.seq_num); 
            let ack_num = u32::from_be(tcp.ack_num); 
            
            let doff_flags = u16::from_be(tcp.data_offset_flags);
            let tcp_header_len = ((doff_flags >> 12) & 0x0F) * 4;
            let flags = doff_flags & 0x01FF;

            let is_syn = (flags & 0x02) != 0;
            let is_psh = (flags & 0x08) != 0; 

            if dest_port == 80 {
                if is_syn {
                    unsafe {
                        crate::compositor::terminal_print(
                            &alloc::format!("> NET: TCP [SYN] RCV'D. PORT {} -> {}\n", src_port, dest_port), 
                            0xF59E0B
                        );
                    }
                    
                    let mut target_mac = [0u8; 6];
                    if let Some(mac) = unsafe { crate::e1000::ARP_TABLE.lookup(&ip.src_ip) } { target_mac = mac; } else { target_mac = [0xFF; 6]; }
                    send_tcp_syn_ack(target_mac, ip.src_ip, src_port, seq_num);
                } 
                else if is_psh {
                    let data_offset = tcp_offset + tcp_header_len as usize;
                    let total_ip_len = u16::from_be(ip.total_length) as usize;
                    
                    if total_ip_len >= (ihl as usize + tcp_header_len as usize) {
                        let data_len = total_ip_len - ihl as usize - tcp_header_len as usize;
                        
                        if data_len > 0 && packet.len() >= data_offset + data_len {
                            let payload = &packet[data_offset..data_offset + data_len];
                            
                            if payload.starts_with(b"GET") {
                                unsafe { crate::compositor::terminal_print("> NET: HTTP [GET] REQUEST INTERCEPTED!\n", 0x10B981); }
                                
                                let mut target_mac = [0u8; 6];
                                if let Some(mac) = unsafe { crate::e1000::ARP_TABLE.lookup(&ip.src_ip) } { target_mac = mac; } else { target_mac = [0xFF; 6]; }
                                
                                send_http_response(target_mac, ip.src_ip, src_port, ack_num, seq_num, data_len as u32);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn send_tcp_syn_ack(target_mac: [u8; 6], target_ip: [u8; 4], target_port: u16, client_seq: u32) {
    let mut frame = craft_base_tcp_frame(target_mac, target_ip, target_port, 0xCAFEBABE, client_seq.wrapping_add(1), 0x12, 24);
    recalculate_checksums(&mut frame, 0);
    inject_frame(&mut frame);
    unsafe { crate::compositor::terminal_print("> NET: [SYN-ACK] TRANSMITTED TO HOST.\n", 0x3B82F6); }
}

pub fn send_http_response(target_mac: [u8; 6], target_ip: [u8; 4], target_port: u16, my_seq: u32, client_seq: u32, client_data_len: u32) {
    let file_content = unsafe { HTTP_PAYLOAD.as_ref().map(|s| s.as_str()).unwrap_or("ERROR: NO PAYLOAD FOUND IN RING-0 MEMORY") };
    
    let body = alloc::format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        file_content.len(),
        file_content
    );

    let ack_num = client_seq.wrapping_add(client_data_len);
    let mut frame = craft_base_tcp_frame(target_mac, target_ip, target_port, my_seq, ack_num, 0x18, 20); 
    
    frame.extend_from_slice(body.as_bytes());
    
    // We override total_len here since the body was added dynamically
    let total_len = (20 + 20 + body.len()) as u16;
    frame[16] = (total_len >> 8) as u8;
    frame[17] = total_len as u8;
    
    recalculate_checksums(&mut frame, body.len());

    inject_frame(&mut frame);
    unsafe { crate::compositor::terminal_print("> NET: HTTP 200 OK RESPONSE SERVED. SOCKET CLOSED.\n", 0x10B981); }
}

fn craft_base_tcp_frame(target_mac: [u8; 6], target_ip: [u8; 4], target_port: u16, seq: u32, ack: u32, flags: u8, tcp_hl: u8) -> Vec<u8> {
    let mut frame: Vec<u8> = vec![0; 34 + tcp_hl as usize];
    let my_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    let my_ip = [10, 0, 2, 15];

    frame[0..6].copy_from_slice(&target_mac);
    frame[6..12].copy_from_slice(&my_mac);
    frame[12] = 0x08; frame[13] = 0x00; 
    frame[14] = 0x45; 
    
    // --- THE CRITICAL FIX: Tell the Linux host how big the packet actually is! ---
    let total_ip_len = 20u16 + tcp_hl as u16;
    frame[16] = (total_ip_len >> 8) as u8;
    frame[17] = total_ip_len as u8;
    
    frame[20] = 0x40; frame[21] = 0x00; // Don't fragment flag
    frame[22] = 64; frame[23] = 6; 
    frame[26..30].copy_from_slice(&my_ip);
    frame[30..34].copy_from_slice(&target_ip);

    let ts = 34;
    frame[ts..ts+2].copy_from_slice(&80u16.to_be_bytes()); 
    frame[ts+2..ts+4].copy_from_slice(&target_port.to_be_bytes());
    frame[ts+4..ts+8].copy_from_slice(&seq.to_be_bytes());
    frame[ts+8..ts+12].copy_from_slice(&ack.to_be_bytes());
    frame[ts+12] = (tcp_hl / 4) << 4;
    frame[ts+13] = flags;
    frame[ts+14..ts+16].copy_from_slice(&0xFAF0u16.to_be_bytes()); 

    if tcp_hl == 24 { 
        frame[ts+20] = 0x02; frame[ts+21] = 0x04;
        frame[ts+22..ts+24].copy_from_slice(&1460u16.to_be_bytes());
    }
    frame
}

fn recalculate_checksums(frame: &mut Vec<u8>, body_len: usize) {
    frame[24] = 0; frame[25] = 0;
    let mut ip_chk: u32 = 0;
    for i in (14..34).step_by(2) { ip_chk += ((frame[i] as u32) << 8) | (frame[i+1] as u32); }
    while (ip_chk >> 16) > 0 { ip_chk = (ip_chk & 0xFFFF) + (ip_chk >> 16); }
    let final_ip = !(ip_chk as u16);
    frame[24] = (final_ip >> 8) as u8; frame[25] = final_ip as u8;

    let tcp_start = 34;
    frame[tcp_start + 16] = 0; frame[tcp_start + 17] = 0; 
    let mut tcp_chk: u32 = 0;
    for i in (26..34).step_by(2) { tcp_chk += ((frame[i] as u32) << 8) | (frame[i+1] as u32); } 
    tcp_chk += 6; 
    tcp_chk += (frame.len() - 34) as u32; 

    for i in (tcp_start..frame.len()).step_by(2) {
        if i + 1 < frame.len() {
            tcp_chk += ((frame[i] as u32) << 8) | (frame[i+1] as u32);
        } else {
            tcp_chk += (frame[i] as u32) << 8; 
        }
    }
    while (tcp_chk >> 16) > 0 { tcp_chk = (tcp_chk & 0xFFFF) + (tcp_chk >> 16); }
    let final_tcp = !(tcp_chk as u16);
    frame[tcp_start + 16] = (final_tcp >> 8) as u8; frame[tcp_start + 17] = final_tcp as u8;
}

fn inject_frame(frame: &mut Vec<u8>) {
    unsafe {
        if let Some(ref mut nic) = crate::NET_CARD {
            let frame_ptr = alloc::boxed::Box::leak(frame.clone().into_boxed_slice()).as_ptr() as u64;
            let descriptor_addr = nic.tx_ring_ptr + (nic.current_tx_bucket as u64 * 16);
            let descriptor_ptr = descriptor_addr as *mut crate::e1000::TxDescriptor;
            (*descriptor_ptr).buffer_address = frame_ptr;
            (*descriptor_ptr).length = frame.len() as u16;
            (*descriptor_ptr).cmd = (1 << 0) | (1 << 1) | (1 << 3);
            nic.current_tx_bucket = (nic.current_tx_bucket + 1) % 8;
            nic.write_register(0x3818, nic.current_tx_bucket as u32);
        }
    }
}
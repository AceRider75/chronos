use alloc::vec::Vec;
use alloc::format;
use alloc::string::String;

// --- HEADER DEFINITIONS ---
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct EthernetHeader {
    pub dest_mac: [u8; 6],
    pub src_mac: [u8; 6],
    pub ethertype: u16,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ArpPacket {
    pub hardware_type: u16,
    pub protocol_type: u16,
    pub hardware_len: u8,
    pub protocol_len: u8,
    pub opcode: u16,
    pub src_mac: [u8; 6],
    pub src_ip: [u8; 4],
    pub dest_mac: [u8; 6],
    pub dest_ip: [u8; 4],
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Ipv4Header {
    pub version_ihl: u8,
    pub type_of_service: u8,
    pub total_length: u16,
    pub identification: u16,
    pub flags_fragment: u16,
    pub ttl: u8,
    pub protocol: u8,
    pub checksum: u16,
    pub src_ip: [u8; 4],
    pub dest_ip: [u8; 4],
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UdpHeader {
    pub src_port: u16,
    pub dest_port: u16,
    pub length: u16,
    pub checksum: u16,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DhcpPacket {
    pub op: u8, pub htype: u8, pub hlen: u8, pub hops: u8,
    pub xid: u32, pub secs: u16, pub flags: u16,
    pub ciaddr: [u8; 4], pub yiaddr: [u8; 4], pub siaddr: [u8; 4], pub giaddr: [u8; 4],
    pub chaddr: [u8; 16], pub sname: [u8; 64], pub file: [u8; 128], pub magic: u32,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct IcmpHeader {
    pub packet_type: u8,
    pub code: u8,
    pub checksum: u16,
    pub id: u16,
    pub seq: u16,
}

fn ntohs(n: u16) -> u16 { ((n & 0xFF) << 8) | ((n & 0xFF00) >> 8) }

// --- HANDLERS ---

// UPDATED RETURN TYPE: Option<(TargetMAC, TargetIP)>
pub fn handle_packet(data: &[u8]) -> Option<([u8; 6], [u8; 4])> {
    if data.len() < 14 { return None; }

    let eth_header = unsafe { &*(data.as_ptr() as *const EthernetHeader) };
    let ethertype = ntohs(eth_header.ethertype);

    match ethertype {
        0x0806 => handle_arp(data),
        0x0800 => {
            handle_ipv4(data);
            None
        },
        _ => {
            // UNCOMMENTED DEBUG PRINT:
            crate::writer::print(&format!("[NET] Unknown Packet Type: {:04x}\n", ethertype));
            None
        }
    }
}

fn handle_arp(data: &[u8]) -> Option<([u8; 6], [u8; 4])> {
    if data.len() < 14 + 28 { return None; }
    
    let arp_ptr = unsafe { data.as_ptr().add(14) as *const ArpPacket };
    let arp = unsafe { &*arp_ptr };

    let opcode = ntohs(arp.opcode);
    
    if opcode == 1 {
        // ARP Request for US (10.0.2.15)
        if arp.dest_ip == [10, 0, 2, 15] {
            crate::writer::print("[NET] ARP Request for ME! Sending Reply...\n");
            // Return Sender's MAC AND Sender's IP so we reply to the right place
            return Some((arp.src_mac, arp.src_ip));
        }
    } else if opcode == 2 {
        crate::writer::print("[NET] ARP Reply received.\n");
    }
    None
}

fn handle_ipv4(data: &[u8]) {
    let ip_header_ptr = unsafe { data.as_ptr().add(14) };
    let ip_header = unsafe { &*(ip_header_ptr as *const Ipv4Header) };
    
    if ip_header.protocol == 17 {
        handle_udp(data, ip_header_ptr);
    } else if ip_header.protocol == 1 {
        handle_icmp(ip_header_ptr);
    }
}

fn handle_udp(data: &[u8], ip_header_ptr: *const u8) {
    let udp_header_ptr = unsafe { ip_header_ptr.add(20) };
    let udp_header = unsafe { &*(udp_header_ptr as *const UdpHeader) };
    let dest_port = ntohs(udp_header.dest_port);
    if dest_port == 68 {
        handle_dhcp(udp_header_ptr);
    }
}

fn handle_dhcp(udp_header_ptr: *const u8) {
    let dhcp_ptr = unsafe { udp_header_ptr.add(8) };
    let dhcp = unsafe { &*(dhcp_ptr as *const DhcpPacket) };
    let ip = dhcp.yiaddr;
    
    // SAVE THE IP TO GLOBAL STATE
    crate::state::set_my_ip(ip);
    
    crate::writer::print(&format!(
        "   >>> IP ASSIGNED AND SAVED: {}.{}.{}.{} <<<\n",
        ip[0], ip[1], ip[2], ip[3]
    ));
}

fn handle_icmp(ip_header_ptr: *const u8) {
    let icmp_ptr = unsafe { ip_header_ptr.add(20) };
    let icmp = unsafe { &*(icmp_ptr as *const IcmpHeader) };
    if icmp.packet_type == 0 { 
        let seq = ntohs(icmp.seq);
        crate::writer::print(&format!("[NET] PING REPLY! Seq={}\n", seq));
    }
}
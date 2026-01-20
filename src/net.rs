use alloc::vec::Vec;
use alloc::format;
use alloc::string::String;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct EthernetHeader {
    pub dest_mac: [u8; 6],
    pub src_mac: [u8; 6],
    pub ethertype: u16,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Ipv4Header {
    pub version_ihl: u8,      // Version (4 bits) + Header Length (4 bits)
    pub type_of_service: u8,
    pub total_length: u16,
    pub identification: u16,
    pub flags_fragment: u16,
    pub ttl: u8,
    pub protocol: u8,         // 17 = UDP
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
    pub op: u8,               // 1 = Boot Request, 2 = Boot Reply
    pub htype: u8,            // Hardware type (1 = Ethernet)
    pub hlen: u8,             // Hardware address length (6)
    pub hops: u8,
    pub xid: u32,             // Transaction ID
    pub secs: u16,
    pub flags: u16,
    pub ciaddr: [u8; 4],      // Client IP
    pub yiaddr: [u8; 4],      // Your IP (The one offered to you)
    pub siaddr: [u8; 4],      // Server IP
    pub giaddr: [u8; 4],      // Gateway IP
    pub chaddr: [u8; 16],     // Client Hardware Address (MAC)
    pub sname: [u8; 64],
    pub file: [u8; 128],
    pub magic: u32,           // DHCP Magic Cookie (0x63825363)
    // Options follow this...
}

fn ntohs(n: u16) -> u16 { ((n & 0xFF) << 8) | ((n & 0xFF00) >> 8) }
fn ntohl(n: u32) -> u32 { 
    ((n & 0xFF) << 24) | ((n & 0xFF00) << 8) | 
    ((n & 0xFF0000) >> 8) | ((n & 0xFF000000) >> 24) 
}

pub fn handle_packet(data: &[u8]) {
    if data.len() < 14 { return; }

    let eth_header = unsafe { &*(data.as_ptr() as *const EthernetHeader) };
    let ethertype = ntohs(eth_header.ethertype);

    match ethertype {
        0x0806 => handle_arp(data),
        0x0800 => handle_ipv4(data),
        _ => {}
    }
}

fn handle_arp(data: &[u8]) {
    // (Existing ARP logic, simplified for brevity or keep your old one)
    crate::writer::print("[NET] ARP Packet.\n");
}

fn handle_ipv4(data: &[u8]) {
    let ip_header_ptr = unsafe { data.as_ptr().add(14) };
    let ip_header = unsafe { &*(ip_header_ptr as *const Ipv4Header) };
    
    // Check Protocol (17 = UDP)
    if ip_header.protocol == 17 {
        handle_udp(data, ip_header_ptr);
    }
}

fn handle_udp(data: &[u8], ip_header_ptr: *const u8) {
    // IP Header length is variable, but usually 20 bytes (0x45)
    // We assume 20 bytes for simplicity here.
    let udp_header_ptr = unsafe { ip_header_ptr.add(20) };
    let udp_header = unsafe { &*(udp_header_ptr as *const UdpHeader) };
    
    let dest_port = ntohs(udp_header.dest_port);
    let src_port = ntohs(udp_header.src_port);

    // DHCP Server sends to Client on Port 68
    if dest_port == 68 && src_port == 67 {
        crate::writer::print("[NET] DHCP OFFER RECEIVED!\n");
        handle_dhcp(udp_header_ptr);
    }
}

fn handle_dhcp(udp_header_ptr: *const u8) {
    // DHCP packet starts after UDP header (8 bytes)
    let dhcp_ptr = unsafe { udp_header_ptr.add(8) };
    let dhcp = unsafe { &*(dhcp_ptr as *const DhcpPacket) };
    
    // The "yiaddr" field contains "Your IP Address"
    let ip = dhcp.yiaddr;
    
    crate::writer::print(&format!(
        "   >>> ASSIGNED IP: {}.{}.{}.{} <<<\n",
        ip[0], ip[1], ip[2], ip[3]
    ));
}
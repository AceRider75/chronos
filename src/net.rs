use alloc::vec::Vec;
use alloc::format;
use alloc::string::String;
use core::mem::transmute;

// Force the compiler to not add padding bytes between fields
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct EthernetHeader {
    pub dest_mac: [u8; 6],
    pub src_mac: [u8; 6],
    pub ethertype: u16, // Big Endian!
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ArpPacket {
    pub hardware_type: u16,
    pub protocol_type: u16,
    pub hardware_len: u8,
    pub protocol_len: u8,
    pub opcode: u16,       // 1 = Request, 2 = Reply
    pub src_mac: [u8; 6],
    pub src_ip: [u8; 4],
    pub dest_mac: [u8; 6],
    pub dest_ip: [u8; 4],
}

// Helper to convert Big Endian (Network) to Little Endian (x86)
fn ntohs(n: u16) -> u16 {
    ((n & 0xFF) << 8) | ((n & 0xFF00) >> 8)
}

pub fn handle_packet(data: &[u8]) {
    if data.len() < 14 { return; } // Too small

    // 1. Parse Ethernet Header
    // SAFETY: We cast raw bytes to a struct. Dangerous but standard for OS devs.
    let eth_header = unsafe { 
        &*(data.as_ptr() as *const EthernetHeader) 
    };

    let ethertype = ntohs(eth_header.ethertype);

    // 0x0806 = ARP, 0x0800 = IPv4
    if ethertype == 0x0806 {
        crate::writer::print("[NET STACK] ARP Packet Detected!\n");
        handle_arp(data);
    } else if ethertype == 0x0800 {
        crate::writer::print("[NET STACK] IPv4 Packet Detected (Ignoring for now)\n");
    } else {
        crate::writer::print(&format!("[NET STACK] Unknown Packet: {:04x}\n", ethertype));
    }
}

fn handle_arp(data: &[u8]) {
    // ARP packet starts AFTER the 14-byte Ethernet Header
    if data.len() < 14 + 28 { return; }
    
    let arp_ptr = unsafe { data.as_ptr().add(14) as *const ArpPacket };
    let arp = unsafe { &*arp_ptr };

    let opcode = ntohs(arp.opcode);
    
    if opcode == 2 {
        crate::writer::print("[NET STACK] ARP REPLY received!\n");
        let ip = arp.src_ip;
        let mac = arp.src_mac;
        
        crate::writer::print(&format!(
            "   Router IP: {}.{}.{}.{}\n   Router MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
            ip[0], ip[1], ip[2], ip[3],
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        ));
    }
}
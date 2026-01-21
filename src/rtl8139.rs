use crate::pci::{PciDevice, pci_read_u32};
use crate::{writer, state, net};
use x86_64::instructions::port::Port;
use alloc::format;
use core::sync::atomic::Ordering;

// --- REGISTERS ---
const REG_MAC: u16 = 0x00;      // MAC Address
const REG_TSD0: u16 = 0x10;     // Transmit Status Descriptor 0
const REG_TSAD0: u16 = 0x20;    // Transmit Start Address Descriptor 0
const REG_RBSTART: u16 = 0x30;  // Receive Buffer Start Address
const REG_CMD: u16 = 0x37;      // Command Register
const REG_CAPR: u16 = 0x38;     // Current Address of Packet Read
const REG_IMR: u16 = 0x3C;      // Interrupt Mask Register
const REG_ISR: u16 = 0x3E;      // Interrupt Status Register
const REG_TCR: u16 = 0x40;      // Transmit Configuration Register
const REG_RCR: u16 = 0x44;      // Receive Configuration Register

// --- MEMORY MAP ---
// We use fixed Physical Addresses in the 32MB range to avoid Kernel/Heap collisions.
const RX_BUFFER_PHYS: u32 = 0x0200_0000; 
const TX_BUFFER_PHYS: u32 = 0x0201_0000; 
const RX_BUF_SIZE: usize = 8192;

pub struct Rtl8139 {
    io_base: u16,
    pub mac_addr: [u8; 6],
    rx_buffer_ptr: *mut u8,
    tx_buffer_ptr: *mut u8,
    tx_cur: u8,
    rx_offset: usize,
}

impl Rtl8139 {
    pub fn new(device: PciDevice) -> Self {
        unsafe {
            // 1. Get I/O Port Base from PCI Configuration Space
            let bar0 = pci_read_u32(device.bus, device.device, device.function, 0x10);
            let io_base = (bar0 & !0x3) as u16;

            // 2. Read the hardware MAC Address
            let mut mac = [0u8; 6];
            for i in 0..6 { 
                mac[i] = Port::<u8>::new(io_base + i as u16).read(); 
            }

            // 3. Map Virtual Pointers using HHDM
            let hhdm = state::HHDM_OFFSET.load(Ordering::Relaxed);
            let rx_ptr = (hhdm + RX_BUFFER_PHYS as u64) as *mut u8;
            let tx_ptr = (hhdm + TX_BUFFER_PHYS as u64) as *mut u8;

            // 4. Zero out buffers to prevent processing old garbage data
            for i in 0..RX_BUF_SIZE { core::ptr::write_volatile(rx_ptr.add(i), 0); }
            for i in 0..2048 { core::ptr::write_volatile(tx_ptr.add(i), 0); }

            let mut driver = Rtl8139 {
                io_base,
                mac_addr: mac,
                rx_buffer_ptr: rx_ptr,
                tx_buffer_ptr: tx_ptr,
                tx_cur: 0,
                rx_offset: 0,
            };

            driver.init();
            driver
        }
    }

    unsafe fn init(&mut self) {
        let mut cmd_port = Port::<u8>::new(self.io_base + REG_CMD);
        
        // Power On
        cmd_port.write(0x00); 
        
        // Software Reset
        cmd_port.write(0x10); 
        while (cmd_port.read() & 0x10) != 0 { core::hint::spin_loop(); }

        // Configure Receive Buffer Address
        Port::<u32>::new(self.io_base + REG_RBSTART).write(RX_BUFFER_PHYS);

        // Enable All Interrupts for polling/debugging
        Port::<u16>::new(self.io_base + REG_IMR).write(0xFFFF); 

        // RCR: Accept Broadcast (AB), Multicast (AM), Physical Match (APM), and All (AAP)
        // Set bit 7 for Wrap (allow data to overflow slightly without crashing)
        Port::<u32>::new(self.io_base + REG_RCR).write(0xCF);

        // Enable Receiver (RE) and Transmitter (TE)
        cmd_port.write(0x0C); 
        
        writer::print("[NET] RTL8139 Driver Initialized (Ring Buffer Active).\n");
    }

    pub fn log_mac(&self) {
        let m = self.mac_addr;
        writer::print(&format!("[NET] Hardware MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
            m[0], m[1], m[2], m[3], m[4], m[5]));
    }

    pub fn get_hardware_status(&self) -> u16 {
        unsafe { Port::<u16>::new(self.io_base + REG_ISR).read() }
    }

    // --- DHCP PROTOCOL ---
    pub fn send_dhcp_discover(&mut self) {
        let mut pkt = [0u8; 300];
        let mut i = 0;

        // Ethernet Header
        for _ in 0..6 { pkt[i] = 0xFF; i += 1; }
        for j in 0..6 { pkt[i] = self.mac_addr[j]; i += 1; }
        pkt[i] = 0x08; pkt[i+1] = 0x00; i += 2; // Type IPv4

        // IP Header
        let ip_start = i;
        pkt[i] = 0x45; pkt[i+2] = 0x01; pkt[i+3] = 0x10; // Len 272
        pkt[i+8] = 0x40; pkt[i+9] = 17; // Protocol UDP
        for j in 0..4 { pkt[i+16+j] = 0xFF; } // Dest 255.255.255.255
        let csum = self.calc_ip_checksum(&pkt[ip_start..ip_start+20]);
        pkt[ip_start+10] = (csum >> 8) as u8; pkt[ip_start+11] = (csum & 0xFF) as u8;
        i += 20;

        // UDP Header
        pkt[i+1] = 68; pkt[i+3] = 67; // Ports 68 -> 67
        pkt[i+5] = 0xFC; // Len 252
        i += 8;

        // DHCP Data
        let dhcp_start = i;
        pkt[i] = 0x01; pkt[i+1] = 0x01; pkt[i+2] = 0x06; i += 4;
        pkt[i] = 0x39; pkt[i+1] = 0x03; pkt[i+2] = 0xF3; pkt[i+3] = 0x26; i += 4; // XID
        i = dhcp_start + 28;
        for j in 0..6 { pkt[i+j] = self.mac_addr[j]; i += 6; } // CHADDR
        i = dhcp_start + 236;
        pkt[i] = 0x63; pkt[i+1] = 0x82; pkt[i+2] = 0x53; pkt[i+3] = 0x63; i += 4; // Cookie
        pkt[i] = 53; pkt[i+1] = 1; pkt[i+2] = 1; i += 3; // Option 53: Discover
        pkt[i] = 255; // Option: End

        self.transmit(&pkt);
        writer::print("[NET] DHCP DISCOVER sent.\n");
    }

    // --- ICMP PING ---
    pub fn send_ping(&mut self, seq: u16) {
        let mut pkt = [0u8; 74];
        let mut i = 0;
        let dest_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]; // Standard QEMU Gateway MAC
        for j in 0..6 { pkt[i] = dest_mac[j]; i += 1; }
        for j in 0..6 { pkt[i] = self.mac_addr[j]; i += 1; }
        pkt[i] = 0x08; pkt[i+1] = 0x00; i += 2;

        let ip_start = i;
        pkt[i] = 0x45; pkt[i+3] = 60; pkt[i+8] = 0x80; pkt[i+9] = 1; // ICMP
        let my_ip = state::get_my_ip();
        let src = if my_ip == [0,0,0,0] { [10,0,2,15] } else { my_ip };
        for j in 0..4 { pkt[i+12+j] = src[j]; pkt[i+16+j] = [10, 0, 2, 2][j]; }
        let csum = self.calc_ip_checksum(&pkt[ip_start..ip_start+20]);
        pkt[ip_start+10] = (csum >> 8) as u8; pkt[ip_start+11] = (csum & 0xFF) as u8;
        i += 20;

        let icmp_start = i;
        pkt[i] = 8; // Type 8: Echo Request
        pkt[i+4] = 0x12; pkt[i+5] = 0x34; // ID
        pkt[i+6] = (seq >> 8) as u8; pkt[i+7] = (seq & 0xFF) as u8;
        let ic_csum = self.calc_ip_checksum(&pkt[icmp_start..icmp_start+40]);
        pkt[icmp_start+2] = (ic_csum >> 8) as u8; pkt[icmp_start+3] = (ic_csum & 0xFF) as u8;
        
        self.transmit(&pkt);
        writer::print(&format!("[NET] ICMP Echo (Seq {}) sent.\n", seq));
    }

    // --- ARP REPLY ---
    pub fn send_arp_reply(&mut self, t_mac: [u8; 6], t_ip: [u8; 4]) {
        let mut pkt = [0u8; 60];
        // Eth
        for i in 0..6 { pkt[i] = t_mac[i]; pkt[i+6] = self.mac_addr[i]; }
        pkt[12] = 0x08; pkt[13] = 0x06;
        // ARP
        pkt[14] = 0; pkt[15] = 1; pkt[16] = 8; pkt[17] = 0; pkt[18] = 6; pkt[19] = 4; pkt[21] = 2; // Reply
        for i in 0..6 { pkt[22+i] = self.mac_addr[i]; pkt[32+i] = t_mac[i]; }
        let my_ip = state::get_my_ip();
        let src = if my_ip == [0,0,0,0] { [10,0,2,15] } else { my_ip };
        for i in 0..4 { pkt[28+i] = src[i]; pkt[38+i] = t_ip[i]; }
        
        self.transmit(&pkt);
        writer::print("[NET] ARP Reply sent to Gateway.\n");
    }

    // --- RECEIVE ENGINE ---
    pub fn sniff_packet(&mut self) {
        unsafe {
            // Check the current offset for a valid header
            let header_addr = self.rx_buffer_ptr.add(self.rx_offset);
            let header = core::ptr::read_volatile(header_addr as *const u32);

            // Bit 0 = ROK (Receive OK). 0xFFFFFFFF = Hardware not ready/Reset.
            if (header & 0x01) != 0 && header != 0xFFFFFFFF {
                let len = (header >> 16) as usize;
                
                if len > 4 && len < 2000 {
                    // Create a slice skipping the 4-byte RTL header
                    let data = core::slice::from_raw_parts(header_addr.add(4), len - 4);
                    
                    // Send to Network Stack for parsing. 
                    // If it returns Some, it means it's an ARP request that needs a reply.
                    if let Some((m, i)) = net::handle_packet(data) { 
                        self.send_arp_reply(m, i); 
                    }
                }

                // Advance Ring Pointer (Aligned to 4 bytes)
                let step = (len + 4 + 3) & !3;
                self.rx_offset = (self.rx_offset + step) % RX_BUF_SIZE;

                // Update CAPR register to let hardware know we've read the data
                Port::<u16>::new(self.io_base + REG_CAPR).write((self.rx_offset as u16).wrapping_sub(0x10));
            }
        }
    }

    // --- LOW LEVEL HELPERS ---
    fn transmit(&mut self, data: &[u8]) {
        unsafe {
            // 1. Copy data to the TX Buffer
            for (i, &b) in data.iter().enumerate() {
                core::ptr::write_volatile(self.tx_buffer_ptr.add(i), b);
            }

            // 2. Pad to minimum Ethernet size (60 bytes before CRC)
            // If we don't do this, QEMU Slirp ret: -1 happens!
            if data.len() < 60 {
                for i in data.len()..60 {
                    core::ptr::write_volatile(self.tx_buffer_ptr.add(i), 0);
                }
            }

            let send_len = core::cmp::max(data.len(), 60);

            // 3. Set the Physical Address for this descriptor
            let tsad_port = self.io_base + REG_TSAD0 + (self.tx_cur as u16 * 4);
            Port::<u32>::new(tsad_port).write(TX_BUFFER_PHYS);

            // 4. Trigger the send by writing the length to the TSD register
            // We also set the 'Early Transmit Threshold' to 0 (start sending immediately)
            let tsd_port = self.io_base + REG_TSD0 + (self.tx_cur as u16 * 4);
            Port::<u32>::new(tsd_port).write(send_len as u32);

            // 5. Rotate descriptor
            self.tx_cur = (self.tx_cur + 1) % 4;
            
            // 6. Wait briefly to avoid overwhelming Slirp
            for _ in 0..1000 { core::hint::spin_loop(); }
        }
    }
    fn calc_ip_checksum(&self, data: &[u8]) -> u16 {
        let mut sum: u32 = 0;
        for i in (0..data.len()).step_by(2) {
            let word = if i + 1 < data.len() {
                ((data[i] as u32) << 8) | (data[i+1] as u32)
            } else {
                (data[i] as u32) << 8
            };
            sum = sum.wrapping_add(word);
        }
        while (sum >> 16) != 0 { sum = (sum & 0xFFFF) + (sum >> 16); }
        !sum as u16
    }
}
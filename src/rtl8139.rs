use crate::pci::{PciDevice, pci_read_u32};
use crate::{writer, state};
use x86_64::instructions::port::Port;
use alloc::format;
use core::sync::atomic::Ordering;
use crate::net;

// REGISTERS
const REG_MAC: u16 = 0x00;
const REG_TSD0: u16 = 0x10;
const REG_TSAD0: u16 = 0x20;
const REG_RBSTART: u16 = 0x30;
const REG_CMD: u16 = 0x37;
const REG_CAPR: u16 = 0x38;
const REG_IMR: u16 = 0x3C;
const REG_ISR: u16 = 0x3E;
const REG_TCR: u16 = 0x40;
const REG_RCR: u16 = 0x44;

// Use Lower Memory (Safe Zone) just in case High Mem is mapped weirdly
const RX_BUFFER_PHYS: u32 = 0x0060_0000; // 6MB
const TX_BUFFER_PHYS: u32 = 0x0061_0000; 

pub struct Rtl8139 {
    io_base: u16,
    mac_addr: [u8; 6],
    rx_buffer_ptr: *mut u8, // Changed to mut for easier clearing
    tx_buffer_ptr: *mut u8,
    tx_cur: u8,
}

impl Rtl8139 {
    fn calc_ip_checksum(&self, header: &[u8]) -> u16 {
        let mut sum: u32 = 0;
        // Sum all 16-bit words
        for i in (0..header.len()).step_by(2) {
            let word = ((header[i] as u32) << 8) | (header[i+1] as u32);
            sum = sum.wrapping_add(word);
        }
        // Add carry bits
        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        // Invert
        !sum as u16
    }

    pub fn send_dhcp_discover(&mut self) {
        unsafe {
            writer::print("[NET] Sending DHCP DISCOVER (With Checksum)...\n");

            let mut idx = 0;
            
            // --- 1. ETHERNET HEADER ---
            for _ in 0..6 { self.write_tx(idx, 0xFF); idx += 1; } // Dest
            for i in 0..6 { self.write_tx(idx, self.mac_addr[i]); idx += 1; } // Src
            self.write_tx(idx, 0x08); idx += 1; self.write_tx(idx, 0x00); idx += 1; // Type IP

            // --- 2. IPv4 HEADER (With Checksum Calc) ---
            // We build it in a temporary buffer first
            let mut ip_header: [u8; 20] = [0; 20];
            
            ip_header[0] = 0x45; // Ver/IHL
            ip_header[1] = 0x00; // TOS
            // Total Length (272 bytes = 0x0110)
            ip_header[2] = 0x01; ip_header[3] = 0x10; 
            // ID, Flags
            ip_header[4] = 0x00; ip_header[5] = 0x00;
            ip_header[6] = 0x00; ip_header[7] = 0x00;
            // TTL, Protocol (UDP)
            ip_header[8] = 0x40; ip_header[9] = 17;
            // Checksum (Initially 0)
            ip_header[10] = 0x00; ip_header[11] = 0x00;
            // Src IP (0.0.0.0)
            ip_header[12] = 0x00; ip_header[13] = 0x00; ip_header[14] = 0x00; ip_header[15] = 0x00;
            // Dest IP (255.255.255.255)
            ip_header[16] = 0xFF; ip_header[17] = 0xFF; ip_header[18] = 0xFF; ip_header[19] = 0xFF;

            // CALCULATE CHECKSUM
            let csum = self.calc_ip_checksum(&ip_header);
            ip_header[10] = (csum >> 8) as u8;
            ip_header[11] = (csum & 0xFF) as u8;

            // WRITE IP HEADER
            for b in ip_header.iter() { self.write_tx(idx, *b); idx += 1; }

            // --- 3. UDP HEADER ---
            self.write_tx(idx, 0x00); idx += 1; self.write_tx(idx, 68); idx += 1; // Src 68
            self.write_tx(idx, 0x00); idx += 1; self.write_tx(idx, 67); idx += 1; // Dest 67
            self.write_tx(idx, 0x00); idx += 1; self.write_tx(idx, 0xFC); idx += 1; // Len 252
            self.write_tx(idx, 0x00); idx += 1; self.write_tx(idx, 0x00); idx += 1; // Csum 0

            // --- 4. DHCP PAYLOAD ---
            self.write_tx(idx, 0x01); idx += 1; self.write_tx(idx, 0x01); idx += 1; // Req, Eth
            self.write_tx(idx, 0x06); idx += 1; self.write_tx(idx, 0x00); idx += 1; // Len, Hops
            // XID
            self.write_tx(idx, 0x12); idx += 1; self.write_tx(idx, 0x34); idx += 1;
            self.write_tx(idx, 0x56); idx += 1; self.write_tx(idx, 0x78); idx += 1;
            // Secs, Flags
            self.write_tx(idx, 0x00); idx += 1; self.write_tx(idx, 0x00); idx += 1;
            self.write_tx(idx, 0x00); idx += 1; self.write_tx(idx, 0x00); idx += 1;
            // IPs (CI, YI, SI, GI) -> All 0
            for _ in 0..16 { self.write_tx(idx, 0x00); idx += 1; }
            // MAC
            for i in 0..6 { self.write_tx(idx, self.mac_addr[i]); idx += 1; }
            for _ in 0..10 { self.write_tx(idx, 0x00); idx += 1; } // Pad MAC
            // Legacy SNAME/FILE
            for _ in 0..192 { self.write_tx(idx, 0x00); idx += 1; }
            // Cookie
            self.write_tx(idx, 0x63); idx += 1; self.write_tx(idx, 0x82); idx += 1;
            self.write_tx(idx, 0x53); idx += 1; self.write_tx(idx, 0x63); idx += 1;
            // Option 53 (Discover)
            self.write_tx(idx, 53); idx += 1; self.write_tx(idx, 1); idx += 1; self.write_tx(idx, 1); idx += 1;
            // Option 255 (End)
            self.write_tx(idx, 255); idx += 1;

            // Pad
            while idx < 60 { self.write_tx(idx, 0); idx += 1; }

            // SEND
            let tsd_port_off = REG_TSD0 + (self.tx_cur as u16 * 4);
            let tsad_port_off = REG_TSAD0 + (self.tx_cur as u16 * 4);
            let mut tsad = Port::<u32>::new(self.io_base + tsad_port_off);
            tsad.write(TX_BUFFER_PHYS);
            let mut tsd = Port::<u32>::new(self.io_base + tsd_port_off);
            tsd.write(idx as u32);
            self.tx_cur = (self.tx_cur + 1) % 4;
        }
    }
    pub fn new(device: PciDevice) -> Self {
        unsafe {
            let bar0 = pci_read_u32(device.bus, device.device, device.function, 0x10);
            let io_base = (bar0 & !0x3) as u16;

            let mut mac = [0u8; 6];
            for i in 0..6 {
                let mut port = Port::<u8>::new(io_base + i);
                mac[i as usize] = port.read();
            }

            let hhdm = state::HHDM_OFFSET.load(Ordering::Relaxed);
            let rx_virt = hhdm + (RX_BUFFER_PHYS as u64);
            let tx_virt = hhdm + (TX_BUFFER_PHYS as u64);

            let rx_ptr = rx_virt as *mut u8;
            let tx_ptr = tx_virt as *mut u8;
            
            // CRITICAL: Zero the buffer manually so we know if it changes!
            for i in 0..8192 { *rx_ptr.add(i) = 0; }
            for i in 0..2048 { *tx_ptr.add(i) = 0; }

            let mut driver = Rtl8139 {
                io_base,
                mac_addr: mac,
                rx_buffer_ptr: rx_ptr,
                tx_buffer_ptr: tx_ptr,
                tx_cur: 0,
            };

            driver.init();
            driver
        }
    }

    pub fn log_mac(&self) {
        let m = self.mac_addr;
        writer::print(&format!("[NET] MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
            m[0], m[1], m[2], m[3], m[4], m[5]));
    }

    unsafe fn init(&mut self) {
        let mut cmd_port = Port::<u8>::new(self.io_base + REG_CMD);
        cmd_port.write(0x00); // Power On
        
        cmd_port.write(0x10); // Reset
        while (cmd_port.read() & 0x10) != 0 { core::hint::spin_loop(); }

        // Setup Rx Buffer
        let mut rbstart_port = Port::<u32>::new(self.io_base + REG_RBSTART);
        rbstart_port.write(RX_BUFFER_PHYS);

        // Setup Interrupts
        let mut imr_port = Port::<u16>::new(self.io_base + REG_IMR);
        imr_port.write(0xFFFF); 

        // RCR Configuration:
        // Accept Broadcast (AB), Multicast (AM), Physical (APM), All (AAP)
        // Wrap (1<<7)
        // 0xCF = 11001111 
        let mut rcr_port = Port::<u32>::new(self.io_base + REG_RCR);
        rcr_port.write(0xCF);

        // Enable Rx and Tx
        cmd_port.write(0x0C);
        
        // --- DIAGNOSTIC CHECK ---
        // Read back to confirm card accepted our values
        let rcr_read = rcr_port.read();
        let rbstart_read = rbstart_port.read();
        let cmd_read = cmd_port.read();
        
        writer::print(&format!("[NET DEBUG] RCR: {:x} (Want CF) | RBSTART: {:x} | CMD: {:x}\n", 
            rcr_read, rbstart_read, cmd_read));
            
        writer::print("[NET] RTL8139 Initialized.\n");
    }

    pub fn send_hello(&mut self) {
        unsafe {
            writer::print(&format!("[NET] Sending on Descriptor {}...\n", self.tx_cur));

            let mut idx = 0;
            // Broadcast Dest
            for _ in 0..6 { self.write_tx(idx, 0xFF); idx += 1; }
            // Src
            for i in 0..6 { self.write_tx(idx, self.mac_addr[i]); idx += 1; }
            // Type/Len
            self.write_tx(idx, 0x08); idx += 1; self.write_tx(idx, 0x00); idx += 1;
            // Payload
            for &b in b"CHRONOS" { self.write_tx(idx, b); idx += 1; }
            while idx < 60 { self.write_tx(idx, 0); idx += 1; }

            let tsd_port_off = REG_TSD0 + (self.tx_cur as u16 * 4);
            let tsad_port_off = REG_TSAD0 + (self.tx_cur as u16 * 4);

            let mut tsad = Port::<u32>::new(self.io_base + tsad_port_off);
            tsad.write(TX_BUFFER_PHYS);

            let mut tsd = Port::<u32>::new(self.io_base + tsd_port_off);
            tsd.write(idx as u32); 

            // Wait for send to complete
            for _ in 0..1000 { core::hint::spin_loop(); }
            
            let status = tsd.read();
            if (status & (1 << 15)) != 0 {
                writer::print("[TX] Status OK.\n");
            } else {
                writer::print(&format!("[TX] Fail code: {:x}\n", status));
            }

            self.tx_cur = (self.tx_cur + 1) % 4;
        }
    }

    unsafe fn write_tx(&self, offset: isize, val: u8) {
        core::ptr::write_volatile(self.tx_buffer_ptr.offset(offset), val);
    }
    pub fn send_arp(&mut self) {
        unsafe {
            writer::print(&format!("[NET] Sending ARP Request (Who is 10.0.2.2?)... desc {}\n", self.tx_cur));

            let mut idx = 0;
            
            // --- ETHERNET HEADER (14 bytes) ---
            // 1. Destination: Broadcast (FF:FF:FF:FF:FF:FF)
            for _ in 0..6 { self.write_tx(idx, 0xFF); idx += 1; }
            
            // 2. Source: Our MAC
            for i in 0..6 { self.write_tx(idx, self.mac_addr[i]); idx += 1; }
            
            // 3. EtherType: ARP (0x0806) - Big Endian
            self.write_tx(idx, 0x08); idx += 1; 
            self.write_tx(idx, 0x06); idx += 1;

            // --- ARP PAYLOAD (28 bytes) ---
            // 4. Hardware Type: Ethernet (1)
            self.write_tx(idx, 0x00); idx += 1; self.write_tx(idx, 0x01); idx += 1;
            
            // 5. Protocol Type: IPv4 (0x0800)
            self.write_tx(idx, 0x08); idx += 1; self.write_tx(idx, 0x00); idx += 1;
            
            // 6. Hardware/Protocol Len (6, 4)
            self.write_tx(idx, 0x06); idx += 1; 
            self.write_tx(idx, 0x04); idx += 1;
            
            // 7. Opcode: Request (1)
            self.write_tx(idx, 0x00); idx += 1; self.write_tx(idx, 0x01); idx += 1;
            
            // 8. Sender MAC (Us)
            for i in 0..6 { self.write_tx(idx, self.mac_addr[i]); idx += 1; }
            
            // 9. Sender IP (0.0.0.0) - We don't have one yet
            for _ in 0..4 { self.write_tx(idx, 0x00); idx += 1; }
            
            // 10. Target MAC (Ignored/Zeros)
            for _ in 0..6 { self.write_tx(idx, 0x00); idx += 1; }
            
            // 11. Target IP (10.0.2.2 - QEMU Gateway)
            self.write_tx(idx, 10); idx += 1;
            self.write_tx(idx, 0);  idx += 1;
            self.write_tx(idx, 2);  idx += 1;
            self.write_tx(idx, 2);  idx += 1;

            // Pad to 60 bytes (Ethernet minimum)
            while idx < 60 { self.write_tx(idx, 0); idx += 1; }

            // --- TRANSMIT COMMAND ---
            let tsd_port_off = REG_TSD0 + (self.tx_cur as u16 * 4);
            let tsad_port_off = REG_TSAD0 + (self.tx_cur as u16 * 4);

            let mut tsad = Port::<u32>::new(self.io_base + tsad_port_off);
            tsad.write(TX_BUFFER_PHYS);

            let mut tsd = Port::<u32>::new(self.io_base + tsd_port_off);
            tsd.write(idx as u32); // Fire!

            self.tx_cur = (self.tx_cur + 1) % 4;
        }
    }    

    pub fn sniff_packet(&self) {
        unsafe {
            // 1. Check if packet exists (header != 0)
            let header = core::ptr::read_volatile(self.rx_buffer_ptr as *const u32);
            
            if header != 0 {
                 // The RTL8139 puts a 4-byte header BEFORE the actual packet data.
                 // Header = [Status (16 bits), Length (16 bits)]
                 // The packet data starts at offset 4.
                 
                 let length = (header >> 16) as usize;
                 
                 // Valid length check (Ethernet min 60, max 1514)
                 if length > 0 && length < 2000 {
                     // Create a slice of the PACKET DATA (skip the 4-byte header)
                     let packet_data = core::slice::from_raw_parts(
                         self.rx_buffer_ptr.add(4), 
                         length
                     );
                     
                     // PASS TO NETWORK STACK
                     net::handle_packet(packet_data);
                 }
                 
                 // Clear buffer to wait for next packet
                 // (In real driver, we'd move CAPR)
                 core::ptr::write_volatile(self.rx_buffer_ptr as *mut u32, 0);
            }
        }
    }
}
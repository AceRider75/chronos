use x86_64::instructions::port::Port;
use alloc::vec::Vec;

// PRIMARY BUS PORTS
const DATA_PORT: u16 = 0x1F0;
const ERROR_PORT: u16 = 0x1F1;
const SECTOR_COUNT_PORT: u16 = 0x1F2;
const LBA_LOW_PORT: u16 = 0x1F3;
const LBA_MID_PORT: u16 = 0x1F4;
const LBA_HIGH_PORT: u16 = 0x1F5;
const DRIVE_PORT: u16 = 0x1F6;
const COMMAND_PORT: u16 = 0x1F7;
const STATUS_PORT: u16 = 0x1F7;

// COMMANDS
const CMD_READ_SECTORS: u8 = 0x20;
const CMD_WRITE_SECTORS: u8 = 0x30;
const CMD_IDENTIFY: u8 = 0xEC;

pub struct AtaDrive {
    master: bool,
}

impl AtaDrive {
    pub fn new(master: bool) -> Self {
        AtaDrive { master }
    }

    /// Reads a 256-word (512 byte) sector from LBA address
    pub fn read_sectors(&self, lba: u32, sectors: u8) -> Vec<u8> {
        unsafe {
            // 1. Wait for drive to be ready
            self.wait_busy();

            // 2. Select Drive and LBA (Top 4 bits)
            // 0xE0 = LBA Mode. 
            // If slave (not master), set bit 4 (0x10).
            let drive_select = 0xE0 | ((lba >> 24) as u8 & 0x0F) | if self.master { 0 } else { 0x10 };
            Port::<u8>::new(DRIVE_PORT).write(drive_select);

            // 3. Send Parameters
            Port::<u8>::new(SECTOR_COUNT_PORT).write(sectors);
            Port::<u8>::new(LBA_LOW_PORT).write(lba as u8);
            Port::<u8>::new(LBA_MID_PORT).write((lba >> 8) as u8);
            Port::<u8>::new(LBA_HIGH_PORT).write((lba >> 16) as u8);

            // 4. Send Command
            Port::<u8>::new(COMMAND_PORT).write(CMD_READ_SECTORS);

            // 5. Read Data
            let mut data = Vec::new();
            
            for _ in 0..sectors {
                self.wait_busy();
                self.wait_drq(); // Wait for Data Request bit

                for _ in 0..256 { // 256 words = 512 bytes
                    let word = Port::<u16>::new(DATA_PORT).read();
                    data.push((word & 0xFF) as u8);
                    data.push((word >> 8) as u8);
                }
            }
            data
        }
    }

    /// Writes data to sector. Data must be multiple of 512 bytes.
    pub fn write_sectors(&self, lba: u32, data: &[u8]) {
        unsafe {
            self.wait_busy();
            let sectors = (data.len() / 512) as u8;

            let drive_select = 0xE0 | ((lba >> 24) as u8 & 0x0F) | if self.master { 0 } else { 0x10 };
            Port::<u8>::new(DRIVE_PORT).write(drive_select);

            Port::<u8>::new(SECTOR_COUNT_PORT).write(sectors);
            Port::<u8>::new(LBA_LOW_PORT).write(lba as u8);
            Port::<u8>::new(LBA_MID_PORT).write((lba >> 8) as u8);
            Port::<u8>::new(LBA_HIGH_PORT).write((lba >> 16) as u8);

            Port::<u8>::new(COMMAND_PORT).write(CMD_WRITE_SECTORS);

            // Write Data
            for chunk in data.chunks(512) {
                self.wait_busy();
                self.wait_drq();

                for i in (0..512).step_by(2) {
                    let word = (chunk[i] as u16) | ((chunk[i+1] as u16) << 8);
                    Port::<u16>::new(DATA_PORT).write(word);
                }
                
                // Flush cache logic is usually needed here for real hardware
                // Port::<u8>::new(COMMAND_PORT).write(0xE7); // Cache Flush
            }
        }
    }

    // Helper: Wait until BSY (Busy) bit is 0
    unsafe fn wait_busy(&self) {
        let mut port = Port::<u8>::new(STATUS_PORT);
        // Bit 7 = BSY
        while (port.read() & 0x80) != 0 { core::hint::spin_loop(); }
    }

    // Helper: Wait until DRQ (Data Request) bit is 1
    unsafe fn wait_drq(&self) {
        let mut port = Port::<u8>::new(STATUS_PORT);
        // Bit 3 = DRQ
        while (port.read() & 0x08) == 0 { core::hint::spin_loop(); }
    }
    
    // Check if drive exists via Identify
    pub fn identify(&self) -> bool {
        unsafe {
            Port::<u8>::new(DRIVE_PORT).write(if self.master { 0xA0 } else { 0xB0 });
            Port::<u8>::new(COMMAND_PORT).write(CMD_IDENTIFY);
            
            if Port::<u8>::new(STATUS_PORT).read() == 0 { return false; }
            
            // Poll until BSY clears
            let mut port = Port::<u8>::new(STATUS_PORT);
            while (port.read() & 0x80) != 0 { 
                if (port.read() & 0x01) != 0 { return false; } // Error
            }
            
            // Check Data Ready
            if (port.read() & 0x08) != 0 {
                // Read 256 words to clear buffer
                for _ in 0..256 { Port::<u16>::new(DATA_PORT).read(); }
                return true;
            }
            false
        }
    }
}
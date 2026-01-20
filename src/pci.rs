use x86_64::instructions::port::Port;
use crate::writer;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

#[derive(Debug, Clone)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
}

// 1. READ CONFIGURATION WORD
// This functions talks to the hardware ports
unsafe fn pci_read_word(bus: u8, slot: u8, func: u8, offset: u8) -> u16 {
    let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
    let mut data_port = Port::<u32>::new(CONFIG_DATA);

    // Create the address packet:
    // Bit 31: Enable bit
    // Bits 23-16: Bus number
    // Bits 15-11: Device number
    // Bits 10-8: Function number
    // Bits 7-2: Register offset
    let address = 0x80000000 
                | ((bus as u32) << 16) 
                | ((slot as u32) << 11) 
                | ((func as u32) << 8) 
                | ((offset as u32) & 0xFC);

    address_port.write(address);
    
    // Read the data and shift to get the specific word we want
    let val = data_port.read();
    ((val >> ((offset & 2) * 8)) & 0xFFFF) as u16
}

// 2. SCAN THE BUS
pub fn scan_bus() -> Vec<PciDevice> {
    let mut devices = Vec::new();

    // Brute force scan: 256 Busses, 32 Slots per bus
    for bus in 0..=255 {
        for slot in 0..32 {
            unsafe {
                // Register 0 contains Vendor ID
                let vendor_id = pci_read_word(bus, slot, 0, 0);
                
                // If Vendor ID is 0xFFFF, the slot is empty
                if vendor_id != 0xFFFF {
                    // Register 2 contains Device ID
                    let device_id = pci_read_word(bus, slot, 0, 2);
                    
                    devices.push(PciDevice {
                        bus,
                        device: slot,
                        function: 0, // Assuming function 0 for simplicity
                        vendor_id,
                        device_id,
                    });
                }
            }
        }
    }
    devices
}

// Helper to translate ID to human name
pub fn lookup_vendor(id: u16) -> &'static str {
    match id {
        0x8086 => "Intel Corp",
        0x10EC => "Realtek",
        0x10DE => "NVIDIA",
        0x1234 => "QEMU / Bochs",
        0x1AF4 => "VirtIO",
        _ => "Unknown",
    }
}

// ... existing code ...

// NEW: Read a 32-bit double word (needed for BARs)
pub unsafe fn pci_read_u32(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
    let mut data_port = Port::<u32>::new(CONFIG_DATA);

    let address = 0x80000000 
                | ((bus as u32) << 16) 
                | ((slot as u32) << 11) 
                | ((func as u32) << 8) 
                | ((offset as u32) & 0xFC);

    address_port.write(address);
    data_port.read()
}

// NEW: Write a 32-bit double word
pub unsafe fn pci_write_u32(bus: u8, slot: u8, func: u8, offset: u8, value: u32) {
    let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
    let mut data_port = Port::<u32>::new(CONFIG_DATA);

    let address = 0x80000000 
                | ((bus as u32) << 16) 
                | ((slot as u32) << 11) 
                | ((func as u32) << 8) 
                | ((offset as u32) & 0xFC);

    address_port.write(address);
    data_port.write(value);
}

// NEW: Enable Bus Mastering
// This sets Bit 2 in the Command Register (Offset 0x04)
pub fn enable_bus_mastering(device: PciDevice) {
    unsafe {
        let command_reg = pci_read_u32(device.bus, device.device, device.function, 0x04);
        // Set bit 2 (Bus Master) and bit 0 (IO Space)
        let new_command = command_reg | (1 << 2) | (1 << 0);
        pci_write_u32(device.bus, device.device, device.function, 0x04, new_command);
    }
}
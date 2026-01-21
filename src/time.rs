use x86_64::instructions::port::Port;

const CMOS_ADDR: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

pub struct Time {
    pub hours: u8,
    pub minutes: u8,
    pub seconds: u8,
}

pub fn read_rtc() -> Time {
    unsafe {
        // Wait until RTC is not updating (Bit 7 of Register A)
        while is_updating() { core::hint::spin_loop(); }

        let mut seconds = read_register(0x00);
        let mut minutes = read_register(0x02);
        let mut hours = read_register(0x04);
        
        let register_b = read_register(0x0B);

        // Convert BCD to Binary if necessary
        // (If Bit 2 of Register B is 0, it's BCD)
        if (register_b & 0x04) == 0 {
            seconds = (seconds & 0x0F) + ((seconds / 16) * 10);
            minutes = (minutes & 0x0F) + ((minutes / 16) * 10);
            hours = (hours & 0x0F) + ((hours / 16) * 10) | (hours & 0x80);
        }

        Time { hours, minutes, seconds }
    }
}

unsafe fn is_updating() -> bool {
    let mut addr = Port::<u8>::new(CMOS_ADDR);
    let mut data = Port::<u8>::new(CMOS_DATA);
    addr.write(0x0A); // Select Status Register A
    (data.read() & 0x80) != 0
}

unsafe fn read_register(reg: u8) -> u8 {
    let mut addr = Port::<u8>::new(CMOS_ADDR);
    let mut data = Port::<u8>::new(CMOS_DATA);
    addr.write(reg);
    data.read()
}
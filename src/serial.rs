use x86_64::instructions::port::Port;
use core::fmt;
use spin::Mutex;
use lazy_static::lazy_static;

pub struct SerialPort {
    data: Port<u8>,
    int_en: Port<u8>,
    fifo_ctrl: Port<u8>,
    line_ctrl: Port<u8>,
    modem_ctrl: Port<u8>,
    line_sts: Port<u8>,
}

impl SerialPort {
    pub fn new(base: u16) -> Self {
        Self {
            data: Port::new(base),
            int_en: Port::new(base + 1),
            fifo_ctrl: Port::new(base + 2),
            line_ctrl: Port::new(base + 3),
            modem_ctrl: Port::new(base + 4),
            line_sts: Port::new(base + 5),
        }
    }

    pub fn init(&mut self) {
        unsafe {
            self.int_en.write(0x00);    // Disable interrupts
            self.line_ctrl.write(0x80); // Enable DLAB (set baud rate divisor)
            self.data.write(0x03);      // Set divisor to 3 (lo byte) 38400 baud
            self.int_en.write(0x00);    //                  (hi byte)
            self.line_ctrl.write(0x03); // 8 bits, no parity, one stop bit
            self.fifo_ctrl.write(0xC7); // Enable FIFO, clear them, with 14-byte threshold
            self.modem_ctrl.write(0x0B); // IRQs enabled, RTS/DSR set
        }
    }

    fn is_transmit_empty(&mut self) -> bool {
        unsafe { self.line_sts.read() & 0x20 != 0 }
    }

    pub fn send(&mut self, data: u8) {
        while !self.is_transmit_empty() {}
        unsafe { self.data.write(data); }
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.send(byte);
        }
        Ok(())
    }
}

lazy_static! {
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = SerialPort::new(0x3F8);
        serial_port.init();
        Mutex::new(serial_port)
    };
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        SERIAL1.lock().write_fmt(args).unwrap();
    });
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(concat!($fmt, "\n"), $($arg)*));
}

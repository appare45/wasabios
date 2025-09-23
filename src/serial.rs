use core::fmt;

use crate::result::Result;
use crate::x86::busy_loop_hint;
use crate::x86::read_io_port_u8;
use crate::x86::write_io_port_u8;

pub struct SerialPort {
    base: u16,
}

#[derive(Debug)]
enum Mode {
    Loopback = 0x1e,
    Default = 0x0f,
}

struct LineStatus {
    value: u8,
}

impl LineStatus {
    fn new(value: u8) -> Self {
        Self { value }
    }

    fn received(&self) -> bool {
        self.value & 0x01 == 1
    }
}

impl SerialPort {
    pub fn new(base: u16) -> Self {
        Self { base }
    }

    pub fn new_for_com1() -> Self {
        Self::new(0x3f8)
    }

    /**
     * self.base = Receiver Buffer Register = データ自体がある
     * self.base = モードを設定する、0x1eを書き込むとシリアルポートがループバックモードになる
     * self.base+5 = Line Status Register = 1になればデータが届いている
     */

    pub fn init(&mut self) {
        write_io_port_u8(self.base + 1, 0x00);
        write_io_port_u8(self.base + 3, 0x80);
        const BAUD_DIVISOR: u16 = 0x0001;
        write_io_port_u8(self.base, (BAUD_DIVISOR & 0xff) as u8);
        write_io_port_u8(self.base, (BAUD_DIVISOR >> 8) as u8);
        write_io_port_u8(self.base, 0x03);
        write_io_port_u8(self.base + 2, 0xC7);
        write_io_port_u8(self.base + 4, 0x0B);
    }

    fn set_mode(&self, mode: Mode) {
        write_io_port_u8(self.base + 4, mode as u8);
    }

    fn get_line_status(&self) -> LineStatus {
        LineStatus::new(read_io_port_u8(self.base + 5))
    }

    pub fn send_char(&self, c: char) {
        while (read_io_port_u8(self.base + 5) & 0x20) == 0 {
            busy_loop_hint();
        }
        write_io_port_u8(self.base, c as u8)
    }

    pub fn try_read(&self) -> Option<u8> {
        if !self.get_line_status().received() {
            None
        } else {
            let c = read_io_port_u8(self.base);
            write_io_port_u8(self.base + 2, 0xC7);
            Some(c)
        }
    }

    pub fn loopback_test(&self) -> Result<()> {
        self.set_mode(Mode::Loopback);
        let test_c = 't';
        self.send_char(test_c);
        let c = self.try_read().ok_or("loopback_test failed: No response")?;
        if c != (test_c as u8) {
            return Err("loopback_test failed: wrong data received");
        }
        self.set_mode(Mode::Default);
        Ok(())
    }

    pub fn send_str(&self, s: &str) {
        let mut sc = s.chars();
        let slen = s.chars().count();
        for _ in 0..slen {
            self.send_char(sc.next().unwrap());
        }
    }
}

impl Default for SerialPort {
    fn default() -> Self {
        Self::new_for_com1()
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let serial = Self::default();
        serial.send_str(s);
        Ok(())
    }
}

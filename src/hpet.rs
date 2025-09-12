use core::mem::size_of;
use core::ptr::read_volatile;
use core::ptr::write_volatile;
use core::time::Duration;

use crate::mutex::Mutex;

const TIMER_CONFIG_LEVEL_TRIGGER: u64 = 1 << 1;
const TIMER_CONFIG_ENABLE: u64 = 1 << 2;
const TIMER_CONFIG_PERIODIC: u64 = 1 << 3;

#[repr(C)]
struct TimerRegister {
    // 2.3.8
    // Timer N Configuration and Capabilities Register
    configuration_and_capabilities: u64,
    _reserved: [u64; 3],
}
const _: () = assert!(size_of::<TimerRegister>() == 0x20);
impl TimerRegister {
    unsafe fn write_config(&mut self, config: u64) {
        write_volatile(&mut self.configuration_and_capabilities, config);
    }
}

#[repr(C)]
pub struct HpetRegisters {
    // hpetの仕様書2.3.4に書いてある
    // General Capabilities and ID Register
    // Read-Only
    capabilites_and_id: u64,
    _reserved0: u64,
    // 2.3.5 General Configuration Register
    configuration: u64,
    _reserved1: [u64; 27],
    // 2.3.7 Main Counter Register
    main_counter_value: u64,
    _reserved2: u64,
    timers: [TimerRegister; 32],
}
const _: () = assert!(size_of::<HpetRegisters>() == 0x500);

pub struct Hpet {
    registers: &'static mut HpetRegisters,
    #[allow(unused)]
    num_of_timers: usize,
    frequency: u64,
}
static HPET: Mutex<Option<Hpet>> = Mutex::new(None);
pub fn set_global_hpet(hpet: Hpet) {
    assert!(HPET.lock().is_none());
    *HPET.lock() = Some(hpet);
}
pub fn global_timestamp() -> Duration {
    if let Some(hpet) = &*HPET.lock() {
        let ns = hpet.main_counter() * 1_000_000_000 / hpet.freq();
        Duration::from_nanos(ns)
    } else {
        Duration::ZERO
    }
}
impl Hpet {
    unsafe fn globally_disable(&mut self) {
        let config = read_volatile(&self.registers.configuration) & !0b11;
        write_volatile(&mut self.registers.configuration, config);
    }
    unsafe fn globally_enable(&mut self) {
        let config = read_volatile(&self.registers.configuration) | 0b01;
        write_volatile(&mut self.registers.configuration, config);
    }
    pub fn main_counter(&self) -> u64 {
        unsafe { read_volatile(&self.registers.main_counter_value) }
    }
    pub fn freq(&self) -> u64 {
        self.frequency
    }
    pub fn new(registers: &'static mut HpetRegisters) -> Hpet {
        let counter_clk_period = registers.capabilites_and_id >> 32;
        let num_of_timers = ((registers.capabilites_and_id >> 8) & 0b11111) as usize + 1;
        let frequency = 1_000_000_000_000_000 / counter_clk_period;
        let mut hpet = Self {
            registers,
            num_of_timers,
            frequency,
        };
        unsafe {
            hpet.globally_disable();
            for i in 0..hpet.num_of_timers {
                let timer = &mut hpet.registers.timers[i];
                let mut config = read_volatile(&timer.configuration_and_capabilities);
                config &= !(TIMER_CONFIG_ENABLE
                    | TIMER_CONFIG_LEVEL_TRIGGER
                    | TIMER_CONFIG_PERIODIC
                    | (0b1111 << 9));
                timer.write_config(config);
            }
            write_volatile(&mut hpet.registers.main_counter_value, 0);
            hpet.globally_enable();
        }
        hpet
    }
}

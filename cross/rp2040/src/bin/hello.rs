#![no_std]
#![no_main]

use cortex_m::asm;
use defmt_rtt as _; // global logger
use panic_probe as _;
use rp_pico as _; // includes boot2

#[cortex_m_rt::entry]
fn main() -> ! {
    defmt::println!("Hello RP2040!");

    loop {
        asm::bkpt()
    }
}

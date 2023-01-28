#![no_std]
#![no_main]

use defmt_rtt as _; // global logger
use panic_probe as _;
use cortex_m::asm;

#[cortex_m_rt::entry]
fn main() -> ! {
    cotton_stm32_eth::hello();

    loop {
        asm::bkpt()
    }
}

#![no_std]
#![no_main]

use defmt_rtt as _; // global logger
use panic_probe as _;
use stm32f7xx_hal as _;
use cortex_m::asm;

#[cortex_m_rt::entry]
fn main() -> ! {
    defmt::println!("Hello STM32F746 Nucleo!");

    loop {
        asm::bkpt()
    }
}

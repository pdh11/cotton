#![no_std]
#![no_main]

// See https://github.com/rp-rs/rp-hal/tree/main/rp235x-hal-examples

use cortex_m::asm;
use panic_probe as _;
use defmt_rtt as _; // global logger
use rp235x_hal as hal;

#[link_section = ".start_block"]
#[used]
pub static IMAGE_DEF: hal::block::ImageDef = hal::block::ImageDef::secure_exe();

#[hal::entry]
fn main() -> ! {
    defmt::println!(
        "{} from {} {}-g{}",
        env!("CARGO_BIN_NAME"),
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        git_version::git_version!()
    );
    loop {
        asm::bkpt()
    }
}

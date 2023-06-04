#![no_std]
#![no_main]

use cortex_m::asm;
use defmt_rtt as _; // global logger
use defmt_rtt as _;
use panic_probe as _;
use rp_pico as _; // includes boot2

#[rtic::app(device = rp_pico::hal::pac, peripherals = true)]
mod app {
    use crate::app::hal::timer::Alarm;
    use core::fmt::Write;
    use core::hash::Hasher;
    use rp_pico::hal;
    use rp_pico::pac;
    use rp_pico::XOSC_CRYSTAL_FREQ;

    struct UniqueId(u64, u64);

    fn rp2040_unique_id() -> UniqueId {
        let mut unique_id = [0u8; 16];
        unsafe { rp2040_flash::flash::flash_unique_id(&mut unique_id, true) };

        defmt::println!("Unique id {}", unique_id[0..8]);
        UniqueId(
            u64::from_le_bytes(unique_id[0..8].try_into().unwrap()),
            u64::from_le_bytes(unique_id[8..16].try_into().unwrap()),
        )
    }

    fn unique_id(rp2040_id: &UniqueId, salt: &[u8]) -> u64 {
        let mut h =
            siphasher::sip::SipHasher::new_with_keys(rp2040_id.0, rp2040_id.1);
        h.write(salt);
        h.finish()
    }

    fn mac_address(rp2040_id: &UniqueId) -> [u8; 6] {
        let mut mac_address = [0u8; 6];
        let r = unique_id(rp2040_id, b"w5500-mac").to_ne_bytes();
        mac_address.copy_from_slice(&r[0..6]);
        mac_address[0] &= 0xFE; // clear multicast bit
        mac_address[0] |= 2; // set local bit
        mac_address
    }

    #[shared]
    struct Shared {
        timer: hal::Timer,
        alarm: hal::timer::Alarm0,
    }

    #[local]
    struct Local {}

    #[init(local = [usb_bus: Option<u32> = None])]
    fn init(c: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::println!("Pre-init");

        let rp2040_id = rp2040_unique_id();
        let mac = mac_address(&rp2040_id);
        defmt::println!("MAC address: {}", mac);
        //*******
        // Initialization of the system clock.

        let mut resets = c.device.RESETS;
        let mut watchdog = hal::watchdog::Watchdog::new(c.device.WATCHDOG);

        // Configure the clocks - The default is to generate a 125 MHz system clock
        let clocks = hal::clocks::init_clocks_and_plls(
            XOSC_CRYSTAL_FREQ,
            c.device.XOSC,
            c.device.CLOCKS,
            c.device.PLL_SYS,
            c.device.PLL_USB,
            &mut resets,
            &mut watchdog,
        )
        .ok()
        .unwrap();

        //*******
        // Initialization of the LED GPIO and the timer.

        let sio = hal::Sio::new(c.device.SIO);
        let pins = rp_pico::Pins::new(
            c.device.IO_BANK0,
            c.device.PADS_BANK0,
            sio.gpio_bank0,
            &mut resets,
        );
        //        let mut led = pins.led.into_push_pull_output();
        //        led.set_low().unwrap();

        let mut timer = hal::Timer::new(c.device.TIMER, &mut resets, &clocks);
        let mut alarm = timer.alarm_0().unwrap();
        //        let _ = alarm.schedule(SCAN_TIME_US.microseconds());
        //        alarm.enable_interrupt(&mut timer);

        // Enable led_blink.
        //        let led_blink_enable = true;

        // Reset the counter
        //        let counter = Counter::new();
        defmt::println!("Hello RP2040 rtic");

        // todo: see
        // https://github.com/rp-rs/rp-hal/blob/main/rp2040-hal/examples/gpio_irq_example.rs
        // https://docs.rs/w5500-dhcp/latest/w5500_dhcp/struct.Client.html
        //
        // and especially:
        // https://github.com/newAM/ambientsensor-rs/blob/main/src/main.rs
        (Shared { timer, alarm }, Local {}, init::Monotonics())
    }

    /*
    // Task with least priority that only runs when nothing else is running.
    #[idle(local = [x: u32 = 0])]
    fn idle(_cx: idle::Context) -> ! {
        // Locals in idle have lifetime 'static
        // let _x: &'static mut u32 = cx.local.x;

        //hprintln!("idle").unwrap();

        loop {
            cortex_m::asm::nop();
        }
    }
    */
}

#![no_std]
#![no_main]

use cortex_m::asm;
use defmt_rtt as _; // global logger
use defmt_rtt as _;
use panic_probe as _;
use rp_pico as _; // includes boot2

/*
struct BlockingSpi<T: embedded_hal_nb::spi::FullDuplex + embedded_hal_nb::spi::ErrorType>(T);

impl<T: embedded_hal_nb::spi::FullDuplex + embedded_hal_nb::spi::ErrorType> embedded_hal::spi::ErrorType for BlockingSpi<T> {
    type Error = T::Error;
}

impl<T: embedded_hal_nb::spi::FullDuplex + embedded_hal_nb::spi::ErrorType> embedded_hal::spi::SpiDevice for BlockingSpi<T> {
    fn transaction(&mut self, _: &mut [embedded_hal::spi::Operation<'_, u8>]) -> Result<(), <Self as embedded_hal::spi::ErrorType>::Error> {
        todo!()
    }
}*/

#[rtic::app(device = rp_pico::hal::pac, peripherals = true)]
mod app {
    use crate::app::hal::timer::Alarm;
    use core::fmt::Write;
    use core::hash::Hasher;
    use rp_pico::pac;
    use rp_pico::XOSC_CRYSTAL_FREQ;
    use rp2040_hal as hal;
    use rp2040_hal::Clock;
    use embedded_hal::spi::SpiDevice;
    use rp2040_hal::fugit::RateExtU32;
    use w5500_ll::eh1::vdm::W5500;

    /*
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
    */

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

//        let rp2040_id = rp2040_unique_id();
//        let mac = mac_address(&rp2040_id);
//        defmt::println!("MAC address: {}", mac);
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

        // W5500-EVB-Pico:
        //   W5500 SPI on SPI0
        //         nCS = GPIO17
        //         TX = GPIO19
        //         RX = GPIO16
        //         SCK = GPIO18
        //   W5500 INTn on GPIO21
        //   W5500 RSTn on GPIO20
        //   Green LED on GPIO25

        // todo: see
        // https://github.com/rp-rs/rp-hal/blob/main/rp2040-hal/examples/gpio_irq_example.rs
        // https://docs.rs/w5500-dhcp/latest/w5500_dhcp/struct.Client.html
        //
        // and especially:
        // https://github.com/newAM/ambientsensor-rs/blob/main/src/main.rs

        // These are implicitly used by the spi driver if they are in the correct mode
        let mut pac = pac::Peripherals::take().unwrap();
        let spi_mosi = pins.gpio7.into_function::<hal::gpio::FunctionSpi>();
        let spi_miso = pins.gpio4.into_function::<hal::gpio::FunctionSpi>();
        let spi_sclk = pins.gpio6.into_function::<hal::gpio::FunctionSpi>();
        let spi = hal::spi::Spi::<_, _, _, 8>::new(pac.SPI0, (spi_mosi, spi_miso, spi_sclk));

        // Exchange the uninitialised SPI driver for an initialised one
        let mut spi = spi.init(
            &mut pac.RESETS,
            clocks.peripheral_clock.freq(),
            16u32.MHz(),
            hal::spi::FrameFormat::MotorolaSpi(embedded_hal::spi::MODE_0),
        );

        //        let mut spi = crate::BlockingSpi(spi);

        let spi_ncs = pins.gpio17.into_push_pull_output();
        let mut spi = embedded_hal_bus::spi::ExclusiveDevice::new_no_delay(spi,
                                                                           spi_ncs);

        let mut w5500 = W5500::new(spi);

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

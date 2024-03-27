#![no_std]
#![no_main]

use cortex_m::asm;
use defmt_rtt as _; // global logger
use defmt_rtt as _;
use panic_probe as _;
use rp_pico as _; // includes boot2

#[rtic::app(device = rp_pico::hal::pac, peripherals = true, dispatchers = [ADC_IRQ_FIFO])]
mod app {
    use crate::app::hal::timer::Alarm;
    use core::fmt::Write;
    use core::hash::Hasher;
    use embedded_hal::delay::DelayNs;
    use embedded_hal::spi::SpiDevice;
    use embedded_hal_bus::spi::ExclusiveDevice;
    use embedded_hal_bus::spi::NoDelay;
    use fugit::ExtU64;
    use rp2040_hal as hal;
    use rp2040_hal::fugit::RateExtU32;
    use rp2040_hal::gpio::bank0::Gpio16;
    use rp2040_hal::gpio::bank0::Gpio17;
    use rp2040_hal::gpio::bank0::Gpio18;
    use rp2040_hal::gpio::bank0::Gpio19;
    use rp2040_hal::gpio::bank0::Gpio21;
    use rp2040_hal::gpio::FunctionSio;
    use rp2040_hal::gpio::FunctionSpi;
    use rp2040_hal::gpio::Interrupt::EdgeLow;
    use rp2040_hal::gpio::PullDown;
    use rp2040_hal::gpio::PullNone;
    use rp2040_hal::gpio::SioInput;
    use rp2040_hal::gpio::SioOutput;
    use rp2040_hal::pac::SPI0;
    use rp2040_hal::Clock;
    use rp_pico::pac;
    use rp_pico::XOSC_CRYSTAL_FREQ;
    use systick_monotonic::Systick;
    use w5500_dhcp::{hl::Hostname, Client as DhcpClient};
    use w5500_ll::eh1::vdm::W5500;
    use w5500_ll::{
        net::{Eui48Addr, Ipv4Addr, SocketAddrV4},
        LinkStatus, OperationMode, PhyCfg, Registers, Sn,
    };

    const DHCP_SN: Sn = Sn::Sn0;
    const NAME: &str = "rp2040-w5500";
    const HOSTNAME: Hostname<'static> = Hostname::new_unwrapped(NAME);

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
    }*/

    #[shared]
    struct Shared {
        timer: hal::Timer,
        alarm: hal::timer::Alarm0,
    }

    #[local]
    struct Local {
        nvic: cortex_m::peripheral::NVIC,
        w5500: W5500<
            ExclusiveDevice<
                rp2040_hal::Spi<
                    rp2040_hal::spi::Enabled,
                    SPI0,
                    (
                        rp2040_hal::gpio::Pin<Gpio19, FunctionSpi, PullDown>,
                        rp2040_hal::gpio::Pin<Gpio16, FunctionSpi, PullDown>,
                        rp2040_hal::gpio::Pin<Gpio18, FunctionSpi, PullDown>,
                    ),
                >,
                rp2040_hal::gpio::Pin<
                    Gpio17,
                    FunctionSio<SioOutput>,
                    PullDown,
                >,
                NoDelay,
            >,
        >,
        w5500_irq:
            rp2040_hal::gpio::Pin<Gpio21, FunctionSio<SioInput>, PullNone>,
        dhcp: DhcpClient<'static>,
    }

    #[monotonic(binds = SysTick, default = true)]
    type Monotonic = Systick<1000>;

    #[init(local = [usb_bus: Option<u32> = None])]
    fn init(mut c: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::println!("Pre-init");

        //        let rp2040_id = rp2040_unique_id();
        //        let mac = mac_address(&rp2040_id);
        //        defmt::println!("MAC address: {}", mac);

        let mac = w5500_ll::net::Eui48Addr {
            octets: [2u8, 1u8, 2u8, 3u8, 4u8, 5u8],
        };

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

        let mono = Systick::new(c.core.SYST, clocks.system_clock.freq().raw());

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
        //         TX (MOSI) = GPIO19
        //         RX (MISO) = GPIO16
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
        //        let mut pac = pac::Peripherals::steal();
        let spi_mosi = pins.gpio19.into_function::<hal::gpio::FunctionSpi>();
        let spi_miso = pins.gpio16.into_function::<hal::gpio::FunctionSpi>();
        let spi_sclk = pins.gpio18.into_function::<hal::gpio::FunctionSpi>();
        let spi = hal::spi::Spi::<_, _, _, 8>::new(
            c.device.SPI0,
            (spi_mosi, spi_miso, spi_sclk),
        );

        // Exchange the uninitialised SPI driver for an initialised one
        let mut spi = spi.init(
            &mut resets,
            clocks.peripheral_clock.freq(),
            16u32.MHz(),
            hal::spi::FrameFormat::MotorolaSpi(embedded_hal::spi::MODE_0),
        );

        let spi_ncs = pins.gpio17.into_push_pull_output();
        let mut spi =
            embedded_hal_bus::spi::ExclusiveDevice::new_no_delay(spi, spi_ncs);

        let mut w5500 = W5500::new(spi);

        let _phy_cfg: PhyCfg = 'outer: loop {
            // sanity check W5500 communications
            assert_eq!(w5500.version().unwrap(), w5500_dhcp::ll::VERSION);

            // load the MAC address we got from EEPROM
            w5500.set_shar(&mac).unwrap();
            debug_assert_eq!(w5500.shar().unwrap(), mac);

            // wait for the PHY to indicate the Ethernet link is up
            let mut attempts: u32 = 0;
            defmt::println!("Polling for link up");
            const PHY_CFG: PhyCfg =
                PhyCfg::DEFAULT.set_opmdc(OperationMode::FullDuplex10bt);
            w5500.set_phycfgr(PHY_CFG).unwrap();

            const LINK_UP_POLL_PERIOD_MILLIS: u32 = 100;
            const LINK_UP_POLL_ATTEMPTS: u32 = 50;
            loop {
                let phy_cfg: PhyCfg = w5500.phycfgr().unwrap();
                if phy_cfg.lnk() == LinkStatus::Up {
                    break 'outer phy_cfg;
                }
                if attempts >= LINK_UP_POLL_ATTEMPTS {
                    defmt::println!(
                        "Failed to link up in {} ms",
                        attempts * LINK_UP_POLL_PERIOD_MILLIS,
                    );
                    break;
                }
                timer.delay_ms(LINK_UP_POLL_PERIOD_MILLIS);
                attempts += 1;
            }
        };
        defmt::println!("Done link up");

        let w5500_irq = pins.gpio21.into_floating_input();
        w5500_irq.set_interrupt_enabled(EdgeLow, true);

        unsafe {
            pac::NVIC::unmask(pac::Interrupt::IO_IRQ_BANK0);
        }

        let seed: u64 = u64::from(cortex_m::peripheral::SYST::get_current())
            << 32
            | u64::from(cortex_m::peripheral::SYST::get_current());

        let dhcp = DhcpClient::new(DHCP_SN, seed, mac, HOSTNAME);
        dhcp.setup_socket(&mut w5500).unwrap();

        periodic::spawn_after(2.secs()).unwrap();

        (
            Shared { timer, alarm },
            Local {
                nvic: c.core.NVIC,
                w5500,
                w5500_irq,
                dhcp,
            },
            init::Monotonics(mono),
        )
    }

    #[task(local = [nvic])]
    fn periodic(cx: periodic::Context) {
        cortex_m::peripheral::NVIC::pend(pac::Interrupt::IO_IRQ_BANK0);
        periodic::spawn_after(2.secs()).unwrap();
    }

    #[task(binds = IO_IRQ_BANK0, local = [w5500, w5500_irq, dhcp], priority = 2)]
    fn eth_interrupt(cx: eth_interrupt::Context) {
        defmt::println!("ETH IRQ");
        let (w5500, w5500_irq, dhcp) =
            (cx.local.w5500, cx.local.w5500_irq, cx.local.dhcp);
        let had_lease = dhcp.has_lease();
        let now: u32 = monotonics::now()
            .duration_since_epoch()
            .to_secs()
            .try_into()
            .unwrap();
        let _spawn_after_secs: u32 = dhcp.process(w5500, now).unwrap();

        if dhcp.has_lease() && !had_lease {
            defmt::println!("DHCP succeeded! {}", dhcp.leased_ip())
        }

        w5500_irq.clear_interrupt(EdgeLow);
    }
}

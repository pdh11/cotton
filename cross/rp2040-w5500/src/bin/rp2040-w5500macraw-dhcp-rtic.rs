//! Example RTIC (1.0) application using RP2040 + W5500 to obtain a DHCP address
#![no_std]
#![no_main]
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

use defmt_rtt as _; // global logger
use defmt_rtt as _;
use panic_probe as _;
use rp_pico as _; // includes boot2
use smoltcp::iface::{self, SocketStorage};

#[rtic::app(device = rp_pico::hal::pac, peripherals = true, dispatchers = [ADC_IRQ_FIFO])]
mod app {
    use crate::NetworkStorage;
    use cross_rp2040_w5500::smoltcp::Stack;
    use embedded_hal::delay::DelayNs;
    use embedded_hal::digital::OutputPin;
    use fugit::ExtU64;
    use rp2040_hal as hal;
    use rp2040_hal::fugit::RateExtU32;
    use rp2040_hal::gpio::bank0::Gpio21;
    use rp2040_hal::gpio::FunctionSio;
    use rp2040_hal::gpio::Interrupt::EdgeLow;
    use rp2040_hal::gpio::PinState;
    use rp2040_hal::gpio::PullDown;
    use rp2040_hal::gpio::PullNone;
    use rp2040_hal::gpio::PullUp;
    use rp2040_hal::gpio::SioInput;
    use rp2040_hal::Clock;
    use rp_pico::pac;
    use rp_pico::XOSC_CRYSTAL_FREQ;
    use systick_monotonic::Systick;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        device: cotton_w5500::smoltcp::w5500_evb_pico::Device,
        stack: Stack,
        nvic: cortex_m::peripheral::NVIC,
        w5500_irq:
            rp2040_hal::gpio::Pin<Gpio21, FunctionSio<SioInput>, PullUp>,
    }

    #[monotonic(binds = SysTick, default = true)]
    type Monotonic = Systick<1000>;

    fn now_fn() -> smoltcp::time::Instant {
        let time = monotonics::now().duration_since_epoch().ticks();
        smoltcp::time::Instant::from_millis(time as i64)
    }

    #[init(local = [ storage: NetworkStorage = NetworkStorage::new() ])]
    fn init(c: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::println!("Pre-init");
        let unique_id = unsafe { cross_rp2040_w5500::unique_flash_id() };
        let mac = cotton_unique::mac_address(&unique_id, b"w5500-spi0");
        defmt::println!("MAC address: {:x}", mac);

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

        let mut timer = hal::Timer::new(c.device.TIMER, &mut resets, &clocks);
        let mono = Systick::new(c.core.SYST, clocks.system_clock.freq().raw());

        let sio = hal::Sio::new(c.device.SIO);
        let pins = rp_pico::Pins::new(
            c.device.IO_BANK0,
            c.device.PADS_BANK0,
            sio.gpio_bank0,
            &mut resets,
        );

        // W5500-EVB-Pico:
        //   W5500 SPI on SPI0
        //         nCS = GPIO17
        //         TX (MOSI) = GPIO19
        //         RX (MISO) = GPIO16
        //         SCK = GPIO18
        //   W5500 INTn on GPIO21
        //   W5500 RSTn on GPIO20
        //   Green LED on GPIO25

        let mut w5500_rst = pins
            .gpio20
            .into_pull_type::<PullNone>()
            .into_push_pull_output_in_state(PinState::Low);
        timer.delay_ms(2);
        let _ = w5500_rst.set_high();
        timer.delay_ms(2);

        let spi_ncs = pins
            .gpio17
            .into_pull_type::<PullNone>()
            .into_push_pull_output();
        let spi_mosi = pins
            .gpio19
            .into_pull_type::<PullNone>()
            .into_function::<hal::gpio::FunctionSpi>();
        let spi_miso = pins
            .gpio16
            .into_pull_type::<PullDown>()
            .into_function::<hal::gpio::FunctionSpi>();
        let spi_sclk = pins
            .gpio18
            .into_pull_type::<PullNone>()
            .into_function::<hal::gpio::FunctionSpi>();
        let spi = hal::spi::Spi::<_, _, _, 8>::new(
            c.device.SPI0,
            (spi_mosi, spi_miso, spi_sclk),
        );

        let spi = spi.init(
            &mut resets,
            clocks.peripheral_clock.freq(),
            16u32.MHz(),
            hal::spi::FrameFormat::MotorolaSpi(embedded_hal::spi::MODE_0),
        );

        let bus = w5500::bus::FourWire::new(spi, spi_ncs);

        let w5500_irq = pins.gpio21.into_pull_up_input();
        w5500_irq.set_interrupt_enabled(EdgeLow, true);

        unsafe {
            pac::NVIC::unmask(pac::Interrupt::IO_IRQ_BANK0);
        }

        let mut device = cotton_w5500::smoltcp::Device::new(bus, &mac);

        device.enable_interrupt();

        let mut stack = Stack::new(
            &mut device,
            &unique_id,
            &mac,
            &mut c.local.storage.sockets[..],
            now_fn(),
        );
        stack.poll(now_fn(), &mut device);

        periodic::spawn_after(2.secs()).unwrap();

        (
            Shared {},
            Local {
                device,
                stack,
                nvic: c.core.NVIC,
                w5500_irq,
            },
            init::Monotonics(mono),
        )
    }

    #[task(local = [nvic])]
    fn periodic(_cx: periodic::Context) {
        cortex_m::peripheral::NVIC::pend(pac::Interrupt::IO_IRQ_BANK0);
        periodic::spawn_after(2.secs()).unwrap();
    }

    #[task(binds = IO_IRQ_BANK0, local = [w5500_irq, device, stack], priority = 2)]
    fn eth_interrupt(cx: eth_interrupt::Context) {
        let w5500_irq = cx.local.w5500_irq;
        cx.local.device.clear_interrupt();
        w5500_irq.clear_interrupt(EdgeLow);
        cx.local.stack.poll(now_fn(), cx.local.device);
    }
}

/// All storage required for networking
struct NetworkStorage {
    sockets: [iface::SocketStorage<'static>; 2],
}

impl NetworkStorage {
    const fn new() -> Self {
        NetworkStorage {
            sockets: [SocketStorage::EMPTY; 2],
        }
    }
}

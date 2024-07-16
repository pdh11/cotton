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
    use fugit::ExtU64;
    use rp2040_hal::gpio::bank0::Gpio21;
    use rp2040_hal::gpio::FunctionSio;
    use rp2040_hal::gpio::Interrupt::EdgeLow;
    use rp2040_hal::gpio::PullUp;
    use rp2040_hal::gpio::SioInput;
    use rp_pico::pac;
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
        let mut setup =
            cross_rp2040_w5500::setup::BasicSetup::new(c.device, c.core.SYST);
        defmt::println!("MAC address: {:x}", setup.mac_address);

        let (spi_device, w5500_irq) = cross_rp2040_w5500::setup::spi_setup(
            setup.pins,
            setup.spi0,
            &mut setup.timer,
            &setup.clocks,
            &mut setup.resets,
        );

        let bus = w5500::bus::FourWire::new(spi_device);
        w5500_irq.set_interrupt_enabled(EdgeLow, true);
        unsafe {
            pac::NVIC::unmask(pac::Interrupt::IO_IRQ_BANK0);
        }

        let mut device =
            cotton_w5500::smoltcp::Device::new(bus, &setup.mac_address);

        device.enable_interrupt();

        let mut stack = Stack::new(
            &mut device,
            &setup.unique_id,
            &setup.mac_address,
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
            init::Monotonics(setup.mono),
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

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
    use fugit::ExtU64;
    use rp2040_hal::gpio::bank0::Gpio21;
    use rp2040_hal::gpio::FunctionSio;
    use rp2040_hal::gpio::Interrupt::EdgeLow;
    use rp2040_hal::gpio::PullUp;
    use rp2040_hal::gpio::SioInput;
    use rp2040_hal::Clock;
    use rp_pico::pac;
    use systick_monotonic::Systick;
    use embedded_hal::delay::DelayNs;

#[inline(never)]
unsafe fn unique_flash_id() -> cotton_unique::UniqueId {
    let mut unique_bytes = [0u8; 16];
    cortex_m::interrupt::free(|_| {
        rp2040_flash::flash::flash_unique_id(&mut unique_bytes, true);
    });
    cotton_unique::UniqueId::new(&unique_bytes)
}
    
    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        regs: pac::USBCTRL_REGS,
        nvic: cortex_m::peripheral::NVIC,
    }

    #[monotonic(binds = SysTick, default = true)]
    type Monotonic = Systick<1000>;

    fn now_fn() -> smoltcp::time::Instant {
        let time = monotonics::now().duration_since_epoch().ticks();
        smoltcp::time::Instant::from_millis(time as i64)
    }

    #[init(local = [])]
    fn init(c: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::println!(
            "{} from {} {}-g{}",
            env!("CARGO_BIN_NAME"),
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            git_version::git_version!()
        );

        let unique_id = unsafe { unique_flash_id() };

        let device = c.device;
        let mut resets = device.RESETS;
        let mut watchdog =
            rp2040_hal::watchdog::Watchdog::new(device.WATCHDOG);

        let clocks = rp2040_hal::clocks::init_clocks_and_plls(
            rp_pico::XOSC_CRYSTAL_FREQ,
            device.XOSC,
            device.CLOCKS,
            device.PLL_SYS,
            device.PLL_USB,
            &mut resets,
            &mut watchdog,
        )
        .ok()
        .unwrap();

        let mono = systick_monotonic::Systick::new(
            c.core.SYST,
            clocks.system_clock.freq().raw(),
        );
        let mut timer = rp2040_hal::Timer::new(device.TIMER, &mut resets, &clocks);

        // The timer doesn't increment if either RP2040 core is under
        // debug, unless the DBGPAUSE bits are cleared, which they
        // aren't by default.
        //
        // There is no neat and tidy method on hal::Timer to clear
        // these bits, and they can't be cleared before
        // hal::Timer::new because it resets the peripheral. So we
        // have to steal the peripheral, but that's OK because we only
        // access the DBGPAUSE register, which nobody else is
        // accessing.
        unsafe {
            rp2040_hal::pac::TIMER::steal()
                .dbgpause()
                .write(|w| w.bits(0));
        }
        let sio = rp2040_hal::Sio::new(device.SIO);
        let pins = rp_pico::Pins::new(
            device.IO_BANK0,
            device.PADS_BANK0,
            sio.gpio_bank0,
            &mut resets,
        );

        let regs = device.USBCTRL_REGS;
        let dpram = device.USBCTRL_DPRAM;

        resets.reset().modify(|_, w| {
            w.usbctrl().set_bit()
        });
        resets.reset().modify(|_, w| {
            w.usbctrl().clear_bit()
        });

        regs.usb_muxing().modify(|_, w| {
            w.to_phy().set_bit();
            w.softcon().set_bit()
        });
        regs.usb_pwr().modify(|_, w| {
                w.vbus_detect().set_bit();
                w.vbus_detect_override_en().set_bit()
            });
        regs.main_ctrl().modify(|_, w| {
            w.sim_timing().clear_bit();
            w.host_ndevice().set_bit();
            w.controller_en().set_bit()
        });
        regs.sie_ctrl().write(|w| {
            w.reset_bus().set_bit()
        });

        timer.delay_ms(50);
        regs.sie_ctrl().write(|w| {
            w.pulldown_en().set_bit();
            w.vbus_en().set_bit();
            w.keep_alive_en().set_bit();
            w.sof_en().set_bit()
        });

        loop {
            let status = regs.sie_status().read();
            defmt::println!("sie_status=0x{:x}", status.bits());
            match status.speed().bits() {
                1 => {
                    defmt::println!("LS detected");
                    break;
                }
                2 => {
                    defmt::println!("FS detected");
                    break;
                }
                _ => {}
            }
            timer.delay_ms(250);
        }

        // set up EPx and EPx buffer control
        // write setup packet
        // start transaction

        periodic::spawn_after(2.secs()).unwrap();

        (
            Shared {},
            Local {
                regs,
                /*
                device,
                stack, */
                nvic: c.core.NVIC,
                //w5500_irq,
            },
            init::Monotonics(mono),
        )
    }

    #[task(local = [regs, nvic])]
    fn periodic(cx: periodic::Context) {
        defmt::println!("sie_status=0x{:x}", cx.local.regs.sie_status().read().bits());
        periodic::spawn_after(2.secs()).unwrap();
    }

    #[task(binds = IO_IRQ_BANK0, local = [], priority = 2)]
    fn eth_interrupt(_cx: eth_interrupt::Context) {
        /*
        let w5500_irq = cx.local.w5500_irq;
        cx.local.device.clear_interrupt();
        w5500_irq.clear_interrupt(EdgeLow);
        cx.local.stack.poll(now_fn(), cx.local.device);
*/
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

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use rp2040_hal::fugit::RateExtU32;
use rp2040_hal::gpio::FunctionSpi;
use rp2040_hal::gpio::PinState;
use rp2040_hal::gpio::PullDown;
use rp2040_hal::gpio::PullNone;
use rp2040_hal::Clock;

/// Construct a UniqueId for RP2040 from the SPI flash unique ID
///
/// The RP2040 itself does not contain a unique chip identifier.
/// But RP2040-based designs typically incorporate a SPI flash
/// chip which *does* contain a unique chip identifier, which is
/// what is used here.
///
/// Note that not all SPI flash chips have this feature. The
/// Winbond parts commonly seen on RP2040 devboards
/// (JEDEC=0xEF7015) support an 8-byte unique ID;
/// <https://forums.raspberrypi.com/viewtopic.php?t=331949> suggests
/// that LCSC (Zetta) parts have a 16-byte unique ID (which is
/// *not* unique in just its first 8 bytes), JEDEC=0xBA6015.
/// Macronix and Spansion parts do not have a unique ID.
///
/// # Safety
///
/// Must be run on RP2040 as it calls a RP2040-specific function in
/// rp2040-flash. Also, no other flash access can be happening
/// concurrently (e.g. in other threads); it is recommended to call
/// this once during early startup and then pass the result around as
/// needed.
#[inline(never)]
unsafe fn unique_flash_id() -> cotton_unique::UniqueId {
    let mut unique_bytes = [0u8; 16];
    cortex_m::interrupt::free(|_| {
        rp2040_flash::flash::flash_unique_id(&mut unique_bytes, true);
    });
    cotton_unique::UniqueId::new(&unique_bytes)
}

pub struct BasicSetup {
    pub unique_id: cotton_unique::UniqueId,
    pub mac_address: [u8; 6],
    pub timer: rp2040_hal::Timer,
    pub pins: rp_pico::Pins,
    pub clocks: rp2040_hal::clocks::ClocksManager,
    pub mono: systick_monotonic::Systick<1000>,
    pub resets: rp2040_hal::pac::RESETS,
    pub spi0: rp2040_hal::pac::SPI0,
}

impl BasicSetup {
    pub fn new(
        device: rp2040_hal::pac::Peripherals,
        syst: rp2040_hal::pac::SYST,
    ) -> BasicSetup {
        let unique_id = unsafe { unique_flash_id() };
        let mac_address =
            cotton_unique::mac_address(&unique_id, b"w5500-spi0");

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
            syst,
            clocks.system_clock.freq().raw(),
        );
        let timer = rp2040_hal::Timer::new(device.TIMER, &mut resets, &clocks);

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

        BasicSetup {
            unique_id,
            mac_address,
            timer,
            pins,
            clocks,
            mono,
            resets,
            spi0: device.SPI0,
        }
    }
}

pub fn spi_setup(
    pins: rp_pico::Pins,
    spi0: rp2040_hal::pac::SPI0,
    timer: &mut rp2040_hal::Timer,
    clocks: &rp2040_hal::clocks::ClocksManager,
    resets: &mut rp2040_hal::pac::RESETS,
) -> (
    cotton_w5500::smoltcp::w5500_evb_pico::SpiDevice,
    cotton_w5500::smoltcp::w5500_evb_pico::IrqPin,
) {
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
        .into_function::<FunctionSpi>();
    let spi_miso = pins
        .gpio16
        .into_pull_type::<PullDown>()
        .into_function::<FunctionSpi>();
    let spi_sclk = pins
        .gpio18
        .into_pull_type::<PullNone>()
        .into_function::<FunctionSpi>();
    let spi = rp2040_hal::spi::Spi::<_, _, _, 8>::new(
        spi0,
        (spi_mosi, spi_miso, spi_sclk),
    );

    let spi_bus = spi.init(
        resets,
        clocks.peripheral_clock.freq(),
        16u32.MHz(),
        rp2040_hal::spi::FrameFormat::MotorolaSpi(embedded_hal::spi::MODE_0),
    );

    let irq_pin = pins.gpio21.into_pull_up_input();

    (
        embedded_hal_bus::spi::ExclusiveDevice::new_no_delay(spi_bus, spi_ncs),
        irq_pin,
    )
}

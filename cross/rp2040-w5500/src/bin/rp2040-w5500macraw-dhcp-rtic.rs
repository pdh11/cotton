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

struct Buffer {
    in_use: bool,
    bytes: [u8; 1536],
}

impl Buffer {
    pub const fn new() -> Self {
        Buffer {
            in_use: false,
            bytes: [0u8; 1536],
        }
    }
}

struct RawDevice<Spi: w5500::bus::Bus> {
    w5500: w5500::raw_device::RawDevice<Spi>,
    rx: Buffer,
    tx: Buffer,
}

struct EthTxToken<'a, Spi: w5500::bus::Bus> {
    w5500: &'a mut w5500::raw_device::RawDevice<Spi>,
    buffer: &'a mut Buffer,
}

struct EthRxToken<'a> {
    count: usize,
    buffer: &'a mut Buffer,
}

impl<Spi: w5500::bus::Bus> smoltcp::phy::Device for RawDevice<Spi> {
    type RxToken<'token> = EthRxToken<'token> where Self: 'token;
    type TxToken<'token> = EthTxToken<'token, Spi> where Self: 'token;

    fn receive(
        &mut self,
        _timestamp: smoltcp::time::Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if !self.tx.in_use && !self.rx.in_use {
            if let Ok(n) = self.w5500.read_frame(&mut self.rx.bytes) {
                if n > 0 {
                    self.rx.in_use = true;
                    self.tx.in_use = true;
                    return Some((
                        EthRxToken {
                            count: n,
                            buffer: &mut self.rx,
                        },
                        EthTxToken {
                            w5500: &mut self.w5500,
                            buffer: &mut self.tx,
                        },
                    ));
                }
            }
        }
        None
    }

    fn transmit(
        &mut self,
        _timestamp: smoltcp::time::Instant,
    ) -> Option<Self::TxToken<'_>> {
        if !self.tx.in_use {
            self.tx.in_use = true;
            Some(EthTxToken {
                w5500: &mut self.w5500,
                buffer: &mut self.tx,
            })
        } else {
            defmt::println!("TX denied");
            None
        }
    }

    fn capabilities(&self) -> smoltcp::phy::DeviceCapabilities {
        let mut caps = smoltcp::phy::DeviceCapabilities::default();
        caps.max_transmission_unit = 1536;
        caps.medium = smoltcp::phy::Medium::Ethernet;
        caps.max_burst_size = Some(1);
        caps
    }
}

impl<'a> smoltcp::phy::RxToken for EthRxToken<'a> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        defmt::println!("RX2 {}", self.count);
        let result = f(&mut self.buffer.bytes[0..self.count]);
        self.buffer.in_use = false;
        result
    }
}

impl<'a> Drop for EthRxToken<'a> {
    fn drop(&mut self) {
        if self.buffer.in_use {
            defmt::println!("Dropping unconsumed RX");
        }
        self.buffer.in_use = false;
    }
}

impl<'a, Spi: w5500::bus::Bus> smoltcp::phy::TxToken for EthTxToken<'a, Spi> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let result = f(&mut self.buffer.bytes[0..len]);
        match self.w5500.write_frame(&self.buffer.bytes[0..len]) {
            Ok(_) => defmt::println!("TX {} OK", len),
            _ => defmt::println!("TX not OK"),
        }
        self.buffer.in_use = false;
        result
    }
}

impl<'a, Spi: w5500::bus::Bus> Drop for EthTxToken<'a, Spi> {
    fn drop(&mut self) {
        if self.buffer.in_use {
            defmt::println!("Dropping unconsumed TX");
        }
        self.buffer.in_use = false;
    }
}

#[rtic::app(device = rp_pico::hal::pac, peripherals = true, dispatchers = [ADC_IRQ_FIFO])]
mod app {
    use crate::Buffer;
    use crate::NetworkStorage;
    use crate::RawDevice;
    use cross_rp2040_w5500::{smoltcp::Stack, unique};
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

    #[shared]
    struct Shared {}

    type MySpi0 = rp2040_hal::Spi<
        rp2040_hal::spi::Enabled,
        SPI0,
        (
            rp2040_hal::gpio::Pin<Gpio19, FunctionSpi, PullDown>,
            rp2040_hal::gpio::Pin<Gpio16, FunctionSpi, PullDown>,
            rp2040_hal::gpio::Pin<Gpio18, FunctionSpi, PullDown>,
        ),
    >;

    type MySpiChipSelect =
        rp2040_hal::gpio::Pin<Gpio17, FunctionSio<SioOutput>, PullDown>;

    type MySpiBus = w5500::bus::FourWire<MySpi0, MySpiChipSelect>;

    #[local]
    struct Local {
        device: RawDevice<MySpiBus>,
        stack: Stack,
        nvic: cortex_m::peripheral::NVIC,
        w5500_irq:
            rp2040_hal::gpio::Pin<Gpio21, FunctionSio<SioInput>, PullNone>,
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
        let unique_id = unsafe { unique::unique_flash_id() };
        let mac = cotton_unique::mac_address(&unique_id, b"w5500-spi0");
        defmt::println!("MAC address: {:x}", mac);

        let w5500_mac = w5500::net::MacAddress {
            octets: mac.clone(),
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

        // W5500-EVB-Pico:
        //   W5500 SPI on SPI0
        //         nCS = GPIO17
        //         TX (MOSI) = GPIO19
        //         RX (MISO) = GPIO16
        //         SCK = GPIO18
        //   W5500 INTn on GPIO21
        //   W5500 RSTn on GPIO20
        //   Green LED on GPIO25

        let spi_ncs = pins.gpio17.into_push_pull_output();
        let spi_mosi = pins.gpio19.into_function::<hal::gpio::FunctionSpi>();
        let spi_miso = pins.gpio16.into_function::<hal::gpio::FunctionSpi>();
        let spi_sclk = pins.gpio18.into_function::<hal::gpio::FunctionSpi>();
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
        let w5500 = w5500::UninitializedDevice::new(bus)
            .initialize_macraw(w5500_mac)
            .unwrap();

        let w5500_irq = pins.gpio21.into_floating_input();
        w5500_irq.set_interrupt_enabled(EdgeLow, true);

        unsafe {
            pac::NVIC::unmask(pac::Interrupt::IO_IRQ_BANK0);
        }

        let mut device = super::RawDevice {
            w5500,
            rx: Buffer::new(),
            tx: Buffer::new(),
        };

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
                device: device,
                stack: stack,
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
        w5500_irq.clear_interrupt(EdgeLow);
        defmt::println!("ETH IRQ");
        cx.local.stack.poll(now_fn(), cx.local.device);
    }
}

/// All storage required for networking
pub struct NetworkStorage {
    pub sockets: [iface::SocketStorage<'static>; 2],
}

impl NetworkStorage {
    pub const fn new() -> Self {
        NetworkStorage {
            sockets: [SocketStorage::EMPTY; 2],
        }
    }
}

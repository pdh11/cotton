struct Buffer {
    bytes: [u8; 1536],
}

impl Buffer {
    pub const fn new() -> Self {
        Buffer { bytes: [0u8; 1536] }
    }
}

/// A W5500 driver for smoltcp
///
/// Implementing `smoltcp::phy::Device`.
///
/// This is a simple implementation that uses synchronous transfers, and
/// so needs just one inbound and one outbound buffer.
pub struct Device<Spi: w5500::bus::Bus> {
    w5500: w5500::raw_device::RawDevice<Spi>,
    rx: Buffer,
    tx: Buffer,
}

impl<Spi: w5500::bus::Bus> Device<Spi> {
    /// Create a new Device from a SPI abstraction and a MAC address
    ///
    /// See the `w5500` crate for what constitutes a SPI abstraction (it's
    /// not just `embedded_hal::SpiDevice`). See the `cotton_unique` crate
    /// for a good way to derive MAC addresses (or, for testing purposes,
    /// just make one up if need be).
    pub fn new(spi: Spi, mac_address: &[u8; 6]) -> Self {
        let mac = w5500::net::MacAddress {
            octets: *mac_address,
        };
        Self {
            w5500: w5500::UninitializedDevice::new(spi)
                .initialize_macraw(mac)
                .unwrap(),
            rx: Buffer::new(),
            tx: Buffer::new(),
        }
    }

    /// Enable chip-level interrupts on pin INTn
    pub fn enable_interrupt(&mut self) {
        let _ = self.w5500.enable_interrupts(4); // RX interrupt
    }

    /// Clear pending interrupts
    pub fn clear_interrupt(&mut self) {
        let _ = self.w5500.clear_interrupts();
    }
}

/// An `EthTxToken` represents permission to send one packet
///
/// The packet is not sent until the `consume` method is called on the
/// token. Because both the SPI transfer and the Ethernet transmission
/// are synchronous, consuming the token (assuming 10MHz SPI and 10Mbit
/// Ethernet) may take up to 1500*8*2*0.1us or 2.4ms.
pub struct EthTxToken<'a, Spi: w5500::bus::Bus> {
    w5500: &'a mut w5500::raw_device::RawDevice<Spi>,
    buffer: &'a mut Buffer,
}

/// An `EthRxToken` represents permission to receive one packet
///
/// The packet is copied from SPI in the `receive` call; consuming the
/// token does no further copies.
pub struct EthRxToken<'a> {
    count: usize,
    buffer: &'a mut Buffer,
}

impl<Spi: w5500::bus::Bus> smoltcp::phy::Device for Device<Spi> {
    type RxToken<'token> = EthRxToken<'token> where Self: 'token;
    type TxToken<'token> = EthTxToken<'token, Spi> where Self: 'token;

    fn receive(
        &mut self,
        _timestamp: smoltcp::time::Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if let Ok(n) = self.w5500.read_frame(&mut self.rx.bytes) {
            if n > 0 {
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
        None
    }

    fn transmit(
        &mut self,
        _timestamp: smoltcp::time::Instant,
    ) -> Option<Self::TxToken<'_>> {
        // Because it returns a mutable reference, this cannot be
        // called again until the previous token has been Dropped,
        // so there's no need to reference-count the buffer.
        Some(EthTxToken {
            w5500: &mut self.w5500,
            buffer: &mut self.tx,
        })
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
        f(&mut self.buffer.bytes[0..self.count])
    }
}

impl<'a, Spi: w5500::bus::Bus> smoltcp::phy::TxToken for EthTxToken<'a, Spi> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let result = f(&mut self.buffer.bytes[0..len]);
        let _ = self.w5500.write_frame(&self.buffer.bytes[0..len]);
        result
    }
}

/// For W5500-EVB-Pico
#[cfg(feature = "w5500-evb-pico")]
pub mod w5500_evb_pico {
    use rp2040_hal::gpio::bank0::Gpio16;
    use rp2040_hal::gpio::bank0::Gpio17;
    use rp2040_hal::gpio::bank0::Gpio18;
    use rp2040_hal::gpio::bank0::Gpio19;
    use rp2040_hal::gpio::FunctionSio;
    use rp2040_hal::gpio::FunctionSpi;
    use rp2040_hal::gpio::PullDown;
    use rp2040_hal::gpio::PullNone;
    use rp2040_hal::gpio::SioOutput;
    use rp2040_hal::pac::SPI0;
    use embedded_hal_bus::spi::ExclusiveDevice;
    use embedded_hal_bus::spi::NoDelay;

    type Spi0 = rp2040_hal::Spi<
        rp2040_hal::spi::Enabled,
        SPI0,
        (
            rp2040_hal::gpio::Pin<Gpio19, FunctionSpi, PullNone>, // TX
            rp2040_hal::gpio::Pin<Gpio16, FunctionSpi, PullDown>, // RX
            rp2040_hal::gpio::Pin<Gpio18, FunctionSpi, PullNone>, // SCK
        ),
    >;

    type SpiChipSelect =
        rp2040_hal::gpio::Pin<Gpio17, FunctionSio<SioOutput>, PullNone>;

    type SpiDevice = ExclusiveDevice<Spi0, SpiChipSelect, NoDelay>;

    type SpiBus = w5500::bus::FourWire<SpiDevice>;

    /// A W5500 driver specialised for the SPI setup on the W5500-EVB-Pico board
    pub type Device = super::Device<SpiBus>;
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use mockall::mock;
    use smoltcp::phy::{Device, RxToken, TxToken};

    mock! {
        Bus {}
        impl w5500::bus::Bus for Bus {
            type Error = u32;

            fn read_frame(&mut self, block: u8, address: u16, data: &mut [u8]) -> Result<(), u32>;

            fn write_frame(&mut self, block: u8, address: u16, data: &[u8]) -> Result<(), u32>;
        }
    }

    const SETUP_CALLS: usize = 20;

    #[test]
    fn test_instantiate() {
        let mut bus = MockBus::new();
        // We don't test the setup calls, that's the w5500 crate's business
        bus.expect_write_frame()
            .times(SETUP_CALLS)
            .return_const(Ok(()));
        let _device = super::Device::new(bus, &[0x88u8; 6]);
    }

    #[test]
    fn test_capabilities() {
        let mut bus = MockBus::new();
        bus.expect_write_frame()
            .times(SETUP_CALLS)
            .return_const(Ok(()));
        let device = super::Device::new(bus, &[0x88u8; 6]);
        let c = device.capabilities();
        assert_eq!(smoltcp::phy::Medium::Ethernet, c.medium);
        assert_eq!(Some(1), c.max_burst_size);
        assert_eq!(1536, c.max_transmission_unit);
    }

    #[test]
    fn test_transmit() {
        let mut bus = MockBus::new();
        bus.expect_write_frame()
            .times(SETUP_CALLS)
            .return_const(Ok(())); // i.e. no 21st time in transmit
        let mut device = super::Device::new(bus, &[0x88u8; 6]);

        let res = device.transmit(smoltcp::time::Instant::ZERO);
        assert!(res.is_some());
    }

    #[test]
    fn test_transmit_consume() {
        let mut bus = MockBus::new();
        bus.expect_write_frame()
            .times(SETUP_CALLS)
            .return_const(Ok(()));
        // It reads the free size
        bus.expect_read_frame()
            .withf(|block, addr, _data| *block == 1 && *addr == 0x20)
            .returning(|_block, _addr, data: &mut [u8]| {
                data[0] = 64;
                data[1] = 0;
                Ok(())
            });
        // It reads the cursor
        bus.expect_read_frame()
            .withf(|block, addr, _data| *block == 1 && *addr == 0x24)
            .returning(|_block, _addr, data: &mut [u8]| {
                data[0] = 0;
                data[1] = 0;
                Ok(())
            });
        // It writes the frame
        bus.expect_write_frame()
            .withf(|_block, _addr, data| data[0] == b'O' && data[1] == b'K')
            .return_const(Ok(()));
        // Several further writes (the cursor, clearing SN_IR, start TX)
        bus.expect_write_frame().return_const(Ok(()));
        // It reads SN_IR
        bus.expect_read_frame()
            .withf(|block, addr, _data| *block == 1 && *addr == 2)
            .returning(|_block, _addr, data: &mut [u8]| {
                data[0] = 16;
                Ok(())
            });
        let mut device = super::Device::new(bus, &[0x88u8; 6]);

        let res = device.transmit(smoltcp::time::Instant::ZERO);
        res.unwrap().consume(2, |buf| {
            buf[0] = b'O';
            buf[1] = b'K';
        });
    }

    #[test]
    fn test_receive_not_ready() {
        let mut bus = MockBus::new();
        bus.expect_write_frame()
            .times(SETUP_CALLS)
            .return_const(Ok(()));
        // It reads the RX cursor
        bus.expect_read_frame()
            .withf(|block, _addr, _data| *block == 1)
            .returning(|_block, _addr, data| {
                data[0] = 0;
                data[1] = 0;
                Ok(())
            });
        let mut device = super::Device::new(bus, &[0x88u8; 6]);

        let res = device.receive(smoltcp::time::Instant::ZERO);
        assert!(res.is_none());
    }

    #[test]
    fn test_receive() {
        let mut bus = MockBus::new();
        bus.expect_write_frame()
            .times(SETUP_CALLS)
            .return_const(Ok(()));
        // It reads the RX-in-use (2 bytes size + 2 bytes frame)
        bus.expect_read_frame()
            .withf(|block, addr, _data| *block == 1 && *addr == 0x26)
            .returning(|_block, _addr, data| {
                data[0] = 0;
                data[1] = 4;
                Ok(())
            });
        // It reads the RX cursor (0)
        bus.expect_read_frame()
            .withf(|block, addr, _data| *block == 1 && *addr == 0x28)
            .returning(|_block, _addr, data| {
                data[0] = 0;
                data[1] = 0;
                Ok(())
            });
        // It reads the frame size (including header, so 4)
        bus.expect_read_frame()
            .withf(|block, addr, _data| *block == 3 && *addr == 0)
            .returning(|_block, _addr, data| {
                data[0] = 0;
                data[1] = 4;
                Ok(())
            });
        // It reads the frame itself
        bus.expect_read_frame()
            .withf(|block, addr, _data| *block == 3 && *addr == 2)
            .returning(|_block, _addr, data| {
                data[0] = b'r';
                data[1] = b'x';
                Ok(())
            });
        // Several writes (the cursor etc)
        bus.expect_write_frame().return_const(Ok(()));
        let mut device = super::Device::new(bus, &[0x88u8; 6]);

        let (rx, _tx) = device.receive(smoltcp::time::Instant::ZERO).unwrap();
        rx.consume(|b| {
            assert_eq!(b.len(), 2);
            assert_eq!(b[0], b'r');
            assert_eq!(b[1], b'x');
        });
    }

    #[test]
    fn test_receive_propagates_error() {
        let mut bus = MockBus::new();
        bus.expect_write_frame()
            .times(SETUP_CALLS)
            .return_const(Ok(()));

        // It reads the RX-in-use (2 bytes size + 2 bytes frame)
        bus.expect_read_frame().returning(|_, _, _| Err(1u32));
        let mut device = super::Device::new(bus, &[0x88u8; 6]);

        let res = device.receive(smoltcp::time::Instant::ZERO);
        assert!(res.is_none());
    }

    #[test]
    fn test_enable_interrupt() {
        let mut bus = MockBus::new();
        bus.expect_write_frame()
            .times(SETUP_CALLS)
            .return_const(Ok(()));

        // S0_IMR
        bus.expect_write_frame()
            .withf(|block, addr, data| {
                *block == 1 && *addr == 0x2C && data[0] == 4
            })
            .return_const(Ok(()));
        // SIMR
        bus.expect_write_frame()
            .withf(|block, addr, data| {
                *block == 0 && *addr == 0x18 && data[0] == 1
            })
            .return_const(Ok(()));
        let mut device = super::Device::new(bus, &[0x88u8; 6]);
        device.enable_interrupt();
    }

    #[test]
    fn test_clear_interrupt() {
        let mut bus = MockBus::new();
        bus.expect_write_frame()
            .times(SETUP_CALLS)
            .return_const(Ok(()));

        // S0_IR
        bus.expect_write_frame()
            .withf(|block, addr, data| {
                *block == 1 && *addr == 2 && data[0] == 0xFF
            })
            .return_const(Ok(()));
        let mut device = super::Device::new(bus, &[0x88u8; 6]);
        device.clear_interrupt();
    }
}

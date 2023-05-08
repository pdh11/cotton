use core::hash::Hasher;
use fugit::RateExtU32;
use hal::gpio::GpioExt;
use ieee802_3_miim::{phy::PhySpeed, Phy};
use smoltcp::{socket::dhcpv4, wire::IpCidr};
use stm32_eth::hal::rcc::Clocks;
use stm32_eth::hal::rcc::RccExt;
use stm32f7xx_hal as hal;

/// Set up the STM32 clocks for normal operation
///
/// The STM32 boots with HSI enabled, running the code at 16MHz. This
/// function ups that to 100MHz. STM32F746 can go up to 180MHz (normal
/// mode) or 216MHz (overdrive mode), see RM0385 rev5 s3.3.2.
///
/// The stm32f7xx-hal crate takes care of setting FLASH->ACR, see
/// `https://github.com/stm32-rs/stm32f7xx-hal/blob/main/src/rcc.rs`
#[must_use]
pub fn setup_clocks(rcc: stm32_eth::stm32::RCC) -> Clocks {
    let rcc = rcc.constrain();
    rcc.cfgr.sysclk(100.MHz()).hclk(100.MHz()).freeze()
}

/// Returns the unique chip ID of this particular STM32
///
/// See RM0385 rev5 s41.1.
///
/// This function is not safe on non-STM32 platforms, as it reads from
/// a fixed physical memory location.
#[must_use]
pub fn stm32_unique_id() -> &'static [u32; 3] {
    unsafe {
        let ptr = 0x1ff0_f420 as *const [u32; 3];
        &*ptr
    }
}

/// Returns a salted unique ID based on the chip ID
///
/// Chip IDs are unique, but predictable. This function calculates a
/// new unique ID with a particular salt value. The result is
/// deterministic and consistent on any one STM32 device for a
/// particular salt, but varies from one device to another (and from
/// one salt to another).
///
/// For instance, this is used by `mac_address()`.
#[must_use]
pub fn unique_id(salt: &[u8]) -> u64 {
    let id = stm32_unique_id();
    let key1 = (u64::from(id[0]) << 32) + u64::from(id[1]);
    let key2 = u64::from(id[2]);
    let mut h = siphasher::sip::SipHasher::new_with_keys(key1, key2);
    h.write(salt);
    h.finish()
}

/// Return a (statistically) unique Ethernet MAC address for this device
///
/// Statistical uniqueness comes from a salted hash of the unique chip ID.
#[must_use]
pub fn mac_address() -> [u8; 6] {
    let mut mac_address = [0u8; 6];
    let r = unique_id(b"stm32-eth-mac").to_ne_bytes();
    mac_address.copy_from_slice(&r[0..6]);
    mac_address[0] &= 0xFE; // clear multicast bit
    mac_address[0] |= 2; // set local bit

    defmt::println!(
        "Local MAC address: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac_address[0],
        mac_address[1],
        mac_address[2],
        mac_address[3],
        mac_address[4],
        mac_address[5]
    );

    mac_address
}

type MdioPa2 =
    hal::gpio::Pin<'A', 2, hal::gpio::Alternate<11, hal::gpio::PushPull>>;

type MdcPc1 =
    hal::gpio::Pin<'C', 1, hal::gpio::Alternate<11, hal::gpio::PushPull>>;

/// Encapsulate the stm32-eth Ethernet and PHY drivers
pub struct Stm32Ethernet {
    /// The actual driver struct (from `stm32-eth` crate)
    pub dma: stm32_eth::dma::EthernetDMA<'static, 'static>,
    phy: ieee802_3_miim::phy::LAN8742A<
        stm32_eth::mac::EthernetMACWithMii<MdioPa2, MdcPc1>,
    >,
    got_link: bool,
}

impl Stm32Ethernet {
    /// Construct an STM32 Ethernet (and PHY) driver from raw peripherals
    #[allow(clippy::too_many_arguments, clippy::similar_names)]
    pub fn new(
        gpioa: hal::pac::GPIOA,
        gpiob: hal::pac::GPIOB,
        gpioc: hal::pac::GPIOC,
        gpiog: hal::pac::GPIOG,
        dma: hal::pac::ETHERNET_DMA,
        mac: hal::pac::ETHERNET_MAC,
        mmc: hal::pac::ETHERNET_MMC,
        clocks: Clocks,
        rx_ring: &'static mut [stm32_eth::dma::RxRingEntry; 2],
        tx_ring: &'static mut [stm32_eth::dma::TxRingEntry; 2],
    ) -> Self {
        let gpioa = gpioa.split();
        let gpiob = gpiob.split();
        let gpioc = gpioc.split();
        let gpiog = gpiog.split();

        let stm32_eth::Parts { dma, mac } = stm32_eth::new_with_mii(
            stm32_eth::PartsIn { mac, mmc, dma },
            rx_ring,
            tx_ring,
            clocks,
            stm32_eth::EthPins {
                ref_clk: gpioa.pa1,
                crs: gpioa.pa7,
                tx_en: gpiog.pg11,
                tx_d0: gpiog.pg13,
                tx_d1: gpiob.pb13,
                rx_d0: gpioc.pc4,
                rx_d1: gpioc.pc5,
            },
            gpioa.pa2.into_alternate(), // mdio
            gpioc.pc1.into_alternate(), // mdc
        )
        .unwrap();

        defmt::println!("Enabling interrupts");
        dma.enable_interrupt();

        let mut phy = ieee802_3_miim::phy::LAN8742A::new(mac, 0);

        phy.phy_init();

        Stm32Ethernet {
            dma,
            phy,
            got_link: false,
        }
    }

    /// Poll the Ethernet PHY to determine whether link is established
    ///
    /// If it is (newly-) established, work out what Ethernet speed
    /// has been negotiated.
    pub fn link_established(&mut self) -> bool {
        use stm32_eth::mac::Speed;

        let got_link = self.phy.link_established();
        if got_link && !self.got_link {
            if let Some(speed) = self.phy.link_speed().map(|s| match s {
                PhySpeed::HalfDuplexBase10T => Speed::HalfDuplexBase10T,
                PhySpeed::FullDuplexBase10T => Speed::FullDuplexBase10T,
                PhySpeed::HalfDuplexBase100Tx => Speed::HalfDuplexBase100Tx,
                PhySpeed::FullDuplexBase100Tx => Speed::FullDuplexBase100Tx,
            }) {
                self.phy.get_miim().set_speed(speed);
                defmt::println!("Detected link speed: {}", speed);
            } else {
                defmt::warn!("Failed to detect link speed.");
            }
        }
        self.got_link = got_link;
        got_link
    }
}

/// A helper container for a TCP/IP stack and some of its metadata
pub struct Stack<'a> {
    /// The underlying Smoltcp implementation
    pub interface: smoltcp::iface::Interface,
    /// Persistent socket data for active sockets
    pub socket_set: smoltcp::iface::SocketSet<'a>,
    dhcp_handle: smoltcp::iface::SocketHandle,
}

impl<'a> Stack<'a> {
    /// Construct a new TCP Stack abstraction
    ///
    /// From an interface, a MAC address, and some storage for the
    /// socket metadata.
    pub fn new<D: smoltcp::phy::Device>(
        device: &mut D,
        mac_address: &[u8; 6],
        sockets: &'a mut [smoltcp::iface::SocketStorage<'a>],
    ) -> Stack<'a> {
        let mut config = smoltcp::iface::Config::new();
        config.random_seed = unique_id(b"smoltcp-config-random");
        config.hardware_addr = Some(
            smoltcp::wire::EthernetAddress::from_bytes(mac_address).into(),
        );
        let interface = smoltcp::iface::Interface::new(config, device);
        let mut socket_set = smoltcp::iface::SocketSet::new(sockets);

        let mut dhcp_socket = smoltcp::socket::dhcpv4::Socket::new();
        dhcp_socket.set_retry_config(smoltcp::socket::dhcpv4::RetryConfig {
            discover_timeout: smoltcp::time::Duration::from_secs(2),
            initial_request_timeout: smoltcp::time::Duration::from_millis(500),
            request_retries: 10,
            min_renew_timeout: smoltcp::time::Duration::from_secs(864_000),
        });
        let dhcp_handle = socket_set.add(dhcp_socket);

        Stack {
            interface,
            socket_set,
            dhcp_handle,
        }
    }

    /// Poll the interface for new packets, then the DHCP socket
    pub fn poll<D: smoltcp::phy::Device>(
        &mut self,
        now: smoltcp::time::Instant,
        device: &mut D,
    ) {
        self.interface.poll(now, device, &mut self.socket_set);
        self.poll_dhcp();
    }

    /// Poll the DHCP socket for any updates
    ///
    /// Smoltcp's `dhcpv4::Socket` takes care of retrying/rebinding
    fn poll_dhcp(&mut self) {
        let socket =
            self.socket_set.get_mut::<dhcpv4::Socket>(self.dhcp_handle);
        let event = socket.poll();
        match event {
            None => {}
            Some(dhcpv4::Event::Configured(config)) => {
                defmt::println!("DHCP config acquired!");
                defmt::println!("IP address:      {}", config.address);

                self.interface.update_ip_addrs(|addrs| {
                    addrs.clear();
                    addrs.push(IpCidr::Ipv4(config.address)).unwrap();
                });

                if let Some(router) = config.router {
                    self.interface
                        .routes_mut()
                        .add_default_ipv4_route(router)
                        .unwrap();
                } else {
                    self.interface.routes_mut().remove_default_ipv4_route();
                }
            }
            Some(dhcpv4::Event::Deconfigured) => {
                defmt::println!("DHCP lost config!");
                self.interface.update_ip_addrs(|addrs| {
                    addrs.clear();
                });
            }
        }
    }
}

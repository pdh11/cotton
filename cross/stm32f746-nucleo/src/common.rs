use core::ptr;
use cotton_unique::UniqueId;
use fugit::RateExtU32;
use hal::gpio::GpioExt;
use ieee802_3_miim::{phy::PhySpeed, Phy};
use linked_list_allocator::LockedHeap;
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
pub unsafe fn stm32_unique_id() -> UniqueId {
    let mut unique_bytes = [0u8; 16];
    ptr::copy(0x1ff0_f420 as *const u8, unique_bytes.as_mut_ptr(), 12);
    UniqueId::new(&unique_bytes)
}

type MdioPa2 =
    hal::gpio::Pin<'A', 2, hal::gpio::Alternate<11, hal::gpio::PushPull>>;

type MdcPc1 =
    hal::gpio::Pin<'C', 1, hal::gpio::Alternate<11, hal::gpio::PushPull>>;

/// The STM32 peripherals needed for Ethernet
///
/// The Ethernet itself, and the GPIO blocks whose pinmux needs setting.
pub struct Stm32EthernetPeripherals {
    gpioa: hal::pac::GPIOA,
    gpiob: hal::pac::GPIOB,
    gpioc: hal::pac::GPIOC,
    gpiog: hal::pac::GPIOG,
    ethernet_dma: hal::pac::ETHERNET_DMA,
    ethernet_mac: hal::pac::ETHERNET_MAC,
    ethernet_mmc: hal::pac::ETHERNET_MMC,
}

/// Split off the STM32 peripherals Ethernet needs
///
/// This is needed because everything passes the peripherals around by
/// value, i.e. taking ownership.
///
/// This plan won't suffice if any other part of the application needs
/// to share these peripherals (e.g. GPIOA), but none of our tests do so.
pub fn split_peripherals(
    device: stm32_eth::stm32::Peripherals,
) -> (Stm32EthernetPeripherals, hal::pac::RCC) {
    let stm32_eth::stm32::Peripherals {
        GPIOA,
        GPIOB,
        GPIOC,
        GPIOG,
        ETHERNET_DMA,
        ETHERNET_MAC,
        ETHERNET_MMC,
        RCC,
        ..
    } = device;

    (
        Stm32EthernetPeripherals {
            gpioa: GPIOA,
            gpiob: GPIOB,
            gpioc: GPIOC,
            gpiog: GPIOG,
            ethernet_dma: ETHERNET_DMA,
            ethernet_mac: ETHERNET_MAC,
            ethernet_mmc: ETHERNET_MMC,
        },
        RCC,
    )
}

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
    pub fn new(
        peripherals: Stm32EthernetPeripherals,
        clocks: Clocks,
        rx_ring: &'static mut [stm32_eth::dma::RxRingEntry; 2],
        tx_ring: &'static mut [stm32_eth::dma::TxRingEntry; 2],
    ) -> Self {
        let gpioa = peripherals.gpioa.split();
        let gpiob = peripherals.gpiob.split();
        let gpioc = peripherals.gpioc.split();
        let gpiog = peripherals.gpiog.split();

        let stm32_eth::Parts { dma, mac } = stm32_eth::new_with_mii(
            stm32_eth::PartsIn {
                mac: peripherals.ethernet_mac,
                mmc: peripherals.ethernet_mmc,
                dma: peripherals.ethernet_dma,
            },
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
        unique: &UniqueId,
        mac_address: &[u8; 6],
        sockets: &'a mut [smoltcp::iface::SocketStorage<'a>],
        now: smoltcp::time::Instant,
    ) -> Stack<'a> {
        let mut config = smoltcp::iface::Config::new(
            smoltcp::wire::EthernetAddress::from_bytes(mac_address).into(),
        );
        config.random_seed = unique.id(b"smoltcp-config-random");
        let interface = smoltcp::iface::Interface::new(config, device, now);
        let mut socket_set = smoltcp::iface::SocketSet::new(sockets);

        let mut dhcp_socket = smoltcp::socket::dhcpv4::Socket::new();
        let mut retry_config = smoltcp::socket::dhcpv4::RetryConfig::default();
        retry_config.discover_timeout = smoltcp::time::Duration::from_secs(2);
        retry_config.initial_request_timeout =
            smoltcp::time::Duration::from_millis(500);
        retry_config.request_retries = 10;
        dhcp_socket.set_retry_config(retry_config);
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

extern "C" {
    static mut __sheap: u32;
    static mut _stack_start: u32;
}

/// Set up the heap
///
/// As is standard, all memory above the rodata segment and below the
/// stack, is used as heap.
pub fn init_heap(allocator: &LockedHeap) {
    const STACK_SIZE: usize = 16 * 1024;
    // SAFETY: this relies on the link map being correct, and STACK_SIZE
    // being large enough for the entire program.
    unsafe {
        let heap_start = ptr::addr_of!(__sheap) as usize;
        let heap_end = ptr::addr_of!(_stack_start) as usize;
        let heap_size = heap_end - heap_start - STACK_SIZE;
        allocator.lock().init(heap_start as *mut u8, heap_size);
    }
}

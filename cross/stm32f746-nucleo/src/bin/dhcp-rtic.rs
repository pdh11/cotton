#![no_std]
#![no_main]

use defmt_rtt as _; // global logger
use panic_probe as _;
use stm32f7xx_hal as _;

use smoltcp::{
    iface::{self, SocketStorage},
    wire::{self, IpAddress, Ipv4Address},
};

use stm32_eth::{
    hal::{gpio::GpioExt, rcc::Clocks},
    PartsIn,
};

pub use pins::{setup_pins, Gpio};

use fugit::RateExtU32;
use stm32_eth::hal::rcc::RccExt;

/// Setup the clocks and return clocks and a GPIO struct that
/// can be used to set up all of the pins.
///
/// This configures HCLK to be at least 25 MHz, which is the minimum required
/// for ethernet operation to be valid.
pub fn setup_peripherals(
    p: stm32_eth::stm32::Peripherals,
) -> (Clocks, Gpio, PartsIn) {
    let ethernet = PartsIn {
        dma: p.ETHERNET_DMA,
        mac: p.ETHERNET_MAC,
        mmc: p.ETHERNET_MMC,
    };

    {
        let rcc = p.RCC.constrain();

        let clocks = rcc.cfgr.sysclk(100.MHz()).hclk(100.MHz());

        let clocks = {
            if cfg!(hse = "bypass") {
                clocks.hse(stm32_eth::hal::rcc::HSEClock::new(
                    8.MHz(),
                    stm32_eth::hal::rcc::HSEClockMode::Bypass,
                ))
            } else if cfg!(hse = "oscillator") {
                clocks.hse(stm32_eth::hal::rcc::HSEClock::new(
                    8.MHz(),
                    stm32_eth::hal::rcc::HSEClockMode::Oscillator,
                ))
            } else {
                clocks
            }
        };

        let clocks = clocks.freeze();

        let gpio = Gpio {
            gpioa: p.GPIOA.split(),
            gpiob: p.GPIOB.split(),
            gpioc: p.GPIOC.split(),
            gpiog: p.GPIOG.split(),
        };

        (clocks, gpio, ethernet)
    }
}

pub use pins::*;

mod pins {
    use stm32_eth::{hal::gpio::*, EthPins};

    pub struct Gpio {
        pub gpioa: gpioa::Parts,
        pub gpiob: gpiob::Parts,
        pub gpioc: gpioc::Parts,
        pub gpiog: gpiog::Parts,
    }

    pub type RefClk = PA1<Input>;
    pub type Crs = PA7<Input>;
    pub type TxD1 = PB13<Input>;
    pub type RxD0 = PC4<Input>;
    pub type RxD1 = PC5<Input>;

    pub type TxEn = PG11<Input>;
    pub type TxD0 = PG13<Input>;

    pub type Mdio = PA2<Alternate<11>>;
    pub type Mdc = PC1<Alternate<11>>;

    pub type Pps = PB5<Output<PushPull>>;

    pub fn setup_pins(
        gpio: Gpio,
    ) -> (
        EthPins<RefClk, Crs, TxEn, TxD0, TxD1, RxD0, RxD1>,
        Mdio,
        Mdc,
        Pps,
    ) {
        #[allow(unused_variables)]
        let Gpio {
            gpioa,
            gpiob,
            gpioc,
            gpiog,
        } = gpio;

        let ref_clk = gpioa.pa1.into_floating_input();
        let crs = gpioa.pa7.into_floating_input();
        let tx_d1 = gpiob.pb13.into_floating_input();
        let rx_d0 = gpioc.pc4.into_floating_input();
        let rx_d1 = gpioc.pc5.into_floating_input();

        let (tx_en, tx_d0) = {
            (
                gpiog.pg11.into_floating_input(),
                gpiog.pg13.into_floating_input(),
            )
        };

        let (mdio, mdc) = (
            gpioa.pa2.into_alternate().set_speed(Speed::VeryHigh),
            gpioc.pc1.into_alternate().set_speed(Speed::VeryHigh),
        );

        let pps = gpiob.pb5.into_push_pull_output();

        (
            EthPins {
                ref_clk,
                crs,
                tx_en,
                tx_d0,
                tx_d1,
                rx_d0,
                rx_d1,
            },
            mdio,
            mdc,
            pps,
        )
    }
}

use ieee802_3_miim::{
    phy::{
        lan87xxa::{LAN8720A, LAN8742A},
        BarePhy, KSZ8081R,
    },
    Miim, Pause, Phy,
};

/// An ethernet PHY
pub enum EthernetPhy<M: Miim> {
    /// LAN8720A
    LAN8720A(LAN8720A<M>),
    /// LAN8742A
    LAN8742A(LAN8742A<M>),
    /// KSZ8081R
    KSZ8081R(KSZ8081R<M>),
}

impl<M: Miim> Phy<M> for EthernetPhy<M> {
    fn best_supported_advertisement(
        &self,
    ) -> ieee802_3_miim::AutoNegotiationAdvertisement {
        match self {
            EthernetPhy::LAN8720A(phy) => phy.best_supported_advertisement(),
            EthernetPhy::LAN8742A(phy) => phy.best_supported_advertisement(),
            EthernetPhy::KSZ8081R(phy) => phy.best_supported_advertisement(),
        }
    }

    fn get_miim(&mut self) -> &mut M {
        match self {
            EthernetPhy::LAN8720A(phy) => phy.get_miim(),
            EthernetPhy::LAN8742A(phy) => phy.get_miim(),
            EthernetPhy::KSZ8081R(phy) => phy.get_miim(),
        }
    }

    fn get_phy_addr(&self) -> u8 {
        match self {
            EthernetPhy::LAN8720A(phy) => phy.get_phy_addr(),
            EthernetPhy::LAN8742A(phy) => phy.get_phy_addr(),
            EthernetPhy::KSZ8081R(phy) => phy.get_phy_addr(),
        }
    }
}

impl<M: Miim> EthernetPhy<M> {
    /// Attempt to create one of the known PHYs from the given
    /// MIIM.
    ///
    /// Returns an error if the PHY does not support the extended register
    /// set, or if the PHY's identifier does not correspond to a known PHY.
    pub fn from_miim(miim: M, phy_addr: u8) -> Result<Self, M> {
        let mut bare = BarePhy::new(miim, phy_addr, Pause::NoPause);
        let phy_ident = if let Some(id) = bare.phy_ident() {
            id.raw_u32()
        } else {
            return Err(bare.release());
        };
        let miim = bare.release();
        match phy_ident & 0xFFFFFFF0 {
            0x0007C0F0 => Ok(Self::LAN8720A(LAN8720A::new(miim, phy_addr))),
            0x0007C130 => Ok(Self::LAN8742A(LAN8742A::new(miim, phy_addr))),
            0x00221560 => Ok(Self::KSZ8081R(KSZ8081R::new(miim, phy_addr))),
            _ => Err(miim),
        }
    }

    /// Get a string describing the type of PHY
    pub const fn ident_string(&self) -> &'static str {
        match self {
            EthernetPhy::LAN8720A(_) => "LAN8720A",
            EthernetPhy::LAN8742A(_) => "LAN8742A",
            EthernetPhy::KSZ8081R(_) => "KSZ8081R",
        }
    }

    /// Initialize the PHY
    pub fn phy_init(&mut self) {
        match self {
            EthernetPhy::LAN8720A(phy) => phy.phy_init(),
            EthernetPhy::LAN8742A(phy) => phy.phy_init(),
            EthernetPhy::KSZ8081R(phy) => {
                phy.set_autonegotiation_advertisement(
                    phy.best_supported_advertisement(),
                );
            }
        }
    }

    #[allow(dead_code)]
    pub fn speed(&mut self) -> Option<ieee802_3_miim::phy::PhySpeed> {
        match self {
            EthernetPhy::LAN8720A(phy) => phy.link_speed(),
            EthernetPhy::LAN8742A(phy) => phy.link_speed(),
            EthernetPhy::KSZ8081R(phy) => phy.link_speed(),
        }
    }

    #[allow(dead_code)]
    pub fn release(self) -> M {
        match self {
            EthernetPhy::LAN8720A(phy) => phy.release(),
            EthernetPhy::LAN8742A(phy) => phy.release(),
            EthernetPhy::KSZ8081R(phy) => phy.release(),
        }
    }
}

const ADDRESS: (IpAddress, u16) = (IpAddress::Ipv4(Ipv4Address::new(10, 0, 0, 1)), 1337);

#[rtic::app(device = stm32_eth::stm32, dispatchers = [SPI1])]
mod app {

    use super::EthernetPhy;

    use ieee802_3_miim::{phy::PhySpeed, Phy};
    use systick_monotonic::Systick;

    use stm32_eth::{
        dma::{EthernetDMA, RxRingEntry, TxRingEntry},
        mac::Speed,
        Parts,
    };

    use core::hash::Hasher;
    use smoltcp::{
        iface::{self, Interface, SocketHandle},
        socket::TcpSocket,
        socket::{Dhcpv4Event, Dhcpv4Socket},
        socket::{TcpSocketBuffer, TcpState},
        wire::{EthernetAddress, IpCidr},
    };

    use super::NetworkStorage;

    #[local]
    struct Local {
        interface:
            Interface<'static, &'static mut EthernetDMA<'static, 'static>>,
        tcp_handle: SocketHandle,
        dhcp_handle: SocketHandle,
    }

    #[shared]
    struct Shared {}

    #[monotonic(binds = SysTick, default = true)]
    type Monotonic = Systick<1000>;

    fn now_fn() -> smoltcp::time::Instant {
        let time = monotonics::now().duration_since_epoch().ticks();
        smoltcp::time::Instant::from_millis(time as i64)
    }

    #[init(local = [
        rx_ring: [RxRingEntry; 2] = [RxRingEntry::new(),RxRingEntry::new()],
        tx_ring: [TxRingEntry; 2] = [TxRingEntry::new(),TxRingEntry::new()],
        storage: NetworkStorage = NetworkStorage::new(),
        dma: core::mem::MaybeUninit<EthernetDMA<'static, 'static>> = core::mem::MaybeUninit::uninit(),
    ])]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::println!("Pre-init");
        let core = cx.core;
        let p = cx.device;

        let rx_ring = cx.local.rx_ring;
        let tx_ring = cx.local.tx_ring;

        let (clocks, gpio, ethernet) = super::setup_peripherals(p);
        let mono = Systick::new(core.SYST, clocks.hclk().raw());

        // Chip unique ID, RM0385 rev5 s41.1
        let mut id = [0u32; 3];
        unsafe {
            let ptr = 0x1ff0_f420 as *const u32;
            id[0] = *ptr;
            id[1] = *ptr.offset(1);
            id[2] = *ptr.offset(2);
        }
        defmt::trace!("Unique id: {:x} {:x} {:x}", id[0], id[1], id[2]);

        let mut h = siphasher::sip::SipHasher::new();
        h.write(b"stm32-eth-mac\0");
        h.write_u32(id[0]);
        h.write_u32(id[1]);
        h.write_u32(id[2]);
        let r = h.finish();
        defmt::trace!("Hashed id: {:x}", r);

        let mut mac_address = [0u8; 6];
        let r = r.to_ne_bytes();
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

        defmt::println!("Setting up pins");
        let (pins, mdio, mdc, _) = super::setup_pins(gpio);

        defmt::println!("Configuring ethernet");

        let Parts { dma, mac } = stm32_eth::new_with_mii(
            ethernet, rx_ring, tx_ring, clocks, pins, mdio, mdc,
        )
        .unwrap();

        let dma = cx.local.dma.write(dma);

        defmt::println!("Enabling interrupts");
        dma.enable_interrupt();

        defmt::println!("Setting up smoltcp");
        let store = cx.local.storage;

        let mut routes =
            smoltcp::iface::Routes::new(&mut store.routes_cache[..]);
        routes
            .add_default_ipv4_route(smoltcp::wire::Ipv4Address::UNSPECIFIED)
            .ok();

        let neighbor_cache =
            smoltcp::iface::NeighborCache::new(&mut store.neighbor_cache[..]);

        let rx_buffer =
            TcpSocketBuffer::new(&mut store.tcp_socket_storage.rx_storage[..]);
        let tx_buffer =
            TcpSocketBuffer::new(&mut store.tcp_socket_storage.tx_storage[..]);

        let socket = TcpSocket::new(rx_buffer, tx_buffer);

        let mut interface =
            iface::InterfaceBuilder::new(dma, &mut store.sockets[..])
                .hardware_addr(
                    EthernetAddress::from_bytes(&mac_address).into(),
                )
                .neighbor_cache(neighbor_cache)
                .ip_addrs(&mut store.ip_addrs[..])
                .routes(routes)
                .finalize();

        let tcp_handle = interface.add_socket(socket);

        let dhcp_socket = Dhcpv4Socket::new();
        let dhcp_handle = interface.add_socket(dhcp_socket);

        let socket = interface.get_socket::<TcpSocket>(tcp_handle);
        socket.listen(crate::ADDRESS).ok();

        interface.poll(now_fn()).unwrap();

        if let Ok(mut phy) = EthernetPhy::from_miim(mac, 0) {
            defmt::println!(
                "Resetting PHY as an extra step. Type: {}",
                phy.ident_string()
            );

            phy.phy_init();

            defmt::println!("Waiting for link up.");

            while !phy.phy_link_up() {}

            defmt::println!("Link up.");

            if let Some(speed) = phy.speed().map(|s| match s {
                PhySpeed::HalfDuplexBase10T => Speed::HalfDuplexBase10T,
                PhySpeed::FullDuplexBase10T => Speed::FullDuplexBase10T,
                PhySpeed::HalfDuplexBase100Tx => Speed::HalfDuplexBase100Tx,
                PhySpeed::FullDuplexBase100Tx => Speed::FullDuplexBase100Tx,
            }) {
                phy.get_miim().set_speed(speed);
                defmt::println!("Detected link speed: {}", speed);
            } else {
                defmt::warn!("Failed to detect link speed.");
            }
        } else {
            defmt::println!(
                "Not resetting unsupported PHY. Cannot detect link speed."
            );
        }

        defmt::println!("Setup done. Listening at {}", crate::ADDRESS);

        (
            Shared {},
            Local {
                interface,
                tcp_handle,
                dhcp_handle,
            },
            init::Monotonics(mono),
        )
    }

    #[task(binds = ETH, local = [interface, tcp_handle, dhcp_handle, data: [u8; 512] = [0u8; 512]], priority = 2)]
    fn eth_interrupt(cx: eth_interrupt::Context) {
        let (iface, tcp_handle, dhcp_handle, buffer) = (
            cx.local.interface,
            cx.local.tcp_handle,
            cx.local.dhcp_handle,
            cx.local.data,
        );

        let interrupt_reason = iface.device_mut().interrupt_handler();
        defmt::trace!(
            "Got an ethernet interrupt! Reason: {}",
            interrupt_reason
        );

        iface.poll(now_fn()).ok();

        let event = iface.get_socket::<Dhcpv4Socket>(*dhcp_handle).poll();
        match event {
            None => {}
            Some(Dhcpv4Event::Configured(config)) => {
                defmt::println!("DHCP config acquired!");

                defmt::println!("IP address:      {}", config.address);

                iface.update_ip_addrs(|addrs| {
                    let dest = addrs.iter_mut().next().unwrap();
                    *dest = IpCidr::Ipv4(config.address);
                });

                if let Some(router) = config.router {
                    defmt::println!("Default gateway: {}", router);
                    iface.routes_mut().add_default_ipv4_route(router).unwrap();
                } else {
                    defmt::println!("Default gateway: None");
                    iface.routes_mut().remove_default_ipv4_route();
                }

                for (i, s) in config.dns_servers.iter().enumerate() {
                    defmt::println!("DNS server {}:    {}", i, s);
                }
            }
            Some(Dhcpv4Event::Deconfigured) => {
                defmt::println!("DHCP lost config!");
            }
        }

        let socket = iface.get_socket::<TcpSocket>(*tcp_handle);
        if let Ok(recv_bytes) = socket.recv_slice(buffer) {
            if recv_bytes > 0 {
                socket.send_slice(&buffer[..recv_bytes]).ok();
                defmt::println!("Echoed {} bytes.", recv_bytes);
            }
        }

        if !socket.is_listening() && !socket.is_open()
            || socket.state() == TcpState::CloseWait
        {
            socket.abort();
            socket.listen(crate::ADDRESS).ok();
            defmt::warn!("Disconnected... Reopening listening socket.");
        }

        iface.poll(now_fn()).ok();
    }
}

/// All storage required for networking
pub struct NetworkStorage {
    pub ip_addrs: [wire::IpCidr; 1],
    pub sockets: [iface::SocketStorage<'static>; 2],
    pub tcp_socket_storage: TcpSocketStorage,
    pub neighbor_cache: [Option<(wire::IpAddress, iface::Neighbor)>; 8],
    pub routes_cache: [Option<(wire::IpCidr, iface::Route)>; 8],
}

impl NetworkStorage {
    const IP_INIT: wire::IpCidr = wire::IpCidr::Ipv4(wire::Ipv4Cidr::new(
        wire::Ipv4Address::new(10, 0, 0, 1),
        24,
    ));

    pub const fn new() -> Self {
        NetworkStorage {
            ip_addrs: [Self::IP_INIT],
            neighbor_cache: [None; 8],
            routes_cache: [None; 8],
            sockets: [SocketStorage::EMPTY; 2],
            tcp_socket_storage: TcpSocketStorage::new(),
        }
    }
}

/// Storage of TCP sockets
#[derive(Copy, Clone)]
pub struct TcpSocketStorage {
    rx_storage: [u8; 512],
    tx_storage: [u8; 512],
}

impl TcpSocketStorage {
    const fn new() -> Self {
        Self {
            rx_storage: [0; 512],
            tx_storage: [0; 512],
        }
    }
}

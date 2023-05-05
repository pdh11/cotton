#![no_std]
#![no_main]

use defmt_rtt as _; // global logger
use fugit::RateExtU32;
use linked_list_allocator::LockedHeap;
use panic_probe as _;
use smoltcp::socket::UdpPacketMetadata;
use smoltcp::{
    iface::{self, SocketStorage},
    wire,
};
use stm32_eth::hal::rcc::Clocks;
use stm32_eth::hal::rcc::RccExt;
use stm32f7xx_hal as _;

extern crate alloc;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub fn setup_clocks(rcc: stm32_eth::stm32::RCC) -> Clocks {
    let rcc = rcc.constrain();

    rcc.cfgr.sysclk(100.MHz()).hclk(100.MHz()).freeze()
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

extern "C" {
    static mut __sheap: u32;
    static mut _stack_start: u32;
}

#[rtic::app(device = stm32_eth::stm32, dispatchers = [SPI1])]
mod app {

    use super::EthernetPhy;

    use ieee802_3_miim::{phy::PhySpeed, Phy};
    use systick_monotonic::Systick;

    use stm32_eth::{
        dma::{EthernetDMA, RxRingEntry, TxRingEntry},
        hal::gpio::GpioExt,
        mac::Speed,
        Parts,
    };

    use core::hash::Hasher;
    use smoltcp::{
        iface::{self, Interface, SocketHandle},
        socket::{Dhcpv4Event, Dhcpv4Socket},
        wire::{self, EthernetAddress, IpCidr},
    };

    use smoltcp::socket::{UdpSocket, UdpSocketBuffer};

    use super::NetworkStorage;
    use crate::alloc::string::ToString;

    pub struct Listener {}

    #[local]
    struct Local {
        interface:
            Interface<'static, &'static mut EthernetDMA<'static, 'static>>,
        dhcp_handle: SocketHandle,
        udp_handle: SocketHandle,
        ssdp: cotton_ssdp::engine::Engine<Listener>,
    }

    impl cotton_ssdp::engine::Callback for Listener {
        fn on_notification(&self, notification: &cotton_ssdp::Notification) {
            if let cotton_ssdp::Notification::Alive {
                ref notification_type,
                ..
            } = notification
            {
                defmt::println!("SSDP! {:?}", &notification_type[..]);
            }
        }
    }

    struct GenericIpv4Address(no_std_net::Ipv4Addr);

    impl From<wire::Ipv4Address> for GenericIpv4Address {
        fn from(ip: wire::Ipv4Address) -> Self {
            GenericIpv4Address(no_std_net::Ipv4Addr::from(ip.0))
        }
    }

    impl From<GenericIpv4Address> for wire::Ipv4Address {
        fn from(ip: GenericIpv4Address) -> Self {
            wire::Ipv4Address(ip.0.octets())
        }
    }

    impl From<no_std_net::Ipv4Addr> for GenericIpv4Address {
        fn from(ip: no_std_net::Ipv4Addr) -> Self {
            GenericIpv4Address(ip)
        }
    }

    impl From<GenericIpv4Address> for no_std_net::Ipv4Addr {
        fn from(ip: GenericIpv4Address) -> Self {
            ip.0
        }
    }

    struct GenericIpAddress(no_std_net::IpAddr);

    impl From<wire::IpAddress> for GenericIpAddress {
        fn from(ip: wire::IpAddress) -> Self {
            match ip {
                wire::IpAddress::Ipv4(v4) => GenericIpAddress(
                    no_std_net::IpAddr::V4(no_std_net::Ipv4Addr::from(v4.0)),
                ),
                _ => GenericIpAddress(no_std_net::IpAddr::V4(
                    no_std_net::Ipv4Addr::UNSPECIFIED,
                )),
            }
        }
    }

    impl From<GenericIpAddress> for wire::IpAddress {
        fn from(ip: GenericIpAddress) -> Self {
            match ip.0 {
                no_std_net::IpAddr::V4(v4) => {
                    wire::IpAddress::Ipv4(wire::Ipv4Address(v4.octets()))
                }
                // no_std_net::IpAddr::V6(v6) => wire::IpAddress::Ipv6(v6.octets()),
                _ => wire::IpAddress::Unspecified,
            }
        }
    }

    impl From<no_std_net::IpAddr> for GenericIpAddress {
        fn from(ip: no_std_net::IpAddr) -> Self {
            GenericIpAddress(ip)
        }
    }

    impl From<GenericIpAddress> for no_std_net::IpAddr {
        fn from(ip: GenericIpAddress) -> Self {
            ip.0
        }
    }

    struct GenericSocketAddr(no_std_net::SocketAddr);

    impl From<wire::IpEndpoint> for GenericSocketAddr {
        fn from(ep: wire::IpEndpoint) -> Self {
            match ep.addr {
                wire::IpAddress::Ipv4(v4) => GenericSocketAddr(
                    no_std_net::SocketAddr::V4(no_std_net::SocketAddrV4::new(
                        GenericIpv4Address::from(v4).into(),
                        ep.port,
                    )),
                ),
                _ => GenericSocketAddr(no_std_net::SocketAddr::V4(
                    no_std_net::SocketAddrV4::new(
                        no_std_net::Ipv4Addr::UNSPECIFIED,
                        ep.port,
                    ),
                )),
            }
        }
    }

    impl From<GenericSocketAddr> for wire::IpEndpoint {
        fn from(sa: GenericSocketAddr) -> Self {
            match sa.0 {
                no_std_net::SocketAddr::V4(v4) => wire::IpEndpoint::new(
                    wire::IpAddress::Ipv4(GenericIpv4Address(*v4.ip()).into()),
                    v4.port(),
                ),
                _ => wire::IpEndpoint::new(wire::IpAddress::Unspecified, 0),
            }
        }
    }

    impl From<no_std_net::SocketAddr> for GenericSocketAddr {
        fn from(ip: no_std_net::SocketAddr) -> Self {
            GenericSocketAddr(ip)
        }
    }

    impl From<GenericSocketAddr> for no_std_net::SocketAddr {
        fn from(ip: GenericSocketAddr) -> Self {
            ip.0
        }
    }

    struct WrappedInterface(
        core::cell::RefCell<
            Interface<'static, &'static mut EthernetDMA<'static, 'static>>,
        >,
    );

    impl cotton_ssdp::udp::Multicast for WrappedInterface {
        fn join_multicast_group(
            &self,
            multicast_address: &no_std_net::IpAddr,
            _interface: cotton_netif::InterfaceIndex,
        ) -> Result<(), cotton_ssdp::udp::Error> {
            defmt::println!("JMG!");
            self.0
                .borrow_mut()
                .join_multicast_group::<wire::IpAddress>(
                    GenericIpAddress::from(*multicast_address).into(),
                    now_fn(),
                )
                .map(|_| ())
                .map_err(|_| cotton_ssdp::udp::Error::NoPacketInfo)
        }

        fn leave_multicast_group(
            &self,
            _multicast_address: &no_std_net::IpAddr,
            _interface: cotton_netif::InterfaceIndex,
        ) -> Result<(), cotton_ssdp::udp::Error> {
            defmt::println!("LMG!");
            Err(cotton_ssdp::udp::Error::NoPacketInfo)
        }
    }

    struct WrappedSocket {}

    impl cotton_ssdp::udp::TargetedSend for WrappedSocket {
        fn send_with<F>(
            &self,
            _size: usize,
            _to: &no_std_net::SocketAddr,
            _from: &no_std_net::IpAddr,
            _f: F,
        ) -> Result<(), cotton_ssdp::udp::Error>
        where
            F: FnOnce(&mut [u8]) -> usize,
        {
            defmt::println!("Send!");
            Err(cotton_ssdp::udp::Error::NoPacketInfo)
        }
    }

    impl cotton_ssdp::udp::TargetedReceive for WrappedSocket {
        fn receive_to(
            &self,
            _buffer: &mut [u8],
        ) -> Result<
            (usize, no_std_net::IpAddr, no_std_net::SocketAddr),
            cotton_ssdp::udp::Error,
        > {
            defmt::println!("Receive!");
            Err(cotton_ssdp::udp::Error::NoPacketInfo)
        }
    }

    #[shared]
    struct Shared {}

    #[monotonic(binds = SysTick, default = true)]
    type Monotonic = Systick<1000>;

    fn now_fn() -> smoltcp::time::Instant {
        let time = monotonics::now().duration_since_epoch().ticks();
        smoltcp::time::Instant::from_millis(time as i64)
    }

    fn init_heap() {
        const STACK_SIZE: usize = 16 * 1024;
        unsafe {
            let heap_start = &super::__sheap as *const u32 as usize;
            let heap_end = &super::_stack_start as *const u32 as usize;
            let heap_size = heap_end - heap_start - STACK_SIZE;
            super::ALLOCATOR
                .lock()
                .init(heap_start as *mut u8, heap_size);
        }
    }

    #[init(local = [
        rx_ring: [RxRingEntry; 2] = [RxRingEntry::new(),RxRingEntry::new()],
        tx_ring: [TxRingEntry; 2] = [TxRingEntry::new(),TxRingEntry::new()],
        storage: NetworkStorage = NetworkStorage::new(),
        dma: core::mem::MaybeUninit<EthernetDMA<'static, 'static>> = core::mem::MaybeUninit::uninit(),
    ])]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::println!("Pre-init");
        init_heap();
        let core = cx.core;
        let p = cx.device;

        let rx_ring = cx.local.rx_ring;
        let tx_ring = cx.local.tx_ring;

        let clocks = super::setup_clocks(p.RCC);
        let ethernet = stm32_eth::PartsIn {
            dma: p.ETHERNET_DMA,
            mac: p.ETHERNET_MAC,
            mmc: p.ETHERNET_MMC,
        };
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

        let gpioa = p.GPIOA.split();
        let gpiob = p.GPIOB.split();
        let gpioc = p.GPIOC.split();
        let gpiog = p.GPIOG.split();

        defmt::println!("Configuring ethernet");

        let Parts { dma, mac } = stm32_eth::new_with_mii(
            ethernet,
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

        let mut interface =
            iface::InterfaceBuilder::new(dma, &mut store.sockets[..])
                .hardware_addr(
                    EthernetAddress::from_bytes(&mac_address).into(),
                )
                .neighbor_cache(neighbor_cache)
                .ip_addrs(&mut store.ip_addrs[..])
                .routes(routes)
                .ipv4_multicast_groups(&mut store.multicast_storage[..])
                .finalize();

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

        let dhcp_socket = Dhcpv4Socket::new();
        let dhcp_handle = interface.add_socket(dhcp_socket);

        let udp_rx_buffer = UdpSocketBuffer::new(
            &mut store.udp_socket_storage.rx_metadata[..],
            &mut store.udp_socket_storage.rx_storage[..],
        );

        let udp_tx_buffer = UdpSocketBuffer::new(
            &mut store.udp_socket_storage.tx_metadata[..],
            &mut store.udp_socket_storage.tx_storage[..],
        );
        let mut udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);
        _ = udp_socket.bind(1900);
        let udp_handle = interface.add_socket(udp_socket);

        interface.poll(now_fn()).unwrap();
        interface.get_socket::<Dhcpv4Socket>(dhcp_handle).poll();

        let mut ssdp = cotton_ssdp::engine::Engine::new();

        let ix = cotton_netif::InterfaceIndex(
            core::num::NonZeroU32::new(1).unwrap(),
        );
        let ev = cotton_netif::NetworkEvent::NewLink(
            ix,
            "".to_string(),
            cotton_netif::Flags::MULTICAST,
        );
        let ws = WrappedSocket {};
        defmt::println!("Calling o-n-e");
        let wi = WrappedInterface(core::cell::RefCell::new(interface));
        _ = ssdp.on_network_event(&ev, &wi, &ws);
        defmt::println!("o-n-e returned");

        let mut loc = Local {
            interface: wi.0.into_inner(),
            dhcp_handle,
            udp_handle,
            ssdp,
        };

        loc.ssdp.subscribe("ssdp:all".to_string(), Listener {}, &ws);

        (Shared {}, loc, init::Monotonics(mono))
    }

    #[task(binds = ETH, local = [interface, dhcp_handle, udp_handle, ssdp], priority = 2)]
    fn eth_interrupt(cx: eth_interrupt::Context) {
        let (iface, dhcp_handle, udp_handle, ssdp) = (
            cx.local.interface,
            cx.local.dhcp_handle,
            cx.local.udp_handle,
            cx.local.ssdp,
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

        if let Some(wasto) = iface.ipv4_addr() {
            let wasto = wire::IpAddress::Ipv4(wasto);
            let socket = iface.get_socket::<UdpSocket>(*udp_handle);
            if socket.can_recv() {
                socket
                    .recv()
                    .map(|(data, sender)| {
                        defmt::println!("{} from {}", data.len(), sender);
                        let ws = WrappedSocket {};
                        ssdp.on_data(
                            data,
                            &ws,
                            GenericIpAddress::from(wasto).into(),
                            GenericSocketAddr::from(sender).into(),
                        );
                    })
                    .unwrap_or_else(|e| {
                        defmt::println!("Recv UDP error: {:?}", e)
                    });
            }
        }

        iface.poll(now_fn()).ok();
    }
}

/// All storage required for networking
pub struct NetworkStorage {
    pub ip_addrs: [wire::IpCidr; 1],
    pub sockets: [iface::SocketStorage<'static>; 2],
    pub neighbor_cache: [Option<(wire::IpAddress, iface::Neighbor)>; 8],
    pub routes_cache: [Option<(wire::IpCidr, iface::Route)>; 8],
    pub multicast_storage: [Option<(wire::Ipv4Address, ())>; 1],
    pub udp_socket_storage: UdpSocketStorage,
}

impl NetworkStorage {
    const IP_INIT: wire::IpCidr = wire::IpCidr::Ipv4(wire::Ipv4Cidr::new(
        wire::Ipv4Address::UNSPECIFIED,
        24,
    ));

    pub const fn new() -> Self {
        NetworkStorage {
            ip_addrs: [Self::IP_INIT],
            sockets: [SocketStorage::EMPTY; 2],
            neighbor_cache: [None; 8],
            routes_cache: [None; 8],
            multicast_storage: [None; 1],
            udp_socket_storage: UdpSocketStorage::new(),
        }
    }
}

/// Storage of TCP sockets
#[derive(Copy, Clone)]
pub struct UdpSocketStorage {
    rx_metadata: [UdpPacketMetadata; 4],
    rx_storage: [u8; 512],
    tx_metadata: [UdpPacketMetadata; 4],
    tx_storage: [u8; 512],
}

impl UdpSocketStorage {
    const fn new() -> Self {
        Self {
            rx_metadata: [UdpPacketMetadata::EMPTY; 4],
            rx_storage: [0; 512],
            tx_metadata: [UdpPacketMetadata::EMPTY; 4],
            tx_storage: [0; 512],
        }
    }
}

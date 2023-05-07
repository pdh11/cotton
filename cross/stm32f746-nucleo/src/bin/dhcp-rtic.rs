#![no_std]
#![no_main]

use defmt_rtt as _; // global logger
use panic_probe as _;
use stm32f7xx_hal as _;
use smoltcp::iface::{self, SocketStorage};
use stm32_eth::hal::rcc::Clocks;
use fugit::RateExtU32;
use stm32_eth::hal::rcc::RccExt;

pub fn setup_clocks(rcc: stm32_eth::stm32::RCC) -> Clocks {
    let rcc = rcc.constrain();

    rcc.cfgr.sysclk(100.MHz()).hclk(100.MHz()).freeze()
}

#[rtic::app(device = stm32_eth::stm32, dispatchers = [SPI1])]
mod app {
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
        iface::{self, Interface, SocketHandle, SocketSet},
        socket::dhcpv4,
        wire::{EthernetAddress, IpCidr},
    };
    use super::NetworkStorage;

    #[local]
    struct Local {
        device: EthernetDMA<'static, 'static>,
        interface: Interface,
        socket_set: SocketSet<'static>,
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
    ])]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::println!("Pre-init");
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

        let Parts { mut dma, mac } = stm32_eth::new_with_mii(
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

        defmt::println!("Enabling interrupts");
        dma.enable_interrupt();

        defmt::println!("Setting up smoltcp");
        let store = cx.local.storage;

        let mut routes = smoltcp::iface::Routes::new();
        routes
            .add_default_ipv4_route(smoltcp::wire::Ipv4Address::UNSPECIFIED)
            .ok();

        let mut config = smoltcp::iface::Config::new();
        // config.random_seed = mono.now();
        config.hardware_addr =
            Some(EthernetAddress::from_bytes(&mac_address).into());

        let mut interface = iface::Interface::new(config, &mut &mut dma);
        let mut socket_set =
            smoltcp::iface::SocketSet::new(&mut store.sockets[..]);

        interface.poll(now_fn(), &mut &mut dma, &mut socket_set);

        let mut phy = ieee802_3_miim::phy::LAN8742A::new(mac, 0);
        phy.phy_init();

        // LAN8742A has an interrupt for link up, but Nucleo doesn't wire
        // it to anything
        defmt::println!("Waiting for link up.");
        while !phy.link_established() {}

        defmt::println!("Link up.");

        if let Some(speed) = phy.link_speed().map(|s| match s {
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

        let dhcp_socket = dhcpv4::Socket::new();
        let dhcp_handle = socket_set.add(dhcp_socket);

        interface.poll(now_fn(), &mut &mut dma, &mut socket_set);
        socket_set.get_mut::<dhcpv4::Socket>(dhcp_handle).poll();

        (
            Shared {},
            Local {
                device: dma,
                interface,
                socket_set,
                dhcp_handle,
            },
            init::Monotonics(mono),
        )
    }

    #[task(binds = ETH, local = [device, interface, socket_set, dhcp_handle], priority = 2)]
    fn eth_interrupt(cx: eth_interrupt::Context) {
        let (mut device, iface, socket_set, dhcp_handle) = (
            cx.local.device,
            cx.local.interface,
            cx.local.socket_set,
            cx.local.dhcp_handle,
        );

        let interrupt_reason =
            EthernetDMA::<'static, 'static>::interrupt_handler();
        defmt::trace!(
            "Got an ethernet interrupt! Reason: {}",
            interrupt_reason
        );

        iface.poll(now_fn(), &mut device, socket_set);

        let event = socket_set.get_mut::<dhcpv4::Socket>(*dhcp_handle).poll();
        match event {
            None => {}
            Some(dhcpv4::Event::Configured(config)) => {
                defmt::println!("DHCP config acquired!");

                defmt::println!("IP address:      {}", config.address);

                iface.update_ip_addrs(|addrs| {
                    addrs.push(IpCidr::Ipv4(config.address)).unwrap();
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
            Some(dhcpv4::Event::Deconfigured) => {
                defmt::println!("DHCP lost config!");
                iface.update_ip_addrs(|addrs| {
                    addrs.clear();
                });
            }
        }

        iface.poll(now_fn(), &mut device, socket_set);
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

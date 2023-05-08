#![no_std]
#![no_main]

use core::hash::Hasher;
use defmt_rtt as _; // global logger
use fugit::RateExtU32;
use hal::gpio::GpioExt;
use ieee802_3_miim::{phy::PhySpeed, Phy};
use panic_probe as _;
use smoltcp::iface::{self, SocketStorage};
use smoltcp::{socket::dhcpv4, wire::IpCidr};
use stm32_eth::dma::{RxRingEntry, TxRingEntry};
use stm32_eth::hal::rcc::Clocks;
use stm32_eth::hal::rcc::RccExt;
use stm32f7xx_hal as hal;

pub fn setup_clocks(rcc: stm32_eth::stm32::RCC) -> Clocks {
    let rcc = rcc.constrain();

    rcc.cfgr.sysclk(100.MHz()).hclk(100.MHz()).freeze()
}

pub fn stm32_unique_id() -> &'static [u32; 3] {
    // Chip unique ID, RM0385 rev5 s41.1
    unsafe {
        let ptr = 0x1ff0_f420 as *const [u32; 3];
        &*ptr
    }
}

pub fn unique_id(salt: &[u8]) -> u64 {
    let id = stm32_unique_id();
    let key1 = ((id[0] as u64) << 32) + (id[1] as u64);
    let key2 = id[2] as u64;
    let mut h = siphasher::sip::SipHasher::new_with_keys(key1, key2);
    h.write(salt);
    h.finish()
}

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

pub struct Stm32Ethernet {
    pub dma: stm32_eth::dma::EthernetDMA<'static, 'static>,
    phy: ieee802_3_miim::phy::LAN8742A<
        stm32_eth::mac::EthernetMACWithMii<MdioPa2, MdcPc1>,
    >,
    got_link: bool,
}

impl Stm32Ethernet {
    #[allow(clippy::too_many_arguments)]
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
            stm32_eth::PartsIn { dma, mac, mmc },
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

pub struct Stack<'a> {
    pub interface: smoltcp::iface::Interface,
    pub socket_set: smoltcp::iface::SocketSet<'a>,
    pub dhcp_handle: smoltcp::iface::SocketHandle,
}

impl<'a> Stack<'a> {
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
            min_renew_timeout: smoltcp::time::Duration::from_secs(864000),
        });
        let dhcp_handle = socket_set.add(dhcp_socket);

        Stack {
            interface,
            socket_set,
            dhcp_handle,
        }
    }

    pub fn poll<D: smoltcp::phy::Device>(
        &mut self,
        now: smoltcp::time::Instant,
        device: &mut D,
    ) {
        self.interface.poll(now, device, &mut self.socket_set);
        self.poll_dhcp();
    }

    pub fn poll_dhcp(&mut self) {
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

#[rtic::app(device = stm32_eth::stm32, dispatchers = [SPI1])]
mod app {
    use super::NetworkStorage;
    use fugit::ExtU64;
    use stm32_eth::dma::EthernetDMA;
    use systick_monotonic::Systick;

    #[local]
    struct Local {
        device: crate::Stm32Ethernet,
        stack: crate::Stack<'static>,
        nvic: stm32_eth::stm32::NVIC,
    }

    #[shared]
    struct Shared {}

    #[monotonic(binds = SysTick, default = true)]
    type Monotonic = Systick<1000>;

    fn now_fn() -> smoltcp::time::Instant {
        let time = monotonics::now().duration_since_epoch().ticks();
        smoltcp::time::Instant::from_millis(time as i64)
    }

    #[init(local = [ storage: NetworkStorage = NetworkStorage::new() ])]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::println!("Pre-init");
        let core = cx.core;

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
        } = cx.device;

        let clocks = super::setup_clocks(RCC);
        let mono = Systick::new(core.SYST, clocks.hclk().raw());

        let mut device = super::Stm32Ethernet::new(
            GPIOA,
            GPIOB,
            GPIOC,
            GPIOG,
            ETHERNET_DMA,
            ETHERNET_MAC,
            ETHERNET_MMC,
            clocks,
            &mut cx.local.storage.rx_ring,
            &mut cx.local.storage.tx_ring,
        );

        // LAN8742A has an interrupt for link up, but Nucleo doesn't
        // wire it to anything
        defmt::println!("Waiting for link up.");
        while !device.link_established() {}

        defmt::println!("Link up.");

        let mac_address = super::mac_address();
        // NB stm32-eth implements smoltcp::Device not for
        // EthernetDMA, but for "&mut EthernetDMA"
        let mut stack = super::Stack::new(
            &mut &mut device.dma,
            &mac_address,
            &mut cx.local.storage.sockets[..],
        );
        stack.poll(now_fn(), &mut &mut device.dma);

        periodic::spawn_after(2.secs()).unwrap();

        (
            Shared {},
            Local {
                device,
                stack,
                nvic: core.NVIC,
            },
            init::Monotonics(mono),
        )
    }

    #[task(local = [nvic])]
    fn periodic(cx: periodic::Context) {
        let nvic = cx.local.nvic;
        nvic.request(stm32_eth::stm32::Interrupt::ETH);
        periodic::spawn_after(2.secs()).unwrap();
    }

    #[task(binds = ETH, local = [device, stack], priority = 2)]
    fn eth_interrupt(cx: eth_interrupt::Context) {
        let (device, stack) = (cx.local.device, cx.local.stack);

        EthernetDMA::<'static, 'static>::interrupt_handler();
        stack.poll(now_fn(), &mut &mut device.dma);
    }
}

/// All storage required for networking
pub struct NetworkStorage {
    pub rx_ring: [RxRingEntry; 2],
    pub tx_ring: [TxRingEntry; 2],
    pub sockets: [iface::SocketStorage<'static>; 2],
}

impl NetworkStorage {
    pub const fn new() -> Self {
        NetworkStorage {
            rx_ring: [RxRingEntry::new(), RxRingEntry::new()],
            tx_ring: [TxRingEntry::new(), TxRingEntry::new()],
            sockets: [SocketStorage::EMPTY; 2],
        }
    }
}

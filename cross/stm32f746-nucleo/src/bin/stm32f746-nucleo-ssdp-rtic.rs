//! On an STM32F746-Nucleo, bring up Ethernet and TCP and obtain a DHCP address
//! and start doing SSDP
#![no_std]
#![no_main]

extern crate alloc;

use defmt_rtt as _; // global logger
use linked_list_allocator::LockedHeap;
use panic_probe as _;
use smoltcp::iface::{self, SocketStorage};
use stm32_eth::dma::{RxRingEntry, TxRingEntry};
use stm32f7xx_hal as _;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[rtic::app(device = stm32_eth::stm32, dispatchers = [SPI1])]
mod app {
    use super::NetworkStorage;
    use crate::alloc::string::ToString;
    use cotton_ssdp::udp::smoltcp::{
        GenericIpAddress, GenericIpv4Address, GenericSocketAddr,
        WrappedInterface, WrappedSocket,
    };
    use cotton_stm32f746_nucleo::common;
    use fugit::ExtU64;
    use smoltcp::{iface::SocketHandle, socket::udp, wire};
    use systick_monotonic::Systick;

    pub struct Listener {}

    #[local]
    struct Local {
        device: common::Stm32Ethernet,
        stack: common::Stack<'static>,
        udp_handle: SocketHandle,
        ssdp: cotton_ssdp::engine::Engine<Listener>,
        nvic: stm32_eth::stm32::NVIC,
    }

    impl cotton_ssdp::engine::Callback for Listener {
        fn on_notification(&self, notification: &cotton_ssdp::Notification) {
            if let cotton_ssdp::Notification::Alive {
                ref notification_type,
                location,
                ..
            } = notification
            {
                defmt::println!(
                    "SSDP! {} {}",
                    &notification_type[..],
                    &location[..]
                );
            }
        }
    }

    #[shared]
    struct Shared {}

    #[monotonic(binds = SysTick, default = true)]
    type Monotonic = Systick<1000>;

    fn upnp_uuid() -> alloc::string::String {
        // uuid crate isn't no_std :(
        let mut u1 = common::unique_id(b"upnp-uuid-1");
        let mut u2 = common::unique_id(b"upnp-uuid-2");
        // Variant 1
        u2 |= 0x8000_0000_0000_0000_u64;
        u2 &= !0x4000_0000_0000_0000_u64;
        // Version 5
        u1 &= !0xF000;
        u1 |= 0x5000;

        alloc::format!("{:016x}{:016x}", u1, u2)
    }

    fn now_fn() -> smoltcp::time::Instant {
        let time = monotonics::now().duration_since_epoch().ticks();
        smoltcp::time::Instant::from_millis(time as i64)
    }

    #[init(local = [
        storage: NetworkStorage = NetworkStorage::new(),
    ])]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::println!("Pre-init");
        common::init_heap(&super::ALLOCATOR);
        let core = cx.core;

        let (ethernet_peripherals, rcc) = common::split_peripherals(cx.device);
        let clocks = common::setup_clocks(rcc);
        let mono = Systick::new(core.SYST, clocks.hclk().raw());

        let mut device = common::Stm32Ethernet::new(
            ethernet_peripherals,
            clocks,
            &mut cx.local.storage.rx_ring,
            &mut cx.local.storage.tx_ring,
        );

        // LAN8742A has an interrupt for link up, but Nucleo doesn't
        // wire it to anything
        defmt::println!("Waiting for link up.");
        while !device.link_established() {}

        defmt::println!("Link up.");

        let mac_address = common::mac_address();
        // NB stm32-eth implements smoltcp::Device not for
        // EthernetDMA, but for "&mut EthernetDMA"
        let mut stack = common::Stack::new(
            &mut &mut device.dma,
            &mac_address,
            &mut cx.local.storage.sockets[..],
            now_fn(),
        );

        let udp_rx_buffer = udp::PacketBuffer::new(
            &mut cx.local.storage.rx_metadata[..],
            &mut cx.local.storage.rx_storage[..],
        );

        let udp_tx_buffer = udp::PacketBuffer::new(
            &mut cx.local.storage.tx_metadata[..],
            &mut cx.local.storage.tx_storage[..],
        );
        let mut udp_socket = udp::Socket::new(udp_rx_buffer, udp_tx_buffer);
        _ = udp_socket.bind(1900);
        let mut ssdp = cotton_ssdp::engine::Engine::new();

        let ix = cotton_netif::InterfaceIndex(
            core::num::NonZeroU32::new(1).unwrap(),
        );
        let ev = cotton_netif::NetworkEvent::NewLink(
            ix,
            "".to_string(),
            cotton_netif::Flags::UP
                | cotton_netif::Flags::RUNNING
                | cotton_netif::Flags::MULTICAST,
        );

        {
            let mut device = &mut device.dma;
            let wi = WrappedInterface::new(
                &mut stack.interface,
                &mut device,
                now_fn(),
            );
            let ws = WrappedSocket::new(&mut udp_socket);
            _ = ssdp.on_network_event(&ev, &wi, &ws);
            ssdp.subscribe("ssdp:all".to_string(), Listener {}, &ws);

            let uuid = upnp_uuid();
            ssdp.advertise(
                uuid,
                cotton_ssdp::Advertisement {
                    notification_type: "stm32f746-nucleo-test".to_string(),
                    location: "http://127.0.0.1/".to_string(),
                },
                &ws,
            );
        }

        let udp_handle = stack.socket_set.add(udp_socket);

        periodic::spawn_after(2.secs()).unwrap();

        (
            Shared {},
            Local {
                device,
                stack,
                udp_handle,
                ssdp,
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

    #[task(binds = ETH, local = [device, stack, udp_handle, ssdp], priority = 2)]
    fn eth_interrupt(cx: eth_interrupt::Context) {
        let (device, stack, udp_handle, ssdp) = (
            cx.local.device,
            cx.local.stack,
            cx.local.udp_handle,
            cx.local.ssdp,
        );

        let old_ip = stack.interface.ipv4_addr();
        stack.poll(now_fn(), &mut &mut device.dma);
        let new_ip = stack.interface.ipv4_addr();

        if let (None, Some(ip)) = (old_ip, new_ip) {
            let socket = stack.socket_set.get_mut::<udp::Socket>(*udp_handle);
            let ws = WrappedSocket::new(socket);

            ssdp.on_new_addr_event(
                &cotton_netif::InterfaceIndex(
                    core::num::NonZeroU32::new(1).unwrap(),
                ),
                &no_std_net::IpAddr::V4(GenericIpv4Address::from(ip).into()),
                &ws,
            );

            defmt::println!("Refreshing!");
            ssdp.refresh(&ws);
        }

        if let Some(wasto) = new_ip {
            let wasto = wire::IpAddress::Ipv4(wasto);
            let socket = stack.socket_set.get_mut::<udp::Socket>(*udp_handle);
            if socket.can_recv() {
                // Shame about the copy here, but we need the socket
                // borrowed mutably to write to it (in on_data), and
                // we also need the data borrowed to read it -- but
                // that's an immutable borrow at the same time as a
                // mutable borrow, which isn't allowed.  We could
                // perhaps have entirely separate sockets for send and
                // receive, but it's simpler just to copy the data.
                let mut buffer = [0u8; 512];
                if let Ok((size, sender)) = socket.recv_slice(&mut buffer) {
                    // defmt::println!("{} from {}", size, sender);
                    let ws = WrappedSocket::new(socket);
                    ssdp.on_data(
                        &buffer[0..size],
                        &ws,
                        GenericIpAddress::from(wasto).into(),
                        GenericSocketAddr::from(sender.endpoint).into(),
                    );
                }
            }
        }
    }
}

/// All storage required for networking
pub struct NetworkStorage {
    pub rx_ring: [RxRingEntry; 2],
    pub tx_ring: [TxRingEntry; 2],
    pub sockets: [iface::SocketStorage<'static>; 2],
    pub rx_metadata: [smoltcp::socket::udp::PacketMetadata; 16],
    pub rx_storage: [u8; 8192],
    pub tx_metadata: [smoltcp::socket::udp::PacketMetadata; 8],
    pub tx_storage: [u8; 2048],
}

impl NetworkStorage {
    pub const fn new() -> Self {
        NetworkStorage {
            rx_ring: [RxRingEntry::new(), RxRingEntry::new()],
            tx_ring: [TxRingEntry::new(), TxRingEntry::new()],
            sockets: [SocketStorage::EMPTY; 2],
            rx_metadata: [smoltcp::socket::udp::PacketMetadata::EMPTY; 16],
            rx_storage: [0; 8192],
            tx_metadata: [smoltcp::socket::udp::PacketMetadata::EMPTY; 8],
            tx_storage: [0; 2048],
        }
    }
}

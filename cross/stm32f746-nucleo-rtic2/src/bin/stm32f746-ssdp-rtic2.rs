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
    use cotton_ssdp::refresh_timer::SmoltcpTimebase;
    use cotton_ssdp::udp::smoltcp::{WrappedInterface, WrappedSocket};
    use cotton_stm32f746_nucleo::common;
    use rtic_monotonics::systick::prelude::*;
    use rtic_sync::make_channel;
    use smoltcp::{iface::SocketHandle, socket::udp, wire};

    type Sender = rtic_sync::channel::Sender<'static, (), 1>;
    type Receiver = rtic_sync::channel::Receiver<'static, (), 1>;

    systick_monotonic!(Mono, 1_000);

    pub struct Listener {}

    #[local]
    struct Local {
        device: common::Stm32Ethernet,
        stack: common::Stack<'static>,
        udp_handle: SocketHandle,
        ssdp: cotton_ssdp::engine::Engine<Listener, SmoltcpTimebase>,
        sender: Sender,
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

    fn now_fn() -> smoltcp::time::Instant {
        let time = Mono::now().duration_since_epoch().ticks();
        smoltcp::time::Instant::from_millis(time as i64)
    }

    #[init(local = [
        storage: NetworkStorage = NetworkStorage::new(),
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        defmt::println!("Pre-init");
        common::init_heap(&super::ALLOCATOR);
        let unique_id = cotton_unique::stm32::unique_chip_id(
            stm32_device_signature::device_id(),
        );

        let (ethernet_peripherals, rcc) = common::split_peripherals(cx.device);
        let clocks = common::setup_clocks(rcc);
        Mono::start(cx.core.SYST, clocks.hclk().raw());

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

        let mac_address = cotton_unique::mac_address(&unique_id, b"stm32-eth");
        // NB stm32-eth implements smoltcp::Device not for
        // EthernetDMA, but for "&mut EthernetDMA"
        let mut stack = common::Stack::new(
            &mut &mut device.dma,
            &unique_id,
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
        let random_seed = unique_id.id(b"ssdp-refresh") as u32;
        let mut ssdp = cotton_ssdp::engine::Engine::new(random_seed, now_fn());

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
            let wi = WrappedInterface::new(&mut stack.interface);
            let ws = WrappedSocket::new(&mut udp_socket);
            _ = ssdp.on_network_event(&ev, &wi, &ws);
            ssdp.subscribe(
                "cotton-test-server-stm32f746".to_string(),
                Listener {},
                &ws,
            );

            let uuid = alloc::format!(
                "{:032x}",
                cotton_unique::uuid(&unique_id, b"upnp")
            );
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

        let (sender, receiver) = make_channel!((), 1);

        network_task::spawn(receiver).unwrap();

        (
            Shared {},
            Local {
                device,
                stack,
                udp_handle,
                ssdp,
                sender,
            },
        )
    }

    #[task(local = [device, stack, udp_handle, ssdp], priority = 2)]
    async fn network_task(cx: network_task::Context, mut receiver: Receiver) {
        let (device, stack, udp_handle, ssdp) = (
            cx.local.device,
            cx.local.stack,
            cx.local.udp_handle,
            cx.local.ssdp,
        );

        loop {
            let now = now_fn();
            let old_ip = stack.interface.ipv4_addr();
            let next = stack.poll(now, &mut &mut device.dma);
            let new_ip = stack.interface.ipv4_addr();
            let socket = stack.socket_set.get_mut::<udp::Socket>(*udp_handle);

            if let (None, Some(ip)) = (old_ip, new_ip) {
                let ws = WrappedSocket::new(socket);
                ssdp.on_new_addr_event(
                    &cotton_netif::InterfaceIndex(
                        core::num::NonZeroU32::new(1).unwrap(),
                    ),
                    &core::net::IpAddr::V4(ip).into(),
                    &ws,
                );

                defmt::println!("Refreshing!");
                ssdp.reset_refresh_timer(now);
            }

            if let Some(wasto) = new_ip {
                let wasto = wire::IpAddress::Ipv4(wasto);
                if let Ok((slice, sender)) = socket.recv() {
                    defmt::println!(
                        "{} from {}",
                        slice.len(),
                        sender.endpoint
                    );
                    ssdp.on_data(
                        slice,
                        wasto.into(),
                        core::net::SocketAddr::new(
                            sender.endpoint.addr.into(),
                            sender.endpoint.port,
                        ),
                        now,
                    );
                }
            }

            if ssdp.poll_timeout() <= now {
                let ws = WrappedSocket::new(socket);
                ssdp.handle_timeout(&ws, now);
            }

            let mut next_wake = ssdp.poll_timeout();
            if let Some(duration) = next {
                next_wake = next_wake.min(now + duration);
            }
            defmt::println!(
                "Waking at {}ms now {}ms",
                next_wake.total_millis(),
                now.total_millis()
            );

            // convert smoltcp::Instant to fugit::Instant
            let _ = Mono::timeout_at(
                <Mono as rtic_monotonics::Monotonic>::Instant::from_ticks(
                    next_wake.total_millis() as u64,
                ),
                receiver.recv(),
            )
            .await;
        }
    }

    #[task(binds = ETH, local = [sender])]
    fn eth_interrupt(cx: eth_interrupt::Context) {
        stm32_eth::eth_interrupt_handler();
        _ = cx.local.sender.try_send(());
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

impl Default for NetworkStorage {
    fn default() -> Self {
        Self::new()
    }
}

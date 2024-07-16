//! Example RTIC (1.0) application using RP2040 + W5500 to obtain a DHCP address
//! and start doing SSDP
#![no_std]
#![no_main]

extern crate alloc;

use core::ptr;
use defmt_rtt as _; // global logger
use defmt_rtt as _;
use embedded_alloc::Heap;
use panic_probe as _;
use rp_pico as _; // includes boot2
use smoltcp::iface::{self, SocketStorage};

#[global_allocator]
static ALLOCATOR: Heap = Heap::empty();

/// Set up the heap
///
/// As is standard, all memory above the rodata segment and below the
/// stack, is used as heap.
pub fn init_heap() {
    const STACK_SIZE: usize = 16 * 1024;
    // SAFETY: this relies on the link map being correct, and STACK_SIZE
    // being large enough for the entire program.
    unsafe {
        extern "C" {
            static mut __sheap: u32;
            static mut _stack_start: u32;
        }

        let heap_start = ptr::addr_of!(__sheap) as usize;
        let heap_end = ptr::addr_of!(_stack_start) as usize;
        let heap_size = heap_end - heap_start - STACK_SIZE;
        ALLOCATOR.init(heap_start, heap_size);
    }
}

#[rtic::app(device = rp_pico::hal::pac, peripherals = true, dispatchers = [ADC_IRQ_FIFO])]
mod app {
    use crate::alloc::string::ToString;
    use crate::NetworkStorage;
    use cotton_ssdp::refresh_timer::SmoltcpTimebase;
    use cotton_ssdp::udp::smoltcp::{
        GenericIpAddress, GenericIpv4Address, GenericSocketAddr,
        WrappedInterface, WrappedSocket,
    };
    use cross_rp2040_w5500::smoltcp::Stack;
    use fugit::ExtU64;
    use rp2040_hal::gpio::bank0::Gpio21;
    use rp2040_hal::gpio::FunctionSio;
    use rp2040_hal::gpio::Interrupt::EdgeLow;
    use rp2040_hal::gpio::PullUp;
    use rp2040_hal::gpio::SioInput;
    use rp_pico::pac;
    use smoltcp::{iface::SocketHandle, socket::udp, wire};
    use systick_monotonic::Systick;

    #[shared]
    struct Shared {}

    pub struct Listener {}

    #[local]
    struct Local {
        device: cotton_w5500::smoltcp::w5500_evb_pico::Device,
        stack: Stack,
        udp_handle: SocketHandle,
        ssdp: cotton_ssdp::engine::Engine<Listener, SmoltcpTimebase>,
        nvic: cortex_m::peripheral::NVIC,
        w5500_irq:
            rp2040_hal::gpio::Pin<Gpio21, FunctionSio<SioInput>, PullUp>,
        timer_handle: Option<periodic::SpawnHandle>,
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

    #[monotonic(binds = SysTick, default = true)]
    type Monotonic = Systick<1000>;

    fn now_fn() -> smoltcp::time::Instant {
        let time = monotonics::now().duration_since_epoch().ticks();
        smoltcp::time::Instant::from_millis(time as i64)
    }

    #[init(local = [ storage: NetworkStorage = NetworkStorage::new() ])]
    fn init(c: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::println!("Pre-init");
        super::init_heap();
        let mut setup =
            cross_rp2040_w5500::setup::BasicSetup::new(c.device, c.core.SYST);
        defmt::println!("MAC address: {:x}", setup.mac_address);

        let (w5500_spi, w5500_irq) = cross_rp2040_w5500::setup::spi_setup(
            setup.pins,
            setup.spi0,
            &mut setup.timer,
            &setup.clocks,
            &mut setup.resets,
        );

        let bus = w5500::bus::FourWire::new(w5500_spi);
        w5500_irq.set_interrupt_enabled(EdgeLow, true);
        unsafe {
            pac::NVIC::unmask(pac::Interrupt::IO_IRQ_BANK0);
        }

        let mut device =
            cotton_w5500::smoltcp::Device::new(bus, &setup.mac_address);

        device.enable_interrupt();

        let mut stack = Stack::new(
            &mut device,
            &setup.unique_id,
            &setup.mac_address,
            &mut c.local.storage.sockets[..],
            now_fn(),
        );
        stack.poll(now_fn(), &mut device);

        let udp_rx_buffer = udp::PacketBuffer::new(
            &mut c.local.storage.rx_metadata[..],
            &mut c.local.storage.rx_storage[..],
        );

        let udp_tx_buffer = udp::PacketBuffer::new(
            &mut c.local.storage.tx_metadata[..],
            &mut c.local.storage.tx_storage[..],
        );
        let mut udp_socket = udp::Socket::new(udp_rx_buffer, udp_tx_buffer);
        _ = udp_socket.bind(1900);
        let random_seed = setup.unique_id.id(b"ssdp-refresh") as u32;
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
            let wi = WrappedInterface::new(
                &mut stack.interface,
                &mut device,
                now_fn(),
            );
            let ws = WrappedSocket::new(&mut udp_socket);
            _ = ssdp.on_network_event(&ev, &wi, &ws);

            ssdp.subscribe(
                "cotton-test-server-rp2040".to_string(),
                Listener {},
                &ws,
            );

            let uuid = alloc::format!(
                "{:032x}",
                cotton_unique::uuid(&setup.unique_id, b"upnp")
            );

            ssdp.advertise(
                uuid,
                cotton_ssdp::Advertisement {
                    notification_type: "rp2040-w5500-test".to_string(),
                    location: "http://127.0.0.1/".to_string(),
                },
                &ws,
            );
        }

        let udp_handle = stack.socket_set.add(udp_socket);

        let timer_handle = Some(periodic::spawn_after(2.secs()).unwrap());

        (
            Shared {},
            Local {
                device,
                stack,
                udp_handle,
                ssdp,
                nvic: c.core.NVIC,
                w5500_irq,
                timer_handle,
            },
            init::Monotonics(setup.mono),
        )
    }

    #[task(local = [nvic])]
    fn periodic(_cx: periodic::Context) {
        cortex_m::peripheral::NVIC::pend(pac::Interrupt::IO_IRQ_BANK0);
    }

    #[task(binds = IO_IRQ_BANK0, local = [w5500_irq, device, stack, udp_handle, ssdp, timer_handle], priority = 2)]
    fn eth_interrupt(cx: eth_interrupt::Context) {
        let (stack, udp_handle, ssdp) =
            (cx.local.stack, cx.local.udp_handle, cx.local.ssdp);

        cx.local.device.clear_interrupt();
        cx.local.w5500_irq.clear_interrupt(EdgeLow);

        let now = now_fn();
        let old_ip = stack.interface.ipv4_addr();
        let next = stack.poll(now, cx.local.device);
        let new_ip = stack.interface.ipv4_addr();
        let socket = stack.socket_set.get_mut::<udp::Socket>(*udp_handle);

        if let (None, Some(ip)) = (old_ip, new_ip) {
            let ws = WrappedSocket::new(socket);
            ssdp.on_new_addr_event(
                &cotton_netif::InterfaceIndex(
                    core::num::NonZeroU32::new(1).unwrap(),
                ),
                &no_std_net::IpAddr::V4(GenericIpv4Address::from(ip).into()),
                &ws,
            );

            defmt::println!("Refreshing!");
            ssdp.reset_refresh_timer(now);
        }

        if let Some(wasto) = new_ip {
            let wasto = wire::IpAddress::Ipv4(wasto);
            if let Ok((slice, sender)) = socket.recv() {
                defmt::println!("{} from {}", slice.len(), sender.endpoint);
                ssdp.on_data(
                    slice,
                    GenericIpAddress::from(wasto).into(),
                    GenericSocketAddr::from(sender.endpoint).into(),
                    now,
                );
            }
        }

        while ssdp.poll_timeout() <= now {
            let ws = WrappedSocket::new(socket);
            ssdp.handle_timeout(&ws, now);
        }
        let mut next_wake = ssdp.poll_timeout() - now;
        if let Some(duration) = next {
            next_wake = next_wake.min(duration);
        }
        defmt::println!("Waking after {}ms", next_wake.total_millis());
        let _ = periodic::spawn_after(next_wake.total_millis().millis());
    }
}

/// All storage required for networking
struct NetworkStorage {
    sockets: [iface::SocketStorage<'static>; 2],
    rx_metadata: [smoltcp::socket::udp::PacketMetadata; 16],
    rx_storage: [u8; 8192],
    tx_metadata: [smoltcp::socket::udp::PacketMetadata; 8],
    tx_storage: [u8; 2048],
}

impl NetworkStorage {
    const fn new() -> Self {
        NetworkStorage {
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

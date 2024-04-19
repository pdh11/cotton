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
    use cotton_ssdp::udp::smoltcp::{
        GenericIpAddress, GenericIpv4Address, GenericSocketAddr,
        WrappedInterface, WrappedSocket,
    };
    use cross_rp2040_w5500::{smoltcp::RefreshTimer, smoltcp::Stack};
    use embedded_hal::delay::DelayNs;
    use embedded_hal::digital::OutputPin;
    use fugit::ExtU64;
    use rp2040_hal as hal;
    use rp2040_hal::fugit::RateExtU32;
    use rp2040_hal::gpio::bank0::Gpio21;
    use rp2040_hal::gpio::FunctionSio;
    use rp2040_hal::gpio::Interrupt::EdgeLow;
    use rp2040_hal::gpio::PinState;
    use rp2040_hal::gpio::PullDown;
    use rp2040_hal::gpio::PullNone;
    use rp2040_hal::gpio::PullUp;
    use rp2040_hal::gpio::SioInput;
    use rp2040_hal::Clock;
    use rp_pico::pac;
    use rp_pico::XOSC_CRYSTAL_FREQ;
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
        ssdp: cotton_ssdp::engine::Engine<Listener>,
        nvic: cortex_m::peripheral::NVIC,
        w5500_irq:
            rp2040_hal::gpio::Pin<Gpio21, FunctionSio<SioInput>, PullUp>,
        timer_handle: Option<periodic::SpawnHandle>,
        refresh_timer: RefreshTimer,
        uuid: uuid::Uuid,
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
        let unique_id = unsafe { cotton_unique::rp2040::unique_flash_id() };
        let mac = cotton_unique::mac_address(&unique_id, b"w5500-spi0");
        defmt::println!("MAC address: {:x}", mac);

        //*******
        // Initialization of the system clock.

        let mut resets = c.device.RESETS;
        let mut watchdog = hal::watchdog::Watchdog::new(c.device.WATCHDOG);

        // Configure the clocks - The default is to generate a 125 MHz system clock
        let clocks = hal::clocks::init_clocks_and_plls(
            XOSC_CRYSTAL_FREQ,
            c.device.XOSC,
            c.device.CLOCKS,
            c.device.PLL_SYS,
            c.device.PLL_USB,
            &mut resets,
            &mut watchdog,
        )
        .ok()
        .unwrap();

        let mut timer = hal::Timer::new(c.device.TIMER, &mut resets, &clocks);
        let mono = Systick::new(c.core.SYST, clocks.system_clock.freq().raw());

        let sio = hal::Sio::new(c.device.SIO);
        let pins = rp_pico::Pins::new(
            c.device.IO_BANK0,
            c.device.PADS_BANK0,
            sio.gpio_bank0,
            &mut resets,
        );

        // W5500-EVB-Pico:
        //   W5500 SPI on SPI0
        //         nCS = GPIO17
        //         TX (MOSI) = GPIO19
        //         RX (MISO) = GPIO16
        //         SCK = GPIO18
        //   W5500 INTn on GPIO21
        //   W5500 RSTn on GPIO20
        //   Green LED on GPIO25

        let mut w5500_rst = pins
            .gpio20
            .into_pull_type::<PullNone>()
            .into_push_pull_output_in_state(PinState::Low);
        timer.delay_ms(2);
        let _ = w5500_rst.set_high();
        timer.delay_ms(2);

        let spi_ncs = pins
            .gpio17
            .into_pull_type::<PullNone>()
            .into_push_pull_output();
        let spi_mosi = pins
            .gpio19
            .into_pull_type::<PullNone>()
            .into_function::<hal::gpio::FunctionSpi>();
        let spi_miso = pins
            .gpio16
            .into_pull_type::<PullDown>()
            .into_function::<hal::gpio::FunctionSpi>();
        let spi_sclk = pins
            .gpio18
            .into_pull_type::<PullNone>()
            .into_function::<hal::gpio::FunctionSpi>();
        let spi = hal::spi::Spi::<_, _, _, 8>::new(
            c.device.SPI0,
            (spi_mosi, spi_miso, spi_sclk),
        );

        let spi = spi.init(
            &mut resets,
            clocks.peripheral_clock.freq(),
            16u32.MHz(),
            hal::spi::FrameFormat::MotorolaSpi(embedded_hal::spi::MODE_0),
        );

        let bus = w5500::bus::FourWire::new(spi, spi_ncs);

        let w5500_irq = pins.gpio21.into_pull_up_input();
        w5500_irq.set_interrupt_enabled(EdgeLow, true);

        unsafe {
            pac::NVIC::unmask(pac::Interrupt::IO_IRQ_BANK0);
        }

        let mut device = cotton_w5500::smoltcp::Device::new(bus, &mac);

        device.enable_interrupt();

        let mut stack = Stack::new(
            &mut device,
            &unique_id,
            &mac,
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
            let wi = WrappedInterface::new(
                &mut stack.interface,
                &mut device,
                now_fn(),
            );
            let ws = WrappedSocket::new(&mut udp_socket);
            _ = ssdp.on_network_event(&ev, &wi, &ws);
        }
        let uuid = cotton_unique::uuid(&unique_id, b"upnp");
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
                refresh_timer: RefreshTimer::new(now_fn()),
                uuid,
            },
            init::Monotonics(mono),
        )
    }

    #[task(local = [nvic])]
    fn periodic(_cx: periodic::Context) {
        cortex_m::peripheral::NVIC::pend(pac::Interrupt::IO_IRQ_BANK0);
    }

    #[task(binds = IO_IRQ_BANK0, local = [w5500_irq, device, stack, udp_handle, ssdp, timer_handle, refresh_timer, uuid], priority = 2)]
    fn eth_interrupt(cx: eth_interrupt::Context) {
        let (stack, udp_handle, ssdp, refresh_timer, uuid) = (
            cx.local.stack,
            cx.local.udp_handle,
            cx.local.ssdp,
            cx.local.refresh_timer,
            cx.local.uuid,
        );
        cx.local.device.clear_interrupt();
        cx.local.w5500_irq.clear_interrupt(EdgeLow);
        defmt::println!("ETH IRQ");

        let now = now_fn();
        let old_ip = stack.interface.ipv4_addr();
        let next = stack.poll(now, cx.local.device);
        let new_ip = stack.interface.ipv4_addr();

        let mut do_refresh = false;
        let mut next_wake = refresh_timer.next_refresh(now);
        if next_wake == smoltcp::time::Duration::ZERO {
            do_refresh = true;
            refresh_timer.update_refresh(now);
            next_wake = refresh_timer.next_refresh(now);
        }

        if let Some(duration) = next {
            next_wake = next_wake.min(duration);
        }

        let next_wake = next_wake.total_millis();
        let next_wake = next_wake.millis();
        let _ = periodic::spawn_after(next_wake);

        let socket = stack.socket_set.get_mut::<udp::Socket>(*udp_handle);
        let ws = WrappedSocket::new(socket);

        if let (None, Some(ip)) = (old_ip, new_ip) {
            ssdp.on_new_addr_event(
                &cotton_netif::InterfaceIndex(
                    core::num::NonZeroU32::new(1).unwrap(),
                ),
                &no_std_net::IpAddr::V4(GenericIpv4Address::from(ip).into()),
                &ws,
            );

            ssdp.subscribe("ssdp:all".to_string(), Listener {}, &ws);

            ssdp.advertise(
                alloc::format!("{:032x}", uuid),
                cotton_ssdp::Advertisement {
                    notification_type: "rp2040-w5500-test".to_string(),
                    location: "http://127.0.0.1/".to_string(),
                },
                &ws,
            );
        } else if do_refresh {
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
                    defmt::println!("{} from {}", size, sender);
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

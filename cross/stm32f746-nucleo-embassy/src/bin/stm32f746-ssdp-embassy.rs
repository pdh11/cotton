#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::ToString;
use core::ops::AddAssign;
use cotton_netif::InterfaceIndex;
use cotton_ssdp::udp::smoltcp::{
    GenericIpAddress, GenericIpv4Address, GenericSocketAddr,
};
use defmt::{println, unwrap};
use embassy_executor::Spawner;
use embassy_futures::block_on;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{Stack, StackResources};
use embassy_stm32::eth::generic_smi::GenericSMI;
use embassy_stm32::eth::{Ethernet, PacketQueue};
use embassy_stm32::peripherals::ETH;
use embassy_stm32::rng::Rng;
use embassy_stm32::time::Hertz;
use embassy_stm32::{bind_interrupts, eth, peripherals, rng, Config};
use embassy_time::WithTimeout;
use linked_list_allocator::LockedHeap;
use core::net::IpAddr;
use rand_core::RngCore;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

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
        let heap_start = core::ptr::addr_of!(__sheap) as usize;
        let heap_end = core::ptr::addr_of!(_stack_start) as usize;
        let heap_size = heap_end - heap_start - STACK_SIZE;
        allocator.lock().init(heap_start as *mut u8, heap_size);
    }
}

bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
    // The interrupt is called "HASH_RNG" on crypto-capable STM32s
    // (such as STM32F756), but just "RNG" on others (like our
    // STM32F746)
    RNG => rng::InterruptHandler<peripherals::RNG>;
});

type Device = Ethernet<'static, ETH, GenericSMI>;

pub struct Listener {}

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

/// A Timebase based on Embassy time
///
/// We can use Embassy's Instant directly, but must make a newtype for
/// Duration because the Embassy one isn't
/// `From<core::time::Duration>`.
struct EmbassyTimebase;

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
struct EmbassyDuration(pub embassy_time::Duration);

impl From<core::time::Duration> for EmbassyDuration {
    fn from(d: core::time::Duration) -> Self {
        Self(embassy_time::Duration::from_ticks(d.as_millis() as u64))
    }
}

impl AddAssign<EmbassyDuration> for embassy_time::Instant {
    fn add_assign(&mut self, d: EmbassyDuration) {
        *self += d.0;
    }
}

impl cotton_ssdp::refresh_timer::Timebase for EmbassyTimebase {
    type Duration = EmbassyDuration;
    type Instant = embassy_time::Instant;
}

struct WrappedStack<'a, D: embassy_net::driver::Driver> {
    stack: &'a Stack<D>,
}

impl<'a, D: embassy_net::driver::Driver> WrappedStack<'a, D> {
    pub const fn new(stack: &'a Stack<D>) -> Self {
        Self { stack }
    }
}

impl<D: embassy_net::driver::Driver> cotton_ssdp::udp::Multicast
    for WrappedStack<'_, D>
{
    fn join_multicast_group(
        &self,
        multicast_address: &IpAddr,
        _interface: InterfaceIndex,
    ) -> Result<(), cotton_ssdp::udp::error::Error> {
        let ip: embassy_net::IpAddress =
            GenericIpAddress::from(*multicast_address).into();

        // @todo This block_on isn't very idiomatic for Embassy
        block_on(self.stack.join_multicast_group(ip))
            .map(|_| ())
            .map_err(|e| {
                cotton_ssdp::udp::error::Error::SmoltcpMulticast(
                    cotton_ssdp::udp::Syscall::JoinMulticast,
                    e,
                )
            })
    }

    fn leave_multicast_group(
        &self,
        _multicast_address: &IpAddr,
        _interface: InterfaceIndex,
    ) -> Result<(), cotton_ssdp::udp::error::Error> {
        todo!();
    }
}

struct WrappedSocket<'a, 'b> {
    socket: &'a mut UdpSocket<'b>,
}

impl<'a, 'b> WrappedSocket<'a, 'b> {
    pub fn new(socket: &'a mut UdpSocket<'b>) -> Self {
        Self { socket }
    }
}

impl cotton_ssdp::udp::TargetedSend for WrappedSocket<'_, '_> {
    fn send_with<F>(
        &self,
        size: usize,
        to: &core::net::SocketAddr,
        _from: &core::net::IpAddr,
        f: F,
    ) -> Result<(), cotton_ssdp::udp::error::Error>
    where
        F: FnOnce(&mut [u8]) -> usize,
    {
        // @todo This buffer/copy is undesirable
        //
        // send_with is coming in the next (0.5?) version of embassy-net
        let mut buf = [0u8; 1520];

        if size > buf.len() {
            return Err(cotton_ssdp::udp::error::Error::NotImplemented); // not quite right
        }
        let size = f(&mut buf);
        let ep: embassy_net::IpEndpoint = GenericSocketAddr::from(*to).into();

        // @todo This block_on isn't very idiomatic for Embassy
        block_on(self.socket.send_to(&buf[0..size], ep))
            .map(|_| ())
            .map_err(|_| cotton_ssdp::udp::error::Error::NotImplemented)
    }
}

#[embassy_executor::task]
async fn net_task(stack: &'static Stack<Device>) -> ! {
    stack.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hse = Some(Hse {
            freq: Hertz(8_000_000),
            mode: HseMode::Oscillator,
        });
        config.rcc.pll_src = PllSource::HSE;
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV4,
            mul: PllMul::MUL216,
            divp: Some(PllPDiv::DIV2), // 8mhz / 4 * 216 / 2 = 216Mhz
            divq: None,
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
        config.rcc.sys = Sysclk::PLL1_P;
    }
    let p = embassy_stm32::init(config);
    init_heap(&ALLOCATOR);

    println!("Hello World!");

    let unique_id =
        cotton_unique::stm32::unique_chip_id(embassy_stm32::uid::uid());

    let mac_addr = cotton_unique::mac_address(&unique_id, b"stm32-eth");

    // Generate random seed.
    let mut rng = Rng::new(p.RNG, Irqs);
    let mut seed = [0; 8];
    rng.fill_bytes(&mut seed);
    let seed = u64::from_le_bytes(seed);

    static PACKETS: StaticCell<PacketQueue<4, 4>> = StaticCell::new();
    let device = Ethernet::new(
        PACKETS.init(PacketQueue::<4, 4>::new()),
        p.ETH,
        Irqs,
        p.PA1,
        p.PA2,
        p.PC1,
        p.PA7,
        p.PC4,
        p.PC5,
        p.PG13,
        p.PB13,
        p.PG11,
        GenericSMI::new(0),
        mac_addr,
    );

    let config = embassy_net::Config::dhcpv4(Default::default());

    // Init network stack
    static STACK: StaticCell<Stack<Device>> = StaticCell::new();
    static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
    let stack = &*STACK.init(Stack::new(
        device,
        config,
        RESOURCES.init(StackResources::new()),
        seed,
    ));

    // Launch network task
    unwrap!(spawner.spawn(net_task(stack)));

    // Ensure DHCP configuration is up before trying connect
    stack.wait_config_up().await;

    println!("Network task initialized");

    // Then we can use it!
    let mut ssdp =
        cotton_ssdp::engine::Engine::<Listener, EmbassyTimebase>::new(
            seed as u32,
            embassy_time::Instant::now(),
        );

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    let mut rx_meta = [PacketMetadata::EMPTY; 16];
    let mut tx_meta = [PacketMetadata::EMPTY; 16];
    let mut buf = [0; 4096];
    let mut udp_socket = UdpSocket::new(
        stack,
        &mut rx_meta,
        &mut rx_buffer,
        &mut tx_meta,
        &mut tx_buffer,
    );
    _ = udp_socket.bind(1900);

    let ix =
        cotton_netif::InterfaceIndex(core::num::NonZeroU32::new(1).unwrap());
    let ev = cotton_netif::NetworkEvent::NewLink(
        ix,
        "".to_string(),
        cotton_netif::Flags::UP
            | cotton_netif::Flags::RUNNING
            | cotton_netif::Flags::MULTICAST,
    );

    {
        let wi = WrappedStack::new(stack);
        let ws = WrappedSocket::new(&mut udp_socket);
        _ = ssdp.on_network_event(&ev, &wi, &ws);

        if let Some(ip) = stack.config_v4().map(|cfg| cfg.address.address()) {
            ssdp.on_new_addr_event(
                &cotton_netif::InterfaceIndex(
                    core::num::NonZeroU32::new(1).unwrap(),
                ),
                &core::net::IpAddr::V4(GenericIpv4Address::from(ip).into()),
                &ws,
            );
        }

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
    loop {
        println!("loop");
        let p = ssdp.poll_timeout();
        let r = udp_socket.recv_from(&mut buf).with_deadline(p).await;
        let now = embassy_time::Instant::now();

        if let Ok(Ok((n, wasfrom))) = r {
            if let Some(wasto) =
                stack.config_v4().map(|cfg| cfg.address.address())
            {
                ssdp.on_data(
                    &buf[0..n],
                    GenericIpAddress::from(embassy_net::IpAddress::Ipv4(
                        wasto,
                    ))
                    .into(),
                    GenericSocketAddr::from(wasfrom).into(),
                    now,
                )
            }
        } else {
            ssdp.handle_timeout(&WrappedSocket::new(&mut udp_socket), now);
        }
    }
}

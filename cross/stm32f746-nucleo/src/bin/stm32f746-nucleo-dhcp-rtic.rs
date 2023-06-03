//! On an STM32F746-Nucleo, bring up Ethernet and TCP and obtain a DHCP address
#![no_std]
#![no_main]

use defmt_rtt as _; // global logger
use panic_probe as _;
use smoltcp::iface::{self, SocketStorage};
use stm32_eth::dma::{RxRingEntry, TxRingEntry};
use stm32f7xx_hal as _;

#[rtic::app(device = stm32_eth::stm32, dispatchers = [SPI1])]
mod app {
    use super::NetworkStorage;
    use cotton_stm32f746_nucleo::common::*;
    use fugit::ExtU64;
    use stm32_eth::dma::EthernetDMA;
    use systick_monotonic::Systick;

    #[local]
    struct Local {
        device: Stm32Ethernet,
        stack: Stack<'static>,
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

        let clocks = setup_clocks(RCC);
        let mono = Systick::new(core.SYST, clocks.hclk().raw());

        let mut device = Stm32Ethernet::new(
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

        let mac_address = mac_address();
        // NB stm32-eth implements smoltcp::Device not for
        // EthernetDMA, but for "&mut EthernetDMA"
        let mut stack = Stack::new(
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

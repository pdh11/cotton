//! Example RTIC (1.0) application using RP2040 + W5500 to obtain a DHCP address
#![no_std]
#![no_main]
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

use defmt_rtt as _; // global logger
use panic_probe as _;
use rp_pico as _; // includes boot2

#[rtic::app(device = rp_pico::hal::pac, peripherals = true, dispatchers = [ADC_IRQ_FIFO])]
mod app {
    use embedded_hal::delay::DelayNs;
    use fugit::ExtU64;
    use rp2040_hal::gpio::Interrupt::EdgeLow;
    use rp_pico::pac;
    use systick_monotonic::Systick;
    use w5500_dhcp::{hl::Hostname, Client as DhcpClient};
    use w5500_ll::eh1::vdm::W5500;
    use w5500_ll::{LinkStatus, OperationMode, PhyCfg, Registers, Sn};

    const DHCP_SN: Sn = Sn::Sn0;
    const NAME: &str = "rp2040-w5500";
    const HOSTNAME: Hostname<'static> = Hostname::new_unwrapped(NAME);

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        nvic: cortex_m::peripheral::NVIC,
        w5500: W5500<cotton_w5500::smoltcp::w5500_evb_pico::SpiDevice>,
        w5500_irq: cotton_w5500::smoltcp::w5500_evb_pico::IrqPin,
        dhcp: DhcpClient<'static>,
    }

    #[monotonic(binds = SysTick, default = true)]
    type Monotonic = Systick<1000>;

    #[init(local = [usb_bus: Option<u32> = None])]
    fn init(c: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::println!("Pre-init");
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
        let mac = w5500_ll::net::Eui48Addr {
            octets: setup.mac_address,
        };

        let mut w5500 = W5500::new(w5500_spi);

        let _phy_cfg: PhyCfg = 'outer: loop {
            // sanity check W5500 communications
            assert_eq!(w5500.version().unwrap(), w5500_dhcp::ll::VERSION);

            // load the MAC address we got from EEPROM
            w5500.set_shar(&mac).unwrap();
            debug_assert_eq!(w5500.shar().unwrap(), mac);

            // wait for the PHY to indicate the Ethernet link is up
            let mut attempts: u32 = 0;
            defmt::println!("Polling for link up");
            const PHY_CFG: PhyCfg =
                PhyCfg::DEFAULT.set_opmdc(OperationMode::FullDuplex10bt);
            w5500.set_phycfgr(PHY_CFG).unwrap();

            const LINK_UP_POLL_PERIOD_MILLIS: u32 = 100;
            const LINK_UP_POLL_ATTEMPTS: u32 = 50;
            loop {
                let phy_cfg: PhyCfg = w5500.phycfgr().unwrap();
                if phy_cfg.lnk() == LinkStatus::Up {
                    break 'outer phy_cfg;
                }
                if attempts >= LINK_UP_POLL_ATTEMPTS {
                    defmt::println!(
                        "Failed to link up in {} ms",
                        attempts * LINK_UP_POLL_PERIOD_MILLIS,
                    );
                    break;
                }
                setup.timer.delay_ms(LINK_UP_POLL_PERIOD_MILLIS);
                attempts += 1;
            }
        };
        defmt::println!("Done link up");

        let seed: u64 = u64::from(cortex_m::peripheral::SYST::get_current())
            << 32
            | u64::from(cortex_m::peripheral::SYST::get_current());

        let dhcp = DhcpClient::new(DHCP_SN, seed, mac, HOSTNAME);
        dhcp.setup_socket(&mut w5500).unwrap();

        periodic::spawn_after(1.secs()).unwrap();

        (
            Shared {},
            Local {
                nvic: c.core.NVIC,
                w5500,
                w5500_irq,
                dhcp,
            },
            init::Monotonics(setup.mono),
        )
    }

    #[task(local = [nvic])]
    fn periodic(_cx: periodic::Context) {
        cortex_m::peripheral::NVIC::pend(pac::Interrupt::IO_IRQ_BANK0);
        periodic::spawn_after(1.secs()).unwrap();
    }

    #[task(binds = IO_IRQ_BANK0, local = [w5500, w5500_irq, dhcp], priority = 2)]
    fn eth_interrupt(cx: eth_interrupt::Context) {
        defmt::println!("ETH IRQ");
        let (w5500, w5500_irq, dhcp) =
            (cx.local.w5500, cx.local.w5500_irq, cx.local.dhcp);
        let had_lease = dhcp.has_lease();
        let now: u32 = monotonics::now()
            .duration_since_epoch()
            .to_secs()
            .try_into()
            .unwrap();
        let _spawn_after_secs: u32 = dhcp.process(w5500, now).unwrap();

        if dhcp.has_lease() && !had_lease {
            defmt::println!("DHCP succeeded! {}", dhcp.leased_ip())
        }

        w5500_irq.clear_interrupt(EdgeLow);
    }
}

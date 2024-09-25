[![CI status](https://github.com/pdh11/cotton/actions/workflows/ci.yml/badge.svg)](https://github.com/pdh11/cotton/actions)
[![codecov](https://codecov.io/gh/pdh11/cotton/branch/main/graph/badge.svg?token=SMSZEPGRHA)](https://codecov.io/gh/pdh11/cotton)
[![dependency status](https://deps.rs/repo/github/pdh11/cotton/status.svg)](https://deps.rs/repo/github/pdh11/cotton)
[![License: CC0-1.0](https://img.shields.io/badge/License-CC0_1.0-lightgrey.svg)](http://creativecommons.org/publicdomain/zero/1.0/)

# Cotton

A collection of Rust crates for low-level networking functionality.

So far:

 - [cotton-netif](https://crates.io/crates/cotton-netif)
   [![Crates.io](https://img.shields.io/crates/v/cotton-netif)](https://crates.io/crates/cotton-netif)
   [![Crates.io](https://img.shields.io/crates/d/cotton-netif)](https://crates.io/crates/cotton-netif)
   [![docs.rs](https://img.shields.io/docsrs/cotton-netif)](https://docs.rs/cotton-netif/latest/cotton_netif/): enumerating
   available network interfaces and their IP addresses, including
   ongoing (asynchronous) comings and goings of network interfaces
   (e.g. on USB hotplug/unplug); so far, for Linux only.

 - [cotton-ssdp](https://crates.io/crates/cotton-ssdp)
   [![Crates.io](https://img.shields.io/crates/v/cotton-ssdp)](https://crates.io/crates/cotton-ssdp)
   [![Crates.io](https://img.shields.io/crates/d/cotton-ssdp)](https://crates.io/crates/cotton-ssdp)
   [![docs.rs](https://img.shields.io/docsrs/cotton-ssdp)](https://docs.rs/cotton-ssdp/latest/cotton_ssdp/): implementing
   SSDP, the Simple Service Discovery Protocol, a mechanism for
   discovering available resources (service) on a local network. Uses
   cotton-netif, in order to do the Right Thing on multi-homed hosts
   (but meaning that it is unlikely to work on Windows platforms).

 - [cotton-unique](https://crates.io/crates/cotton-unique)
   [![Crates.io](https://img.shields.io/crates/v/cotton-unique)](https://crates.io/crates/cotton-unique)
   [![Crates.io](https://img.shields.io/crates/d/cotton-unique)](https://crates.io/crates/cotton-unique)
   [![docs.rs](https://img.shields.io/docsrs/cotton-unique)](https://docs.rs/cotton-unique/latest/cotton_unique/): creating deterministic but per-device unique
   identifiers such as MAC addresses.

 - [cotton-w5500](https://crates.io/crates/cotton-w5500)
   [![Crates.io](https://img.shields.io/crates/v/cotton-w5500)](https://crates.io/crates/cotton-w5500)
   [![Crates.io](https://img.shields.io/crates/d/cotton-w5500)](https://crates.io/crates/cotton-w5500)
   [![docs.rs](https://img.shields.io/docsrs/cotton-w5500)](https://docs.rs/cotton-w5500/latest/cotton_w5500/): smoltcp driver for the Wiznet W5500 Ethernet
   controller in MACRAW mode, including interrupt-driven mode.

These crates are `no_std`-compatible, meaning that they can be used on
embedded systems. In fact, all pushes to my local (not Github)
continuous-integration server are *automatically* tested on both STM32
and RP2040 platforms. You can read about how that is set up on my
blog: *[Part
one](https://pdh11.blogspot.com/2024/02/system-testing-embedded-code-in-rust.html),
[Part two](https://pdh11.blogspot.com/2024/03/system-tests-2.html),
[Part three](https://pdh11.blogspot.com/2024/04/blog-post.html)*.

These system-tests also serve as example code combining the Cotton
crates with the wider ecosystem, including examples where the
combining of the wider ecosystem components needed a little research
in its own right even before involving Cotton, so perhaps that in
itself will be useful to others:

  - [stm32f746-nucleo-hello](https://github.com/pdh11/cotton/blob/main/cross/stm32f746-nucleo/src/bin/stm32f746-nucleo-hello.rs):
    basic test that an attached STM32F746-Nucleo development board is
    working correctly; no-alloc;

  - [stm32f746-nucleo-dhcp-rtic](https://github.com/pdh11/cotton/blob/main/cross/stm32f746-nucleo/src/bin/stm32f746-nucleo-dhcp-rtic.rs):
    combining [RTIC (1.x)](https://rtic.rs/1/book/en/) +
    [stm32-eth](https://crates.io/crates/stm32-eth/) +
    [smoltcp](https://crates.io/crates/smoltcp) +
    cotton-unique (a.k.a. how *not* to have a hardcoded,
    made-up MAC address!); no-alloc;

  - [stm32f746-nucleo-ssdp-rtic](https://github.com/pdh11/cotton/blob/main/cross/stm32f746-nucleo/src/bin/stm32f746-nucleo-dhcp-rtic.rs):
    combining RTIC + stm32-eth + smoltcp + cotton-unique + cotton-ssdp;

  - [stm32f746-nucleo-dhcp-rtic2](https://github.com/pdh11/cotton/blob/main/cross/stm32f746-nucleo-rtic2/src/bin/stm32f746-dhcp-rtic2.rs):
    combining [RTIC 2](https://rtic.rs/2/book/en/) +
    stm32-eth +
    smoltcp +
    cotton-unique; no-alloc;

  - [stm32f746-nucleo-ssdp-rtic2](https://github.com/pdh11/cotton/blob/main/cross/stm32f746-nucleo-rtic2/src/bin/stm32f746-ssdp-rtic2.rs):
    combining RTIC 2 +
    stm32-eth +
    smoltcp +
    cotton-unique +
    cotton-ssdp;

  - [rp2040-w5500-hello](https://github.com/pdh11/cotton/blob/main/cross/rp2040-w5500/src/bin/hello.rs):
    basic test that an attached W5500-Pico-EVB development board (or
    anything that equivalently wires together an RP2040 and a W5500)
    is working correctly; no-alloc;

  - [rp2040-w5500-dhcp-rtic](https://github.com/pdh11/cotton/blob/main/cross/rp2040-w5500/src/bin/rp2040-w5500-dhcp-rtic.rs):
    combining 
    [rp2040-hal](https://crates.io/crates/rp2040-hal) + RTIC +
    [w5500-hl](https://crates.io/crates/w5500-hl) +
    [w5500-dhcp](https://crates.io/crates/w5500-dhcp) + cotton-unique; no-alloc;

  - [rp2040-w5500macraw-dhcp-rtic](https://github.com/pdh11/cotton/blob/main/cross/rp2040-w5500/src/bin/rp2040-w5500macraw-dhcp-rtic.rs):
    combining rp2040-hal + RTIC +
    [w5500](https://crates.io/crates/w5500) (MACRAW mode with
    interrupts) + smoltcp + cotton-unique (note that's a *different* W5500
    crate); no-alloc;

  - [rp2040-w5500macraw-ssdp-rtic](https://github.com/pdh11/cotton/blob/main/cross/rp2040-w5500/src/bin/rp2040-w5500macraw-ssdp-rtic.rs):
    combining rp2040-hal + RTIC + w5500 (MACRAW mode with
    interrupts) + smoltcp + cotton-unique + cotton-ssdp;

My long-term goals for this project as a whole:

 - provide useful, solid, well-tested components to folks needing Rust
   crates for networking, including UPnP and embedded devices

 - develop skills in Rust coding, including the packaging,
   distributing, and publicising of it, after a career spent with C++

Everything is licensed under Creative Commons CC0, qv.

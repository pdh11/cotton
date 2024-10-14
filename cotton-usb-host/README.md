[![CI status](https://github.com/pdh11/cotton/actions/workflows/ci.yml/badge.svg)](https://github.com/pdh11/cotton/actions)
[![codecov](https://codecov.io/gh/pdh11/cotton/branch/main/graph/badge.svg?token=SMSZEPGRHA)](https://codecov.io/gh/pdh11/cotton)
[![dependency status](https://deps.rs/repo/github/pdh11/cotton/status.svg)](https://deps.rs/repo/github/pdh11/cotton)
[![Crates.io](https://img.shields.io/crates/v/cotton-usb-host)](https://crates.io/crates/cotton-usb-host)
[![Crates.io](https://img.shields.io/crates/d/cotton-usb-host)](https://crates.io/crates/cotton-usb-host)
[![docs.rs](https://img.shields.io/docsrs/cotton-usb-host)](https://docs.rs/cotton-usb-host/latest/cotton_usb-host/)
[![License: CC0-1.0](https://img.shields.io/badge/License-CC0_1.0-lightgrey.svg)](http://creativecommons.org/publicdomain/zero/1.0/)

# cotton-usb-host

Part of the [Cotton](https://github.com/pdh11/cotton) project.

## A no-std, no-alloc USB host stack for embedded devices

USB operation is _asynchronous_ and so this crate is suited for use
with embedded asynchronous executors such as [RTIC
2](https://rtic.rs/2/book/en/) and [Embassy](https://embassy.dev).

Currently supports:

 - RP2040 (USB 1.1 host)[^1]

 System-tests and examples:

 - rp2040-usb-otge100: identifying (not yet really "driving") a
   Plugable USB2-OTGE100 Ethernet adaptor (based on ASIX AX88772)

The RP2040 support is in this repo to provide a convenient worked example;
specific host-controller support for other microcontrollers probably
belongs in those microcontrollers' HAL crates.

TODO before merge:

 - [ ] Unit tests
 - [ ] Bulk in/out
 - [ ] At least one real example (MSC?)
 - [x] Interlocking to avoid contending on pipe 0
 - [ ] Review register usage for contention (buff_status?)
 - [ ] STM32?

TODO later:

 - [ ] Non-async version?
 - [ ] rp-pac vs rp2040-pac?
 - [ ] More microcontrollers

[^1]: The documentation describes this as "USB&nbsp;2.0 LS and FS" (1.5 and
12Mbits/s), but as the _only_ changes in USB&nbsp;2.0 compared to 1.1
were related to the addition of HS (480Mbits/s), it seems more honest
to describe it as USB&nbsp;1.1.

Library documentation is [on
docs.rs](https://docs.rs/cotton-usb-host/latest/cotton_usb-host/).

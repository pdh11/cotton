[![CI status](https://github.com/pdh11/cotton/actions/workflows/ci.yml/badge.svg)](https://github.com/pdh11/cotton/actions)
[![codecov](https://codecov.io/gh/pdh11/cotton/branch/main/graph/badge.svg?token=SMSZEPGRHA)](https://codecov.io/gh/pdh11/cotton)
[![dependency status](https://deps.rs/repo/github/pdh11/cotton/status.svg)](https://deps.rs/repo/github/pdh11/cotton)
[![Crates.io](https://img.shields.io/crates/v/cotton-w5500)](https://crates.io/crates/cotton-w5500)
[![Crates.io](https://img.shields.io/crates/d/cotton-w5500)](https://crates.io/crates/cotton-w5500)
[![docs.rs](https://img.shields.io/docsrs/cotton-w5500)](https://docs.rs/cotton-w5500/latest/cotton_unique/)
[![License: CC0-1.0](https://img.shields.io/badge/License-CC0_1.0-lightgrey.svg)](http://creativecommons.org/publicdomain/zero/1.0/)

# cotton-w5500

Part of the [Cotton](https://github.com/pdh11/cotton) project.

## A Wiznet W5500 driver for smoltcp

This crate includes an implementation of `smoltcp::phy::Device` which
uses the [W5500](https://crates.io/crates/w5500) crate to target
[smoltcp](https://crates.io/crates/smoltcp) to the Wiznet W5500
SPI-to-Ethernet chip, as found on the
[W5500-EVB-Pico](https://thepihut.com/products/wiznet-w5100s-evb-pico-rp2040-board-with-ethernet)
board (and in many other places). The W5500 is operated in "MACRAW"
(raw packet) mode, which allows more flexible networking (via smoltcp)
than is possible using the W5500's onboard TCP/UDP mode -- for
instance, it enables IPv6 support, which would otherwise require the
somewhat rarer W6100 chip.

Although cotton-w5500 works well with cotton-unique, it is relatively
stand-alone: it does not depend on cotton-unique nor on any other part
of the Cotton project.

Library documentation is [on
docs.rs](https://docs.rs/cotton-w5500/latest/cotton_w5500/).

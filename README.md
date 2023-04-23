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
   cotton-netif, in order to do the Right Thing on multi-homed hosts.

My long-term goals for this project as a whole:

 - provide useful, solid, well-tested components to folks needing Rust
   crates for networking, including UPnP and embedded devices

 - develop skills in Rust coding, including the packaging,
   distributing, and publicising of it, after a career spent with C++

Everything is licensed under Creative Commons CC0, qv.

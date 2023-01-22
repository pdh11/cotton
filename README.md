[![CI status](https://github.com/pdh11/cotton/actions/workflows/ci.yml/badge.svg)](https://github.com/pdh11/cotton/actions) [![codecov](https://codecov.io/gh/pdh11/cotton/branch/main/graph/badge.svg?token=SMSZEPGRHA)](https://codecov.io/gh/pdh11/cotton) [![Crates.io](https://img.shields.io/crates/v/cotton-netif)](https://crates.io/crates/cotton-netif) [![docs.rs](https://img.shields.io/docsrs/cotton-netif)](https://docs.rs/cotton-netif/latest/cotton_netif/) [![License: CC0-1.0](https://img.shields.io/badge/License-CC0_1.0-lightgrey.svg)](http://creativecommons.org/publicdomain/zero/1.0/)

# Cotton

A collection of Rust crates for low-level networking functionality.

So far:

 - [cotton-netif](https://crates.io/crates/cotton-netif): enumerating
   available network interfaces and their IP addresses, including
   ongoing (asynchronous) comings and goings of network interfaces
   (e.g. on USB hotplug/unplug); so far, for Linux only

My long-term goals for this project as a whole:

 - provide useful, solid, well-tested components to folks needing Rust
   crates for networking, including UPnP and embedded devices

 - develop skills in Rust coding, including the packaging,
   distributing, and publicising of it, after a career spent with C++

Everything is licensed under Creative Commons CCO, qv.


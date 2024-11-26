[![CI status](https://github.com/pdh11/cotton/actions/workflows/ci.yml/badge.svg)](https://github.com/pdh11/cotton/actions)
[![codecov](https://codecov.io/gh/pdh11/cotton/branch/main/graph/badge.svg?token=SMSZEPGRHA)](https://codecov.io/gh/pdh11/cotton)
[![dependency status](https://deps.rs/repo/github/pdh11/cotton/status.svg)](https://deps.rs/repo/github/pdh11/cotton)
[![Crates.io](https://img.shields.io/crates/v/cotton-usb-host-msc)](https://crates.io/crates/cotton-usb-host-msc)
[![Crates.io](https://img.shields.io/crates/d/cotton-usb-host-msc)](https://crates.io/crates/cotton-usb-host-msc)
[![docs.rs](https://img.shields.io/docsrs/cotton-usb-host-msc)](https://docs.rs/cotton-usb-host-msc/latest/cotton_usb-host-msc/)
[![License: CC0-1.0](https://img.shields.io/badge/License-CC0_1.0-lightgrey.svg)](http://creativecommons.org/publicdomain/zero/1.0/)

# cotton-usb-host-msc

Part of the [Cotton](https://github.com/pdh11/cotton) project.

## A no-std, no-alloc USB mass-storage _host_ driver for embedded devices

This crate lets you use thumb-drives and other USB storage devices from a
microcontroller such as a Raspberry&nbsp;Pi Pico.

The microcontroller here acts as the USB _host_. This crate does not
help you if you want your microcontroller to _appear_ as a storage
device when plugged into a USB host such a laptop -- that's the other
way around!

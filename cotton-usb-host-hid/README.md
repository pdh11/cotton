[![CI status](https://github.com/pdh11/cotton/actions/workflows/ci.yml/badge.svg)](https://github.com/pdh11/cotton/actions)
[![codecov](https://codecov.io/gh/pdh11/cotton/branch/main/graph/badge.svg?token=SMSZEPGRHA)](https://codecov.io/gh/pdh11/cotton)
[![dependency status](https://deps.rs/repo/github/pdh11/cotton/status.svg)](https://deps.rs/repo/github/pdh11/cotton)
[![Crates.io](https://img.shields.io/crates/v/cotton-usb-host-hid)](https://crates.io/crates/cotton-usb-host-hid)
[![Crates.io](https://img.shields.io/crates/d/cotton-usb-host-hid)](https://crates.io/crates/cotton-usb-host-hid)
[![docs.rs](https://img.shields.io/docsrs/cotton-usb-host-hid)](https://docs.rs/cotton-usb-host-hid/latest/cotton_usb-host-hid/)
[![License: CC0-1.0](https://img.shields.io/badge/License-CC0_1.0-lightgrey.svg)](http://creativecommons.org/publicdomain/zero/1.0/)

# cotton-usb-host-hid

Part of the [Cotton](https://github.com/pdh11/cotton) project.

## A no-std, no-alloc USB HID _host_ driver for embedded devices

This crate lets you use HID Devices from a microcontroller such as a
Raspberry&nbsp;Pi Pico.

The microcontroller here acts as the USB _host_. This crate does not
help you if you want your microcontroller to _appear_ as a HID
device when plugged into a USB host such a laptop -- that's the other
way around!

Currently only HID Keyboards in 'BIOS' mode are supported.

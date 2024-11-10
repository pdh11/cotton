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

This crate enables the USB host-controller peripheral on the RP2040
microcontroller, allowing USB devices (memory sticks, keyboards, hubs,
etc.) to be connected directly to the RP2040 and controlled by it.

USB operation is _asynchronous_ and so this crate is suited for use
with embedded asynchronous executors such as
[RTIC&nbsp;2](https://rtic.rs/2/book/en/) and
[Embassy](https://embassy.dev).

Includes:

 - control, interrupt, and bulk endpoint support;
 - hub support;
 - hot-plug, and hot-unplug, including of hubs.

Currently supports:

 - RP2040 (USB 1.1 host)[^1]

System-tests and examples:

 - rp2040-usb-otge100: identifying (not yet really "driving") a
   Plugable USB2-OTGE100 Ethernet adaptor (based on ASIX AX88772)

Limitations:

 - maximum of 31 devices total (including hubs);
 - maximum of 15 hubs;
 - maximum of 15 ports on any one hub[^2];
 - supports Low Speed (1.5Mbits/s) and Full Speed (12Mbits/s)
   operation only -- not High Speed (480Mbits/s) or above.

[^1]: The documentation describes this as "USB&nbsp;2.0 LS and FS" (1.5 and
12Mbits/s), but as the _only_ changes in USB&nbsp;2.0 compared to 1.1
were related to the addition of HS (480Mbits/s), it seems more honest
to describe it as USB&nbsp;1.1.

[^2]: USB&nbsp;3.0 explicitly limits hubs to 15 downstream ports;
USB&nbsp;2.0 and earlier did not, but even for USB&nbsp;2.0 such wide
hubs are either very rare or, quite possibly, non-existent. Most
freestanding hubs which _appear_ to have more than about 7 downstream
ports, are in fact multiple hubs in a trenchcoat.

Library documentation is [on
docs.rs](https://docs.rs/cotton-usb-host/latest/cotton_usb-host/).

## Using cotton-usb-host with a Raspberry&nbsp;Pi Pico

This crate supports USB host mode _only_, and not USB device mode. So
before running your code, make sure that the USB connector on your
Raspberry&nbsp;Pi Pico is plugged into a USB device, and not into
another USB host such as a laptop[^3]. (You can still use a SWD connection
to program and debug your Raspberry&nbsp;Pi Pico, just not the USB
connection.)

If your Raspberry&nbsp;Pi Pico is itself powered by USB (perhaps via a
Pico Debug Probe), then it will not have enough power to reliably
supply USB power to downstream devices unless you power your Pico's
VUSB/GND pins from a separate 5V power supply &mdash; and perhaps not
even then. For best results with multiple devices, you should use a
_powered_ hub. (Powered hubs with micro-USB plugs, compatible with the
Raspberry&nbsp;Pi Pico in host mode, are often sold as "[OTG
hubs](https://www.amazon.co.uk/AuviPal-Adapter-Playstation-Classic-Raspberry-Black/dp/B083WML1XB/)".)

The crate is split between a generic (hardware-agnostic) [`usb_bus::UsbBus`]
class, and a host-controller driver specific to the RP2040. So the
minimal code example would involve:

 - creating a `UsbShared` object, making sure it's shared between the
   software tasks and the hardware interrupt handler;
 - arranging that the `USBCTRL_IRQ` interrupt handler calls `UsbShared::on_irq()`;
 - creating a `UsbStatics` object, which needn't be shared, but must
   be `&'static` &mdash; for instance, by using the `static-cell` crate;
 - constructing a `host::rp2040::Rp2040HostController` from the UsbShared,
   the UsbStatics, and the USB register banks from `rp2040-pac`;
 - constructing a `UsbBus` from the host-controller driver;
 - obtaining a stream of device-status events from
   `UsbBus::device_events()` &mdash; or, alternatively,
   `UsbBus::device_events_no_hubs()` for smaller code-size if
   supporting USB hubs isn't required;
 - waiting on the stream until it produces a `DeviceEvent::Connect`
   indicating that the device has been detected;
 - using APIs such as `UsbBus::control_transfer` to read descriptors,
   `UsbBus::configure` to configure the device appropriately, and
   `UsbBus::interrupt_endpoint` to read data from the device.

[^3]: The Raspberry&nbsp;Pi Pico (and the W5500-EVB-Pico for that
matter) have USB Micro-B receptacles (sockets), capable of receiving
Micro-B plugs only. Because they are capable of both USB device and
USB host, they _should_ arguably have had USB Micro-AB receptacles,
capable of receiving both Micro-A (rectangular) and Micro-B
(trapezium) plugs. But USB cables and adaptors with Micro-B plugs for
their _host_ side connection (instead of Micro-A which it [should
technically be](https://en.wikipedia.org/wiki/USB_On-The-Go)) are
common. Both ST and Renesas devboards get this right: they have Micro-AB
receptacles (though the very newest ST ones have type-C instead).

## Writing drivers for USB devices

This crate includes an example of identifying and communicating with
a Plugable USB2-OTGE100 Ethernet adaptor based on the ASIX&nbsp;AX88772
chip.

Once your code has successfully created the `UsbBus` object and has
called `UsbBus::device_events()`, it will receive `DeviceInfo` objects
which allow your code to identify relevant devices either by class
code (for generic class drivers such as mass-storage or HID) or by VID
and PID (for device-specific drivers).

## Writing drivers for alternative host controllers

The `UsbBus` code _should_ be generic enough to be usable with other
microcontrollers' USB host peripherals. You'll need to implement the
`host_controller::HostController` trait, which encapsulates all the
actual hardware interaction. Typically such host controllers have a
smallish, fixed number of "pipes" (actively-used endpoints) which can
be used simultaneously; you might find `async_pool::Pool`, as used by
the RP2040 host-controller driver, to be a convenient way of
allocating those pipes as required.

The RP2040 support is in this repo to provide a convenient worked example;
specific host-controller support for other microcontrollers probably
belongs in those microcontrollers' HAL crates.

## TODO

TODO before merge

 - [x] Hub state machine
 - [x] Unit tests
 - [x] Interlocking to avoid contending on pipe 0
 - [x] Rename types to wire
 - [x] Introduce delay to process_hub_packet and do away with currently_resetting
 - [ ] Remove from HubEventStream on disconnect
 - [ ] Move all tests to src/tests

TODO before 0.1.0:

 - [x] System test
 - [ ] Allocate endpoints from device object?
 - [x] UnconfiguredDevice/Device?
 - [ ] Bulk in/out
 - [ ] doc-comments
 - [ ] At least one real example (MSC? HID?)
 - [ ] Review register usage for contention (buff_status?)
 - [ ] STM32?

TODO later:

 - [ ] Non-async version?
 - [ ] rp-pac vs rp2040-pac?
 - [ ] More microcontrollers

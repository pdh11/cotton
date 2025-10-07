
# cotton-usb-host Changelog

## Unreleased

## [0.2.1] 2025-10-07

### Fixed

* RP2040: Fixed race-condition in ISR that could lead to transfer-complete
  interrupts getting dropped (and everything grinding to a halt).

## [0.2.0] 2025-08-20

### Fixed

* Low-Speed devices behind Full-Speed hubs need special treatment (GH-12)

### Changed

* Add `TransferExtras` (`Normal` or `WithPreamble`) to `HostController` methods
  * This is a breaking change for HostControllers, but not for UsbBus users
* `UsbBus::interrupt_endpoint_in()` now takes `&UsbDevice`, not just the address
  * This is a breaking change (but callers will likely be easy to change)

## [0.1.1] 2025-04-27

### Changed

* Cope with config descriptors up to 256 bytes (instead of 64) (@cryptographix)

## [0.1.0] 2024-12-02

Initial release

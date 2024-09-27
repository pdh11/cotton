# cotton-netif Changelog

## Unreleased

## [0.0.5] 2024-09-27

### Changed

* Flags is now a plain struct, not a "bitfield!"

* Update MSRV from 1.65 to 1.75.

* Update some dependencies.

## [0.0.4] 2023-04-23

### Changed

* InterfaceIndex, which can't be zero, now contains a NonZeroU32.

## [0.0.3] 2023-03-29

### Changed

* get_interfaces_async doesn't _itself_ need to be an async function.

### Fixed

* No longer assumes that network interfaces are always 1,2,3... with no gaps.

## [0.0.2] 2023-01-22

Packaging and documentation issues only, no code changes.

## [0.0.1] 2023-01-22

Initial release.

[![CI status](https://github.com/pdh11/cotton/actions/workflows/ci.yml/badge.svg)](https://github.com/pdh11/cotton/actions)
[![codecov](https://codecov.io/gh/pdh11/cotton/branch/main/graph/badge.svg?token=SMSZEPGRHA)](https://codecov.io/gh/pdh11/cotton)
[![dependency status](https://deps.rs/repo/github/pdh11/cotton/status.svg)](https://deps.rs/repo/github/pdh11/cotton)
[![Crates.io](https://img.shields.io/crates/v/cotton-unique)](https://crates.io/crates/cotton-unique)
[![Crates.io](https://img.shields.io/crates/d/cotton-unique)](https://crates.io/crates/cotton-unique)
[![docs.rs](https://img.shields.io/docsrs/cotton-unique)](https://docs.rs/cotton-unique/latest/cotton_unique/)
[![License: CC0-1.0](https://img.shields.io/badge/License-CC0_1.0-lightgrey.svg)](http://creativecommons.org/publicdomain/zero/1.0/)

# cotton-unique

Part of the [Cotton](https://github.com/pdh11/cotton) project.

## Implementing statistically-unique per-device IDs based on chip IDs

The cotton-unique crate encapsulates the creation of per-device unique
identifiers -- for things such as Ethernet MAC addresses, or UPnP UUIDs.

Most microcontrollers (e.g. STM32, RA6M5) have a unique per-unit
identifier built-in; RP2040 does not, but on that platform it's intended
to use the unique identifier in the associated SPI flash chip instead.

But it's not a good idea to just use the raw chip ID as the MAC
address, for several reasons: it's the wrong size, it's quite
predictable (it's not 96 random bits per chip, it typically
encodes the chip batch number and die position on the wafer, so
two different STM32s might have IDs that differ only in one or two
bits, meaning we can't just pick any 46 bits from the 96 in case
we accidentally pick seldom-changing ones) — and, worst of all, if
anyone were to use the same ID for anything else later, they might
be surprised if it were very closely correlated with the device's
MAC address.

So the thing to do, is to hash the unique ID along with a key, or
salt, which indicates what we're using it for. The result is thus
deterministic and consistent on any one device for a particular
salt, but varies from one device to another (and from one salt to
another).

For instance, the cotton-ssdp device tests obtain a MAC address by
hashing the STM32 unique ID with the salt string "stm32-eth", and
UPnP UUIDs by hashing the _same_ ID with a _different_ salt.

This does not _guarantee_ uniqueness, but if the hash function is
doing its job, the odds of a collision involve a factor of 2^-64 --
or in other words are highly unlikely.

There's more about the thinking behind for this crate [on my
blog](https://pdh11.blogspot.com/2024/03/system-tests-2.html).

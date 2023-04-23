[![CI status](https://github.com/pdh11/cotton/actions/workflows/ci.yml/badge.svg)](https://github.com/pdh11/cotton/actions)
[![codecov](https://codecov.io/gh/pdh11/cotton/branch/main/graph/badge.svg?token=SMSZEPGRHA)](https://codecov.io/gh/pdh11/cotton)
[![dependency status](https://deps.rs/repo/github/pdh11/cotton/status.svg)](https://deps.rs/repo/github/pdh11/cotton)
[![Crates.io](https://img.shields.io/crates/v/cotton-ssdp)](https://crates.io/crates/cotton-ssdp)
[![Crates.io](https://img.shields.io/crates/d/cotton-ssdp)](https://crates.io/crates/cotton-ssdp)
[![docs.rs](https://img.shields.io/docsrs/cotton-ssdp)](https://docs.rs/cotton-ssdp/latest/cotton_ssdp/)
[![License: CC0-1.0](https://img.shields.io/badge/License-CC0_1.0-lightgrey.svg)](http://creativecommons.org/publicdomain/zero/1.0/)

# cotton-ssdp

Part of the [Cotton](https://github.com/pdh11/cotton) project.

Implementing SSDP, the Simple Service Discovery Protocol

The cotton-ssdp crate encapsulates a client and server for the
Simple Service Discovery Protocol (SSDP), a mechanism for
discovering available _resources_ (services) on local networks. A
 _resource_ might be a streaming-media server, or a router, or a
network printer, or anything else that someone might want to
search for or enumerate on a network.

What is advertised, or discovered, is, for each resource, a unique
identifier for that particular resource (Unique Service Name,
USN), an identifier for the _type_ of resource (Notification Type,
NT), and the _location_ of the resource in the form of a URL.

SSDP is mainly used by UPnP (Universal Plug-'n'-Play) systems,
such as for media libraries and local streaming of music and video
-- but the mechanism is quite generic, and could as easily be used
for any type of device or resource that must be discoverable over
a network, including in *ad hoc* settings which don't necessarily
have expert network administrators close at hand.

Library documentation is [on
docs.rs](https://docs.rs/cotton-netif/latest/cotton_ssdp/).

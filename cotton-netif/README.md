[![CI status](https://github.com/pdh11/cotton/actions/workflows/ci.yml/badge.svg)](https://github.com/pdh11/cotton/actions) [![codecov](https://codecov.io/gh/pdh11/cotton/branch/main/graph/badge.svg?token=SMSZEPGRHA)](https://codecov.io/gh/pdh11/cotton) [![Crates.io](https://img.shields.io/crates/v/cotton-netif)](https://crates.io/crates/cotton-netif) [![docs.rs](https://img.shields.io/docsrs/cotton-netif)](https://docs.rs/cotton-netif/latest/cotton_netif/) [![License: CC0-1.0](https://img.shields.io/badge/License-CC0_1.0-lightgrey.svg)](http://creativecommons.org/publicdomain/zero/1.0/)

# cotton-netif

Part of the [Cotton](https://github.com/pdh11/cotton) project.

Enumerating network interfaces and their IP addresses

The cotton-netif library crate encapsulates the obtaining of the
host’s network interfaces and IP addresses. It supports both
static/synchronous listing (i.e., a snapshot of the current list of
network interfaces) using get_interfaces and dynamic/asynchronous
listing (i.e., getting events as network interfaces and addresses come
and go) using get_interfaces_async.

At present this crate only works on Linux (and maybe BSD) but the
structure is such that adding compatibility with other platforms in
future, shouldn’t require changes to any client code.

Library documentation is [on
docs.rs](https://docs.rs/cotton-netif/latest/cotton_netif/).

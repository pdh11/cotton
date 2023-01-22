# cotton-netif

Part of the [Cotton](https://github.com/pdh11/cotton) project.

Enumerating network interfaces and their IP addresses

The netif crate encapsulates the obtaining of the host’s network
interfaces and IP addresses. It supports both static/synchronous
listing (i.e., a snapshot of the current list of network interfaces)
using get_interfaces and dynamic/asynchronous listing (i.e., getting
events as network interfaces and addresses come and go) using
get_interfaces_async.

At present this crate only works on Linux (and maybe BSD) but the
structure is such that adding compatibility with other platforms in
future, shouldn’t require changes to any client code.

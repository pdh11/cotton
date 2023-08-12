
# cotton-ssdp Changelog

## Unreleased

## [0.0.3] 2023-08-12

### Changed

* Don't require "sockets" passed to Engine to be *both* Multicast and
  TargetedSend. Engine never needs to do both things to the same socket,
  and under smoltcp the two might be different types.

## [0.0.2] 2023-04-23

### Changed

* Move call to `cotton_netif::get_interfaces_async` out of
  AsyncService::new; callers must now call it themselves, see
  ssdp-search.rs. This allows callers to filter the list of interfaces
  on which SSDP is performed.

* Rename Engine::on_interface_event to Engine::on_network_event, to
  match the type name NetworkEvent.

* Pass NetworkEvent structs as references where appropriate.

* Change the `location` field of `Advertisement` from url::Url to plain
  String, for no_std compatibility.


## [0.0.1] 2023-03-29

Initial release

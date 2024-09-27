
# cotton-ssdp Changelog

## Unreleased

### Fixed

* An off-by-one error in `Engine::handle_timeout()` meant that it would
  make no progress if called at precisely the requested timeout time.

### Changed

* Update MSRV from 1.65 to 1.75.

* Move RefreshTimer into Engine. Users of the low-level Engine
  facility must now parameterise Engine by a Timebase implementation
  (usually either StdTimebase or SmoltcpTimebase), but no longer need
  a separate refresh timer. Users of the higher-level Service and
  AsyncService facilities should see no API change. The
  "Engine::refresh" call is left public for transition purposes, but
  there should no longer be any need to call it directly; see the
  implementations of Service or AsyncService for example timeout
  handling.

* The `Engine::on_data` call has changed. It requires an additional
  parameter, the current time as a Timebase::Instant, but no longer
  requires a Socket. Users of the higher-level Service and
  AsyncService facilities should see no API change.

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

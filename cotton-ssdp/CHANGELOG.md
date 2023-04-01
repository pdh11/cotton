# cotton-ssdp Changelog

## Unreleased

### Changed

* Move call to `cotton_netif::get_interfaces_async` out of
  AsyncService::new; callers must now call it themselves, see
  ssdp-search.rs. This allows callers to filter the list of interfaces
  on which SSDP is performed.

* Rename Engine::on_interface_event to Engine::on_network_event, to
  match the type name NetworkEvent.

* Pass NetworkEvent structs as references where appropriate.


## [0.0.1] 2023-03-29

Initial release
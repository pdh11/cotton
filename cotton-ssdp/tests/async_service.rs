use cotton_ssdp::{Advertisement, AsyncService, Notification};
use futures_util::StreamExt;

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn services_communicate() {
    let mut ssdp1 = AsyncService::new().unwrap();
    let mut ssdp2 = AsyncService::new().unwrap();

    for event in cotton_netif::get_interfaces().unwrap() {
        ssdp1.on_network_event(&event).unwrap();
        ssdp2.on_network_event(&event).unwrap();
    }

    ssdp1.advertise(
        "uuid:999",
        Advertisement {
            notification_type: "upnp::Directory:3".to_string(),
            location: "http://127.0.0.1/description.xml".to_string(),
        },
    );

    ssdp1.advertise(
        "uuid:998",
        Advertisement {
            notification_type: "upnp::root_device".to_string(),
            location: "http://127.0.0.1/description.xml".to_string(),
        },
    );

    let mut stage: u32 = 0;

    let mut stream = ssdp2.subscribe("ssdp:all");
    while let Some(r) = stream.next().await {
        match r {
            Notification::Alive {
                ref notification_type,
                ref unique_service_name,
                location: _,
            } if notification_type == "upnp::Directory:3"
                && unique_service_name == "uuid:999"
                && stage == 0 =>
            {
                ssdp1.deadvertise("uuid:999");
                stage = 1;
            }
            Notification::ByeBye {
                ref notification_type,
                ref unique_service_name,
            } if notification_type == "upnp::Directory:3"
                && unique_service_name == "uuid:999"
                && stage == 1 =>
            {
                return;
            }
            _ => {}
        }
    }
}

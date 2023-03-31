use cotton_ssdp::*;
use futures_util::StreamExt;

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn services_communicate() {
    let mut ssdp1 = AsyncService::new().unwrap();
    let mut ssdp2 = AsyncService::new().unwrap();

    for event in cotton_netif::get_interfaces().unwrap() {
        ssdp1.on_network_event(&event);
        ssdp2.on_network_event(&event);
    }

    ssdp1.advertise(
        "uuid:999",
        Advertisement {
            notification_type: "upnp::Directory:3".to_string(),
            location: url::Url::parse("http://127.0.0.1/description.xml")
                .unwrap(),
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

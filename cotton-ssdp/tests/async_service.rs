use cotton_ssdp::*;
use futures_util::StreamExt;

#[tokio::test]
async fn services_communicate() {
    let mut ssdp1 = AsyncService::new().await.unwrap();
    let mut ssdp2 = AsyncService::new().await.unwrap();

    ssdp1.advertise(
        "uuid:999",
        Advertisement {
            notification_type: "upnp::Directory:3".to_string(),
            location: url::Url::parse("http://127.0.0.1/description.xml")
                .unwrap(),
        },
    );

    let mut stream = ssdp2.subscribe("ssdp:all");
    while let Some(r) = stream.next().await {
        if let Notification::Alive {
            ref notification_type,
            ref unique_service_name,
            location: _,
        } = r
        {
            if notification_type == "upnp::Directory:3"
                && unique_service_name == "uuid:999" {
                    return;
                }
        }
    }
}

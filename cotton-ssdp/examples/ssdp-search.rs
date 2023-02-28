use cotton_ssdp::*;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::error::Error;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    println!(
        "ssdp-search from {} {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    let mut s = AsyncService::new().await?;

    let mut map = HashMap::new();

    let uuid = uuid::Uuid::new_v4();

    s.advertise(
        uuid.to_string(),
        Advertisement {
            notification_type: "test".to_string(),
            location: url::Url::parse("http://127.0.0.1/test").unwrap(),
        },
    );

    let mut stream = s.subscribe("ssdp:all");
    while let Some(r) = stream.next().await {
        println!("GOT {:?}", r);
        if let NotificationSubtype::AliveLocation(loc) =
            &r.notification_subtype
        {
            if !map.contains_key(&r.unique_service_name) {
                println!("+ {}", r.notification_type);
                println!("  {} at {}", r.unique_service_name, loc);
                map.insert(r.unique_service_name.clone(), r);
            }
        }
    }

    Ok(())
}

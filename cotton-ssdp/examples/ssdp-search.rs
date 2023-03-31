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

    let mut netif = cotton_netif::get_interfaces_async()?;
    let mut ssdp = AsyncService::new()?;
    let mut map = HashMap::new();
    let uuid = uuid::Uuid::new_v4();

    ssdp.advertise(
        uuid.to_string(),
        Advertisement {
            notification_type: "test".to_string(),
            location: url::Url::parse("http://127.0.0.1/test").unwrap(),
        },
    );

    let mut stream = ssdp.subscribe("ssdp:all");
    loop {
        tokio::select! {
            notification = stream.next() => {
                if let Some(r) = notification {
                    if let Notification::Alive {
                        ref notification_type,
                        ref unique_service_name,
                        ref location,
                    } = r
                    {
                        if !map.contains_key(unique_service_name) {
                            println!("+ {}", notification_type);
                            println!("  {} at {}", unique_service_name, location);
                            map.insert(unique_service_name.clone(), r);
                        }
                    }
                }
            },
            e = netif.next() => {
                if let Some(Ok(event)) = e {
                    ssdp.on_network_event(&event);
                }
            }
        }
    }
}

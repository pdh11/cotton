use cotton_netif::*;
use futures_util::StreamExt;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut s = network_interfaces_dynamic().await?;

    while let Some(e) = s.next().await {
        println!("{:?}", e);
    }

    Ok(())
}

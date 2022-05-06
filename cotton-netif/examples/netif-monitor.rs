use cotton_netif::*;
use futures_util::StreamExt;
use std::error::Error;

#[tokio::main(flavor="current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut s = network_interfaces_static().await?;

    println!("static:");
    while let Some(e) = s.next().await {
        println!("{:?}", e);
    }

    let mut s = network_interfaces_dynamic().await?;

    println!("dynamic:");
    while let Some(e) = s.next().await {
        println!("{:?}", e);
    }

    Ok(())
}

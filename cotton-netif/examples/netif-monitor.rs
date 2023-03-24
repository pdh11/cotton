use futures_util::StreamExt;
use std::error::Error;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("static:");

    for e in cotton_netif::get_interfaces()? {
        println!("{:?}", e);
    }

    let mut s = cotton_netif::get_interfaces_async()?;

    println!("dynamic:");
    while let Some(e) = s.next().await {
        println!("{:?}", e);
    }

    Ok(())
}

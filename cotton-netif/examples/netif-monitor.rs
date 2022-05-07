use futures_util::StreamExt;
use std::error::Error;

#[tokio::main(flavor="current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("static:");

    cotton_netif::get_interfaces(|e| println!("{:?}", e))?;

    let mut s = cotton_netif::get_interfaces_async().await?;

    println!("dynamic:");
    while let Some(e) = s.next().await {
        println!("{:?}", e);
    }

    Ok(())
}

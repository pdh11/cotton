use cotton_netif::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let ni = dynamic::NetworkInterfaces::new().await?;

    let mut s = ni.scan();

    while let Some(e) = s.next().await {
        println!("{:?}", e);
    }

    Ok(())
}

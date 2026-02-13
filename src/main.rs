mod bt_mgmt;

use bdaddr::Address;
use std::time::Duration;
use tokio::time;

use crate::bt_mgmt::BtMgmt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bt_mgmt = BtMgmt::new("24:29:34:8E:0A:58".parse::<Address>().unwrap().into())?;

    println!("Monitoring RSSI for 24:29:34:8E:0A:58...");

    let mut interval = time::interval(Duration::from_secs(2));

    loop {
        interval.tick().await;

        match bt_mgmt.get_connection_information().await {
            Ok((rssi, tx_power)) => {
                println!("Current RSSI: {} dBm (TX Power: {} dBm)", rssi, tx_power);
            }
            Err(e) => {
                eprintln!("Failed to get RSSI: {}", e);
            }
        }
    }
}

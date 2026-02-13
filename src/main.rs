mod bt_mgmt;
mod kalman;

use bdaddr::Address;
use std::time::Duration;
// use tokio::time::Instant;
use colored::Colorize;
use tokio::time;

use crate::bt_mgmt::BtMgmt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    const PHONE_MAC: &str = "24:29:34:8E:0A:58";
    let mut bt_mgmt = BtMgmt::new(PHONE_MAC.parse::<Address>().unwrap().into())?;

    println!("Monitoring {}...", PHONE_MAC);

    let mut interval = time::interval(Duration::from_millis(200));

    loop {
        interval.tick().await;

        // let now = Instant::now();

        match bt_mgmt.relative_path_loss().await {
            Ok(rpl) => {
                let output = format!(
                    "{}: Current RPL: {} ",
                    chrono::Local::now().format("%H:%M:%S%.3f"),
                    rpl
                );
                println!(
                    "{}",
                    if rpl >= 15.0 {
                        output.red()
                    } else {
                        output.green()
                    }
                );
            }
            Err(e) => {
                eprintln!("Con Info Error: {}", e);
            }
        }

        // println!("elapsed: {:?} ", now.elapsed());
    }
}

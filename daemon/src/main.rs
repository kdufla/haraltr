mod bt_mgmt;
mod input;
mod kalman;
mod session;
mod wake_up;

use bdaddr::Address;
use colored::Colorize;
use std::time::Duration;
use tokio::time::Instant;
use tokio::{select, time};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::bt_mgmt::BtMgmt;
use crate::session::SessionController;
use crate::wake_up::wake_up;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // const PHONE_MAC: &str = "24:29:34:8E:0A:58";
    // const PHONE_MAC: &str = "F4:EE:25:B1:B1:6E";

    // let mut bt_mgmt = BtMgmt::new(PHONE_MAC.parse::<Address>().unwrap().into())?;

    // println!("Monitoring {}...", PHONE_MAC);

    // let mut interval = time::interval(Duration::from_millis(2000));

    // let controller = SessionController::new().await.unwrap();
    // println!("{:?}", controller);

    // time::sleep(Duration::from_secs(3)).await;

    // controller.lock().await.unwrap();

    // time::sleep(Duration::from_secs(3)).await;

    let (tx, rx) = tokio::sync::oneshot::channel();

    // wake_up(rx);

    select! {
        _ = wake_up(rx) => (),
        _ = time::sleep(Duration::from_secs(10)) => tx.send(()).unwrap(),
    };

    // controller.unlock().await.unwrap();

    // time::sleep(Duration::from_secs(3)).await;

    // loop {
    //     interval.tick().await;

    //     let now = Instant::now();

    //     match bt_mgmt.relative_path_loss().await {
    //         Ok(rpl) => {
    //             let output = format!(
    //                 "{}: Current RPL: {} ",
    //                 chrono::Local::now().format("%H:%M:%S%.3f"),
    //                 rpl
    //             );
    //             println!(
    //                 "{}",
    //                 if rpl >= 15.0 {
    //                     output.red()
    //                 } else {
    //                     output.green()
    //                 }
    //             );
    //         }
    //         Err(e) => {
    //             eprintln!("Con Info Error: {}", e);
    //         }
    //     }

    //     println!("elapsed: {:?} ", now.elapsed());
    // }

    Ok(())
}

pub fn init_tracing() {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
}

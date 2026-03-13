use btmgmt::command::GetConnectionInformation;
use btmgmt::{Client, client::Result};
use btmgmt_packet::{Address, AddressType};
use tracing::{debug, trace};

use crate::kalman::KalmanFilter;

pub struct BtMgmt {
    client: Client,
    adapter_index: u16,
    target_mac: Address,
    addr_type: AddressType,
    denoise_filter: KalmanFilter,
}

impl BtMgmt {
    pub fn new(target_mac: Address) -> Result<Self> {
        Ok(Self {
            client: Client::open()?,
            adapter_index: 0,
            target_mac,
            addr_type: AddressType::BrEdr,
            denoise_filter: KalmanFilter::new(5.0),
        })
    }

    pub async fn relative_path_loss(&mut self) -> Result<f64> {
        let (rssi, tx_power) = self.get_connection_information().await?;
        let raw_rpl = tx_power as f64 - rssi as f64;
        let filtered_rpl = self.denoise_filter.update(raw_rpl);
        debug!(raw_rpl, filtered_rpl, "relative path loss");
        Ok(filtered_rpl)
    }

    async fn get_connection_information(&self) -> Result<(i8, i8)> {
        let cmd = GetConnectionInformation::new(self.target_mac.clone(), self.addr_type.clone());
        let reply = self.client.call(self.adapter_index, cmd).await?;

        let rssi = *reply.rssi() as i8;
        let tx_power = *reply.tx_power() as i8;
        trace!(rssi, tx_power, "connection info");

        Ok((rssi, tx_power))
    }
}

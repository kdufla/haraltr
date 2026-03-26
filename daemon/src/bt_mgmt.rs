use btmgmt::command::GetConnectionInformation;
use btmgmt::{Client, client::Result};
use btmgmt_packet::{Address, AddressType};
use crate::config::{AddressTypeConfig, BluetoothConfig, ProximityConfig};
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
    pub fn new(bt_config: &BluetoothConfig, prox_config: &ProximityConfig) -> Result<Self> {
        let target_mac: Address = bt_config
            .target_mac
            .as_deref()
            .expect("target_mac must be set")
            .parse::<bdaddr::Address>()
            .expect("invalid target MAC address")
            .into();

        let addr_type = match bt_config.address_type {
            AddressTypeConfig::BrEdr => AddressType::BrEdr,
            AddressTypeConfig::LePublic => AddressType::LePublic,
        };

        Ok(Self {
            client: Client::open()?,
            adapter_index: bt_config.adapter_index,
            target_mac,
            addr_type,
            denoise_filter: KalmanFilter::new(
                prox_config.kalman_initial,
                prox_config.kalman_q,
                prox_config.kalman_r,
            ),
        })
    }

    pub async fn relative_path_loss(&mut self) -> Result<(f64, f64)> {
        let (rssi, tx_power) = self.get_connection_information().await?;
        let raw_rpl = tx_power as f64 - rssi as f64;
        let filtered_rpl = self.denoise_filter.update(raw_rpl);
        debug!(raw_rpl, filtered_rpl, "relative path loss");
        Ok((filtered_rpl, raw_rpl))
    }

    pub fn update_kalman_params(&mut self, q: f64, r: f64) {
        self.denoise_filter.update_params(q, r);
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

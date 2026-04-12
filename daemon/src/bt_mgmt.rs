use btmgmt::{
    Client,
    client::{Error, Result},
    command::GetConnectionInformation,
};
use btmgmt_packet::{Address, AddressType};
use tracing::{debug, trace};

use crate::config::{AddressTypeConfig, BluetoothConfig, ProximityConfig};

const HCI_TX_POWER_INVALID: i8 = 127;

pub struct BtMgmt {
    client: Client,
    adapter_index: u16,
    target_mac: Address,
    addr_type: AddressType,
    denoise_filter: KalmanFilter,
    // if LE hci does not support HCI_OP_READ_TX_POWER
    fallback_tx_power: i8,
}

impl BtMgmt {
    pub fn new(
        target_mac: &str,
        bt_config: &BluetoothConfig,
        prox_config: &ProximityConfig,
    ) -> Result<Self> {
        let target_mac: Address = target_mac
            .parse::<bdaddr::Address>()
            .expect("invalid target MAC address")
            .into();

        let addr_type = match bt_config.address_type {
            AddressTypeConfig::BrEdr => AddressType::BrEdr,
            AddressTypeConfig::LePublic => AddressType::LePublic,
            AddressTypeConfig::LeRandom => AddressType::LeRandom,
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
            fallback_tx_power: prox_config.fallback_tx_power,
        })
    }

    pub async fn relative_path_loss(&mut self) -> Result<(f64, f64)> {
        let (rssi, tx_power) = self.get_connection_information().await?;
        let effective_tx = if tx_power == HCI_TX_POWER_INVALID {
            match self.addr_type {
                // bredr hci supports HCI_OP_READ_TX_POWER
                // HCI_TX_POWER_INVALID == failure
                AddressType::BrEdr => {
                    return Err(Error::Unexpected(
                        "BR/EDR controller returned TX power HCI_TX_POWER_INVALID".into(),
                    ));
                }
                // le hci may not support HCI_OP_READ_TX_POWER
                // use preconfigured fallback
                _ => self.fallback_tx_power,
            }
        } else {
            tx_power
        };
        let raw_rpl = effective_tx as f64 - rssi as f64;
        let filtered_rpl = self.denoise_filter.update(raw_rpl);
        debug!(raw_rpl, filtered_rpl, "relative path loss");
        Ok((filtered_rpl, raw_rpl))
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

struct KalmanFilter {
    x: f64, // Estimate
    p: f64, // Covariance of the estimate
    q: f64, // Covariance of the process noise
    r: f64, // Covariance of the observation noise
}

impl KalmanFilter {
    fn new(initial_value: f64, q: f64, r: f64) -> Self {
        Self {
            x: initial_value,
            p: 1.0,
            q,
            r,
        }
    }

    fn update(&mut self, z: f64) -> f64 {
        self.p += self.q;
        let k = self.p / (self.p + self.r);
        self.x += k * (z - self.x);
        self.p *= 1.0 - k;
        self.x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kalman_filter_init() {
        let kf = KalmanFilter::new(10.0, 0.1, 3.0);
        assert_eq!(kf.x, 10.0);
        assert_eq!(kf.p, 1.0);
        assert_eq!(kf.q, 0.1);
        assert_eq!(kf.r, 3.0);
    }

    #[test]
    fn kalman_filter_update() {
        let mut kf = KalmanFilter::new(10.0, 0.1, 3.0);
        let first_update = kf.update(12.0);
        assert!(first_update > 10.0 && first_update < 12.0);

        let second_update = kf.update(12.0);
        // Should move closer to 12.0
        assert!(second_update > first_update);
        assert!(second_update < 12.0);
    }
}

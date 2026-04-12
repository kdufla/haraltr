use std::{sync::Arc, time::Duration};

use tokio::{sync::mpsc, task::JoinHandle, time};
use tracing::{debug, error};

use crate::{
    bt_mgmt::BtMgmt,
    config::{BluetoothConfig, ProximityConfig},
    proximity::{Action, ProxState, Reading},
    state::{AppState, DeviceAction, DeviceStatus},
};

pub fn spawn_device_task(
    target_mac: String,
    bt_config: BluetoothConfig,
    prox_config: ProximityConfig,
    app_state: Arc<AppState>,
    action_tx: mpsc::Sender<DeviceAction>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut bt = match BtMgmt::new(&target_mac, &bt_config, &prox_config) {
            Ok(bt) => bt,
            Err(e) => {
                error!(mac = %target_mac, "failed to create BtMgmt: {e}");
                return;
            }
        };

        let mut prox_state = ProxState::new(&prox_config);
        let mut poll_interval = time::interval(Duration::from_millis(bt_config.poll_interval_ms));

        loop {
            poll_interval.tick().await;

            let (reading, rpl, raw_rpl, connected) = match bt.relative_path_loss().await {
                Ok((filtered_rpl, raw_rpl)) => (
                    Reading::Rpl(filtered_rpl),
                    Some(filtered_rpl),
                    Some(raw_rpl),
                    true,
                ),
                Err(e) => {
                    debug!(mac = %target_mac, "BT poll failed: {e}");
                    (Reading::ConnectionLost, None, None, false)
                }
            };

            let was_disconnected = prox_state.is_disconnected();
            let action = prox_state.transition(reading);
            let is_disconnected = prox_state.is_disconnected();

            app_state.update_device(
                target_mac.clone(),
                DeviceStatus {
                    rpl,
                    raw_rpl,
                    phase: prox_state.proximity_phase(),
                    connected,
                },
            );

            if action != Action::None {
                let _ = action_tx
                    .send(DeviceAction {
                        target_mac: target_mac.clone(),
                        action,
                    })
                    .await;
            }

            if was_disconnected != is_disconnected {
                let target_ms = if is_disconnected {
                    bt_config.disconnect_poll_interval_ms
                } else {
                    bt_config.poll_interval_ms
                };
                poll_interval = time::interval(Duration::from_millis(target_ms));
            }
        }
    })
}

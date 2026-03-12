//! The AirPods showing up nearby. BlueZ's advertisement monitor
//! (`Experimental = true` in `/etc/bluetooth/main.conf`) reports Apple
//! proximity pairing advertisements, also sent while the pods charge in the
//! closed case. It reports a device once, so each one's advertisement data
//! is followed as BlueZ updates it: the lid opening changes it. The engine
//! connects the pods whose stored IRK resolves the address, if they're out
//! (`Engine::nearby`). A fresh link starts with the card profile `off`.

use std::sync::Arc;

use bluer::monitor::{Monitor, MonitorEvent, Pattern, data_type};
use bluer::{Address, Device, DeviceEvent, DeviceProperty};
use futures_util::StreamExt as _;
use protocol::MacAddress;
use protocol::aacp::KeyType;
use protocol::ble::{APPLE_COMPANY_ID, PROXIMITY_PAIRING, PodsKeys};
use tokio::sync::mpsc;

use crate::{Daemon, Input, state};

/// A key's file: `<address>.irk` or `<address>.enc` (hex) in the state
/// directory, one per pair.
fn key_path(pods: Address, kind: KeyType) -> Option<std::path::PathBuf> {
    let extension = match kind {
        KeyType::Irk => "irk",
        KeyType::EncryptionKey => "enc",
    };
    Some(state::dir()?.join(format!("{pods}.{extension}")))
}

/// Store a key the pods sent (both are requested in every session).
pub fn save_key(pods: MacAddress, kind: KeyType, key: [u8; 16]) {
    let pods = Address(pods.to_array());
    let Some(path) = key_path(pods, kind) else {
        return;
    };
    let hex = hex::encode(key);
    if std::fs::read_to_string(&path).is_ok_and(|stored| stored.trim() == hex) {
        return;
    }
    let saved = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, hex));
    match saved {
        Ok(()) => tracing::info!(%pods, ?kind, "stored the AirPods' key"),
        Err(e) => tracing::warn!(?kind, "storing the AirPods' key: {e}"),
    }
}

fn read_key(pods: Address, kind: KeyType) -> Option<[u8; 16]> {
    let hex = std::fs::read_to_string(key_path(pods, kind)?).ok()?;
    hex::decode(hex.trim()).ok()?.try_into().ok()
}

/// Every stored pair: classic address and keys. Read on each advertisement,
/// so a key learned meanwhile counts at once.
pub fn stored_keys() -> Vec<(MacAddress, PodsKeys)> {
    let entries = state::dir().and_then(|dir| std::fs::read_dir(dir).ok());
    entries
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "irk" {
                return None;
            }
            let pods = path.file_stem()?.to_str()?.parse::<Address>().ok()?;
            let keys = PodsKeys {
                irk: read_key(pods, KeyType::Irk)?,
                encryption: read_key(pods, KeyType::EncryptionKey),
            };
            Some((MacAddress::new(pods.0), keys))
        })
        .collect()
}

/// Watch for the AirPods' advertisement; without BlueZ's monitor (not
/// experimental), say so once and carry on.
pub fn spawn(daemon: Arc<Daemon>) {
    tokio::spawn(async move {
        if let Err(e) = watch(&daemon).await {
            tracing::info!(
                "no AirPods presence monitor (Experimental = true in \
                 /etc/bluetooth/main.conf?): {e}"
            );
        }
    });
}

async fn watch(daemon: &Daemon) -> bluer::Result<()> {
    let [company_lo, company_hi] = APPLE_COMPANY_ID.to_le_bytes();
    // Dropping the manager unregisters its monitors from BlueZ: keep it.
    let manager = daemon.adapter.monitor().await?;
    let mut monitor = manager
        .register(Monitor {
            patterns: Some(vec![Pattern::new(
                data_type::MANUFACTURER_SPECIFIC_DATA,
                0,
                &[company_lo, company_hi, PROXIMITY_PAIRING],
            )]),
            ..Default::default()
        })
        .await?;
    tracing::info!("watching for the AirPods' BLE advertisement");
    while let Some(event) = monitor.next().await {
        if let MonitorEvent::DeviceFound(found) = event {
            tracing::debug!(address = %found.device, "Apple device advertising nearby");
            match daemon.adapter.device(found.device) {
                Ok(device) => {
                    tokio::spawn(follow(device, daemon.inputs.clone()));
                }
                Err(e) => tracing::debug!(address = %found.device, "{e}"),
            }
        }
    }
    Ok(())
}

/// Report `device`'s advertisement now and whenever it changes, until BlueZ
/// forgets the device (its address rotates).
async fn follow(device: Device, inputs: mpsc::UnboundedSender<Input>) {
    let address = device.address();
    let report = |data: Option<std::collections::HashMap<u16, Vec<u8>>>| {
        let data = data.and_then(|mut data| data.remove(&APPLE_COMPANY_ID));
        inputs
            .send(Input::Nearby(address, data.unwrap_or_default()))
            .is_ok()
    };
    // Subscribe first, so a change in between isn't missed.
    let Ok(mut events) = device.events().await else {
        return;
    };
    if !report(device.manufacturer_data().await.ok().flatten()) {
        return;
    }
    while let Some(DeviceEvent::PropertyChanged(property)) = events.next().await {
        if let DeviceProperty::ManufacturerData(data) = property
            && !report(Some(data))
        {
            return;
        }
    }
}

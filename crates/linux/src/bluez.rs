//! BlueZ plumbing: the Apple Device ID check, finding the pods, L2CAP setup,
//! and A2DP.

use std::time::Duration;

use bluer::{DeviceEvent, DeviceProperty, Uuid};
use futures_util::StreamExt as _;

use crate::CONNECT_TIMEOUT;

const L2CAP_ESTABLISH_POLL_INTERVAL: Duration = Duration::from_millis(50);
const A2DP_SINK_UUID: Uuid = Uuid::from_u128(0x0000110b_0000_1000_8000_00805f9b34fb);

const BLUEZ_CONFIG: &str = "/etc/bluetooth/main.conf";

/// Whether BlueZ itself already claims Apple's Device ID (from boot, so also
/// for autoconnects before this daemon runs).
pub fn apple_device_id_configured() -> bool {
    std::fs::read_to_string(BLUEZ_CONFIG).is_ok_and(|conf| configures_apple_device_id(&conf))
}

/// `DeviceID = bluetooth:004C:…` on an uncommented line.
fn configures_apple_device_id(conf: &str) -> bool {
    conf.lines().any(|line| {
        line.split_once('=').is_some_and(|(key, value)| {
            key.trim() == "DeviceID"
                && value
                    .trim()
                    .to_ascii_lowercase()
                    .starts_with("bluetooth:004c:")
        })
    })
}

/// First paired device named `AirPods…`.
pub async fn find_airpods(adapter: &bluer::Adapter) -> anyhow::Result<bluer::Device> {
    for addr in adapter.device_addresses().await? {
        let device = adapter.device(addr)?;
        let is_airpods = device
            .name()
            .await?
            .is_some_and(|name| name.to_lowercase().starts_with("airpod"));
        if is_airpods && device.is_paired().await? {
            return Ok(device);
        }
    }
    anyhow::bail!("no paired AirPods found")
}

/// Resolve once BlueZ reports `device` connected (its property signal, not a
/// poll). Errors when the device goes away, e.g. is unpaired.
pub async fn wait_connected(device: &bluer::Device) -> anyhow::Result<Link> {
    // Subscribe before checking, so a connect in between isn't missed.
    let mut events = device.events().await?;
    if device.is_connected().await? {
        return Ok(Link::Existing);
    }
    while let Some(DeviceEvent::PropertyChanged(property)) = events.next().await {
        if let DeviceProperty::Connected(true) = property {
            return Ok(Link::Fresh);
        }
    }
    anyhow::bail!("device removed")
}

/// How the pods' link came to be connected when we attached.
#[derive(Debug, Clone, Copy)]
pub enum Link {
    /// It just came up: nothing of ours can be playing yet.
    Fresh,
    /// It already was (daemon restart, AACP reconnect): this laptop may be
    /// the one playing.
    Existing,
}

/// Wait until the kernel assigns the channel ID.
pub async fn wait_l2cap_established(stream: &bluer::l2cap::SeqPacket) -> anyhow::Result<()> {
    const ENOTCONN: i32 = 107;
    let start = std::time::Instant::now();
    loop {
        match stream.peer_addr() {
            Ok(peer) if peer.cid != 0 => return Ok(()),
            Ok(_) => {}
            Err(e) if e.raw_os_error() == Some(ENOTCONN) => {
                anyhow::bail!("peer disconnected during L2CAP setup")
            }
            Err(e) => anyhow::bail!("peer_addr failed during setup: {e}"),
        }
        anyhow::ensure!(
            start.elapsed() < CONNECT_TIMEOUT,
            "timed out waiting for L2CAP connection"
        );
        tokio::time::sleep(L2CAP_ESTABLISH_POLL_INTERVAL).await;
    }
}

/// Connect (page) the pods unless they are; in the background, as a page
/// can take seconds.
pub fn connect(device: bluer::Device) {
    tokio::spawn(async move {
        if device.is_connected().await.unwrap_or(true) {
            return;
        }
        if let Err(e) = device.connect().await {
            tracing::info!(pods = %device.address(), "connecting the pods failed: {e}");
        }
    });
}

/// Fire-and-forget: BlueZ can block for many seconds here, and stalling the
/// session loop times out the AACP link.
pub fn activate_a2dp(device: &bluer::Device) {
    let device = device.clone();
    tokio::spawn(async move {
        tracing::info!("activating A2DP profile");
        if let Err(e) = device.connect_profile(&A2DP_SINK_UUID).await {
            tracing::warn!("failed to connect A2DP profile: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::configures_apple_device_id;

    #[test]
    fn detects_apple_device_id_in_bluez_config() {
        assert!(configures_apple_device_id(
            "[General]\nDeviceID = bluetooth:004C:0000:0000\n"
        ));
        assert!(configures_apple_device_id(
            "DeviceID=bluetooth:004c:0000:0000"
        ));
        assert!(!configures_apple_device_id(
            "[General]\n#DeviceID = bluetooth:004C:0000:0000\n"
        ));
        assert!(!configures_apple_device_id(
            "DeviceID = bluetooth:1234:5678:abcd"
        ));
    }
}

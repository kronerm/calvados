//! The pods' exact running firmware over UARP (`protocol::uarp`), through
//! BlueZ's GATT client: once per session, off the loop, reported back as
//! [`Input::ExactFirmware`]. Read-only: only Sync, version discovery and
//! the running firmware's information request are ever written, never an
//! offer, data or apply.

use std::pin::pin;
use std::time::Duration;

use bluer::gatt::WriteOp;
use bluer::gatt::remote::{Characteristic, CharacteristicWriteRequest};
use futures_util::StreamExt as _;
use protocol::MacAddress;
use protocol::firmware::ExactVersion;
use protocol::uarp::{self, Reassembler};
use tokio::sync::mpsc;

use crate::Input;

const TIMEOUT: Duration = Duration::from_secs(10);
/// Right after a reconnect BlueZ can refuse the first write ("Not
/// connected") for a moment.
const RETRY_DELAY: Duration = Duration::from_secs(3);
/// Between the writes, as the probe that was observed working spaced them.
const WRITE_GAP: Duration = Duration::from_millis(300);

/// Ask `device` for its exact firmware; without GATT or an answer, the
/// reported (`0x1D`) version stays.
pub fn spawn(device: bluer::Device, inputs: mpsc::UnboundedSender<Input>) {
    tokio::spawn(async move {
        let pods = MacAddress::new(device.address().0);
        match query(&device).await {
            Ok(exact) => {
                // The loop is gone only when the daemon stops.
                let _ = inputs.send(Input::ExactFirmware(pods, exact));
            }
            Err(e) => tracing::debug!(%pods, "no exact firmware over UARP: {e:#}"),
        }
    });
}

/// Why asking failed.
enum Failure {
    /// The first write: worth another try.
    Refused(bluer::Error),
    Other(anyhow::Error),
}

impl From<Failure> for anyhow::Error {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Refused(e) => e.into(),
            Failure::Other(e) => e,
        }
    }
}

async fn query(device: &bluer::Device) -> anyhow::Result<ExactVersion> {
    let answer = match tokio::time::timeout(TIMEOUT, ask(device)).await {
        Ok(Err(Failure::Refused(e))) => {
            tracing::debug!("UARP write refused, asking again: {e}");
            tokio::time::sleep(RETRY_DELAY).await;
            tokio::time::timeout(TIMEOUT, ask(device)).await
        }
        answer => answer,
    };
    Ok(answer.map_err(|_| anyhow::anyhow!("no answer within {TIMEOUT:?}"))??)
}

async fn ask(device: &bluer::Device) -> Result<ExactVersion, Failure> {
    let characteristic = characteristic(device).await.map_err(Failure::Other)?;
    // Subscribed before writing, so the response is buffered.
    let indications = characteristic
        .notify()
        .await
        .map_err(|e| Failure::Other(e.into()))?;
    let request = CharacteristicWriteRequest {
        op_type: WriteOp::Request,
        ..Default::default()
    };
    for (i, write) in uarp::running_version_query().into_iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(WRITE_GAP).await;
        }
        characteristic
            .write_ext(&write, &request)
            .await
            .map_err(|e| match i {
                0 => Failure::Refused(e),
                _ => Failure::Other(e.into()),
            })?;
    }
    let mut reassembler = Reassembler::default();
    let mut versions = pin!(indications.filter_map(|fragment| {
        let version = reassembler
            .push(&fragment)
            .and_then(|message| uarp::running_version(&message));
        std::future::ready(version)
    }));
    versions
        .next()
        .await
        .ok_or_else(|| Failure::Other(anyhow::anyhow!("indications ended")))
}

/// The UARP characteristic among the pods' GATT services.
async fn characteristic(device: &bluer::Device) -> anyhow::Result<Characteristic> {
    let uuid = bluer::Uuid::from_u128(uarp::CHARACTERISTIC);
    for service in device.services().await? {
        for characteristic in service.characteristics().await? {
            if characteristic.uuid().await? == uuid {
                return Ok(characteristic);
            }
        }
    }
    anyhow::bail!("no UARP characteristic")
}

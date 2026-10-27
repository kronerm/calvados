//! UARP (`protocol::uarp`) through BlueZ's GATT client: the pods' exact
//! running firmware, asked once per session off the loop and reported as
//! [`Input::ExactFirmware`] (read-only: Sync, version discovery and one
//! information request); and the firmware update's transport, which writes
//! what the engine says and reports what arrives as [`Input::Uarp`]. Never
//! both at once: the update's own Sync would cut the query short, and the
//! query's the update's.

use std::pin::pin;
use std::time::Duration;

use anyhow::Context as _;
use bluer::gatt::WriteOp;
use bluer::gatt::remote::{Characteristic, CharacteristicWriteRequest};
use futures_util::StreamExt as _;
use protocol::MacAddress;
use protocol::firmware::ExactVersion;
use protocol::uarp::{self, Reassembler};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use crate::Input;

const TIMEOUT: Duration = Duration::from_secs(10);
/// After a connect BlueZ refuses writes ("Not connected") until its GATT
/// channel is up: over 4 s once, so retry for half a minute.
const RETRY_DELAY: Duration = Duration::from_secs(5);
const TRIES: usize = 6;
/// Between the writes, as the probe that was observed working spaced them.
const WRITE_GAP: Duration = Duration::from_millis(300);

/// What the update's transport reports.
#[derive(Debug)]
pub enum Event {
    Up,
    /// An indication: a fragment, with its header.
    Received(Vec<u8>),
    /// Ended, or never opened.
    Down,
}

/// The loop's UARP: the session's query and the update's transport, the
/// transport winning.
#[derive(Default)]
pub struct Uarp {
    query: Option<AbortHandle>,
    /// Of the latest transport, so a stale one's events are told apart.
    generation: u64,
    transport: Option<mpsc::UnboundedSender<Vec<u8>>>,
}

impl Uarp {
    /// Ask `device` for its exact firmware, unless an update has the
    /// transport.
    pub fn query(&mut self, device: bluer::Device, inputs: &mpsc::UnboundedSender<Input>) {
        if self.transport.is_some() {
            return;
        }
        self.query.take().inspect(AbortHandle::abort);
        self.query = Some(spawn(device, inputs.clone()));
    }

    /// Open the update's transport to `device` unless it's open; reports
    /// [`Input::Uarp`] with this transport's generation.
    pub fn open(&mut self, device: bluer::Device, inputs: &mpsc::UnboundedSender<Input>) {
        if self.transport.is_some() {
            return;
        }
        if let Some(query) = self.query.take() {
            tracing::debug!("firmware update: the version query gives way");
            query.abort();
        }
        self.generation += 1;
        let (writes, queued) = mpsc::unbounded_channel();
        tokio::spawn(transport(device, self.generation, queued, inputs.clone()));
        self.transport = Some(writes);
    }

    /// Queued behind the writes before it.
    pub fn write(&self, write: Vec<u8>) {
        if let Some(transport) = &self.transport {
            // A transport that ended reports it; the engine follows.
            let _ = transport.send(write);
        }
    }

    /// Close the transport once its queued writes are out.
    pub fn close(&mut self) {
        self.transport = None;
    }

    /// `event` if it's the open transport's; its end closes it.
    pub fn event(&mut self, generation: u64, event: Event) -> Option<Event> {
        if generation != self.generation || self.transport.is_none() {
            return None;
        }
        if let Event::Down = event {
            self.transport = None;
        }
        Some(event)
    }
}

/// The update's transport: subscribe, report [`Event::Up`], then forward
/// indications and write what's queued, one write at a time, until the
/// queue closes or GATT fails. A failure is reported after a pause, which
/// paces the engine's reopening.
async fn transport(
    device: bluer::Device,
    generation: u64,
    mut writes: mpsc::UnboundedReceiver<Vec<u8>>,
    inputs: mpsc::UnboundedSender<Input>,
) {
    // The loop is gone only when the daemon stops.
    let report = |event| {
        let _ = inputs.send(Input::Uarp(generation, event));
    };
    match carry(&device, &mut writes, &report).await {
        Ok(()) => tracing::debug!("firmware update: transport closed"),
        Err(e) => {
            tracing::info!("firmware update: transport failed: {e:#}");
            tokio::time::sleep(RETRY_DELAY).await;
        }
    }
    report(Event::Down);
}

async fn carry(
    device: &bluer::Device,
    writes: &mut mpsc::UnboundedReceiver<Vec<u8>>,
    report: &impl Fn(Event),
) -> anyhow::Result<()> {
    let characteristic = characteristic(device).await?;
    let mut indications = pin!(characteristic.notify().await?);
    report(Event::Up);
    let request = CharacteristicWriteRequest {
        op_type: WriteOp::Request,
        ..Default::default()
    };
    // BlueZ refuses the first writes ("Not connected") until its GATT
    // channel is up, as for the query.
    let mut refusals = TRIES;
    loop {
        tokio::select! {
            fragment = indications.next() => {
                report(Event::Received(fragment.context("indications ended")?));
            }
            write = writes.recv() => {
                let Some(write) = write else {
                    return Ok(());
                };
                loop {
                    match characteristic.write_ext(&write, &request).await {
                        Ok(()) => break,
                        Err(e) if refusals > 1 => {
                            tracing::debug!("UARP write refused, writing again: {e}");
                            refusals -= 1;
                            tokio::time::sleep(RETRY_DELAY).await;
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                // Only the first write waits for GATT.
                refusals = 1;
            }
        }
    }
}

/// Ask `device` for its exact firmware; without GATT or an answer, the
/// reported (`0x1D`) version stays.
fn spawn(device: bluer::Device, inputs: mpsc::UnboundedSender<Input>) -> AbortHandle {
    tokio::spawn(async move {
        let pods = MacAddress::new(device.address().0);
        match query(&device).await {
            Ok(exact) => {
                // The loop is gone only when the daemon stops.
                let _ = inputs.send(Input::ExactFirmware(pods, exact));
            }
            Err(e) => tracing::debug!(%pods, "no exact firmware over UARP: {e:#}"),
        }
    })
    .abort_handle()
}

/// Why asking failed.
enum Failure {
    /// The first write: worth another try, GATT may not be up yet.
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
    for _ in 1..TRIES {
        match tokio::time::timeout(TIMEOUT, ask(device)).await {
            Ok(Err(Failure::Refused(e))) => {
                tracing::debug!("UARP write refused, asking again: {e}");
                tokio::time::sleep(RETRY_DELAY).await;
            }
            answer => return finish(answer),
        }
    }
    finish(tokio::time::timeout(TIMEOUT, ask(device)).await)
}

fn finish(
    answer: Result<Result<ExactVersion, Failure>, tokio::time::error::Elapsed>,
) -> anyhow::Result<ExactVersion> {
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

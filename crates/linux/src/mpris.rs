//! Local media control over MPRIS on the session bus. Commands run in order
//! on one background task, so callers never wait on slow players. Players'
//! `PlaybackStatus` changes are watched too (D-Bus signals, no polling).

use std::collections::HashMap;

use futures_util::StreamExt as _;
use futures_util::future::join_all;
use tokio::sync::{mpsc, watch};
use zbus::message::Type;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, MatchRule, MessageStream, Proxy};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Debug, Clone, Copy)]
enum Command {
    Pause,
    Resume,
}

pub struct Media {
    commands: mpsc::UnboundedSender<Command>,
    /// The last player status change: `Some(true)` once one starts playing.
    playback: watch::Receiver<Option<bool>>,
}

impl Media {
    pub async fn spawn() -> zbus::Result<Self> {
        let bus = Connection::session().await?;
        let (commands, rx) = mpsc::unbounded_channel();
        let (playback_tx, playback) = watch::channel(None);
        let changes = playback_changes(&bus).await?;
        tokio::spawn(run(bus, rx));
        // Changes only: players repeat their status (Plezy every second
        // while playing), and each `true` would be media starting again.
        tokio::spawn(changes.for_each(move |playing| {
            playback_tx.send_if_modified(|last| {
                let changed = *last != Some(playing);
                if changed {
                    tracing::debug!(playing, "MPRIS playback changed");
                }
                *last = Some(playing);
                changed
            });
            std::future::ready(())
        }));
        Ok(Self { commands, playback })
    }

    /// Player status changes from now on: `true` when one starts playing.
    pub fn playback(&self) -> watch::Receiver<Option<bool>> {
        let mut playback = self.playback.clone();
        playback.mark_unchanged();
        playback
    }

    /// Pause every playing player.
    pub fn pause(&self) {
        let _ = self.commands.send(Command::Pause);
    }

    /// Resume the players the last [`Self::pause`] paused.
    pub fn resume(&self) {
        let _ = self.commands.send(Command::Resume);
    }
}

/// `PlaybackStatus` changes of any player, as `Playing` or not.
async fn playback_changes(
    bus: &Connection,
) -> zbus::Result<impl futures_util::Stream<Item = bool> + use<>> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .path(PATH)?
        .arg(0, PLAYER)?
        .build();
    let stream = MessageStream::for_match_rule(rule, bus, None).await?;
    Ok(stream.filter_map(|message| async move {
        let body = message.ok()?.body();
        let (_, changed, _) = body
            .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
            .ok()?;
        let status = String::try_from(changed.get("PlaybackStatus")?.try_clone().ok()?).ok()?;
        Some(status == "Playing")
    }))
}

async fn run(bus: Connection, mut commands: mpsc::UnboundedReceiver<Command>) {
    let mut paused = Vec::new();
    while let Some(command) = commands.recv().await {
        match command {
            Command::Pause => paused = pause_playing(&bus).await,
            Command::Resume => resume(&bus, std::mem::take(&mut paused)).await,
        }
    }
}

/// Pause every playing player; returns the ones paused.
async fn pause_playing(bus: &Connection) -> Vec<String> {
    let players = match player_names(bus).await {
        Ok(players) => players,
        Err(e) => {
            tracing::warn!("listing MPRIS players failed: {e}");
            return Vec::new();
        }
    };
    let paused = join_all(players.into_iter().map(|name| async move {
        pause_if_playing(bus, &name)
            .await
            .inspect_err(|e| tracing::warn!(name, "MPRIS pause failed: {e}"))
            .is_ok_and(|paused| paused)
            .then_some(name)
    }))
    .await
    .into_iter()
    .flatten()
    .collect();
    tracing::info!(players = ?paused, "paused");
    paused
}

async fn resume(bus: &Connection, players: Vec<String>) {
    tracing::info!(?players, "resuming");
    join_all(players.iter().map(|name| async move {
        let played = async { player(bus, name).await?.call_method("Play", &()).await }.await;
        if let Err(e) = played {
            tracing::warn!(name, "MPRIS play failed: {e}");
        }
    }))
    .await;
}

async fn player_names(bus: &Connection) -> zbus::Result<Vec<String>> {
    let names = zbus::fdo::DBusProxy::new(bus).await?.list_names().await?;
    Ok(names
        .into_iter()
        .map(|name| name.to_string())
        .filter(|name| name.starts_with(PREFIX))
        .collect())
}

async fn player<'a>(bus: &'a Connection, name: &str) -> zbus::Result<Proxy<'a>> {
    Proxy::new(bus, name.to_owned(), PATH, PLAYER).await
}

async fn pause_if_playing(bus: &Connection, name: &str) -> zbus::Result<bool> {
    let player = player(bus, name).await?;
    if player.get_property::<String>("PlaybackStatus").await? != "Playing" {
        return Ok(false);
    }
    player.call_method("Pause", &()).await?;
    Ok(true)
}

//! Local media control over MPRIS on the session bus. Commands run in order
//! on one background task, so callers never wait on slow players. Players'
//! `PlaybackStatus` changes are watched too (D-Bus signals, no polling). A
//! player counts only while it plays here: one that mirrors another device's
//! playback says `Playing` too, and taking the audio for it, or pausing it
//! on a release, makes the two hosts fight over the pods.

use std::collections::HashMap;

use futures_util::StreamExt as _;
use futures_util::future::join_all;
use tokio::sync::{mpsc, watch};
use zbus::message::Type;
use zbus::names::BusName;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, MatchRule, MessageStream, Proxy};

use crate::pipewire::Streams;

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Debug)]
enum Command {
    /// With the processes playing audio, if known.
    Pause(Option<Vec<u32>>),
    Resume,
}

pub struct Media {
    commands: mpsc::UnboundedSender<Command>,
    /// Whether media plays here: `true` once a player does, audibly.
    playback: watch::Receiver<bool>,
    streams: Streams,
}

impl Media {
    pub async fn spawn(streams: Streams) -> zbus::Result<Self> {
        let bus = Connection::session().await?;
        let (commands, rx) = mpsc::unbounded_channel();
        let (playback_tx, playback) = watch::channel(false);
        let changes = changes(&bus).await?;
        tokio::spawn(run(bus.clone(), rx));
        tokio::spawn(track(bus, changes, streams.clone(), playback_tx));
        Ok(Self {
            commands,
            playback,
            streams,
        })
    }

    /// Changes from now on: `true` when media starts playing here.
    pub fn playback(&self) -> watch::Receiver<bool> {
        let mut playback = self.playback.clone();
        playback.mark_unchanged();
        playback
    }

    /// Pause every player playing here, by the streams running now (they
    /// change as the audio is released).
    pub fn pause(&self) {
        let streams = self.streams.borrow().clone();
        let _ = self.commands.send(Command::Pause(streams));
    }

    /// Resume the players the last [`Self::pause`] paused.
    pub fn resume(&self) {
        let _ = self.commands.send(Command::Resume);
    }
}

/// What a player's connection (its unique bus name) did.
enum Change {
    Status { player: String, playing: bool },
    Gone(String),
}

/// `PlaybackStatus` changes of any player, and connections leaving the bus.
async fn changes(
    bus: &Connection,
) -> zbus::Result<impl futures_util::Stream<Item = Change> + use<>> {
    let status = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .path(PATH)?
        .arg(0, PLAYER)?
        .build();
    let status = MessageStream::for_match_rule(status, bus, None)
        .await?
        .filter_map(|message| async move {
            let message = message.ok()?;
            let player = message.header().sender()?.to_string();
            let (_, changed, _) = message
                .body()
                .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
                .ok()?;
            let status = String::try_from(changed.get("PlaybackStatus")?.try_clone().ok()?).ok()?;
            Some(Change::Status {
                player,
                playing: status == "Playing",
            })
        });
    let gone = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg(2, "")?
        .build();
    let gone = MessageStream::for_match_rule(gone, bus, None)
        .await?
        .filter_map(|message| async move {
            let (name, _, _) = message
                .ok()?
                .body()
                .deserialize::<(String, String, String)>()
                .ok()?;
            name.starts_with(':').then_some(Change::Gone(name))
        });
    Ok(futures_util::stream::select(status, gone))
}

/// Follow the players that say `Playing` and publish whether one of them
/// plays here: its process has a running output stream. Changes only:
/// players repeat their status (Plezy every second while playing), and each
/// `true` would be media starting again.
async fn track(
    bus: Connection,
    changes: impl futures_util::Stream<Item = Change>,
    streams: Streams,
    playback: watch::Sender<bool>,
) {
    let mut changes = std::pin::pin!(changes);
    // `None` once PipeWire's thread is gone: every player counts again.
    let mut streams = Some(streams);
    // By unique bus name, with the process.
    let mut playing = HashMap::<String, u32>::new();
    loop {
        tokio::select! {
            change = changes.next() => match change {
                Some(Change::Status { player, playing: true }) => match pid(&bus, &player).await {
                    Ok(pid) => {
                        playing.insert(player, pid);
                    }
                    Err(e) => tracing::debug!(player, "MPRIS player's process: {e}"),
                },
                Some(Change::Status { player, playing: false } | Change::Gone(player)) => {
                    playing.remove(&player);
                }
                None => return,
            },
            () = stream_change(&mut streams) => {}
        }
        let running = streams
            .as_ref()
            .and_then(|streams| streams.borrow().clone());
        let here = playing
            .values()
            .any(|&player| audible(player, running.as_deref()));
        playback.send_if_modified(|last| {
            let changed = *last != here;
            if changed {
                tracing::debug!(here, players = ?playing, streams = ?running, "playback changed");
            }
            *last = here;
            changed
        });
    }
}

/// The next change of the running streams; never, once their sender is gone.
async fn stream_change(streams: &mut Option<Streams>) {
    match streams {
        Some(changes) => {
            if changes.changed().await.is_err() {
                *streams = None;
            }
        }
        None => std::future::pending().await,
    }
}

/// The process behind a bus name.
async fn pid(bus: &Connection, name: &str) -> zbus::Result<u32> {
    let name = BusName::try_from(name)?;
    Ok(zbus::fdo::DBusProxy::new(bus)
        .await?
        .get_connection_unix_process_id(name)
        .await?)
}

/// Whether a player's process plays through one of the running streams;
/// always while they're unknown (no PipeWire).
fn audible(player: u32, streams: Option<&[u32]>) -> bool {
    streams.is_none_or(|streams| streams.iter().any(|&stream| plays_for(stream, player)))
}

/// A stream is a player's when its process is the player's, a descendant
/// (browsers play from a child), or in the same app scope (Flatpak: the
/// player is on the bus through a proxy). Not any shared cgroup: apps
/// started by the compositor share its.
fn plays_for(stream: u32, player: u32) -> bool {
    let read = |pid: u32, file: &str| std::fs::read_to_string(format!("/proc/{pid}/{file}")).ok();
    let parent = |pid: &u32| read(*pid, "stat").as_deref().and_then(parent_of);
    let scope = |pid: u32| {
        read(pid, "cgroup")
            .as_deref()
            .and_then(app_scope)
            .map(str::to_owned)
    };
    std::iter::successors(Some(stream), parent).any(|pid| pid == player)
        || scope(player).is_some_and(|player| scope(stream) == Some(player))
}

/// The parent from `/proc/<pid>/stat`: `pid (comm) state ppid …`, where
/// `comm` may hold anything; none for init's children.
fn parent_of(stat: &str) -> Option<u32> {
    let (_, rest) = stat.rsplit_once(')')?;
    let parent = rest.split_whitespace().nth(1)?.parse().ok()?;
    (parent > 1).then_some(parent)
}

/// The cgroup from `/proc/<pid>/cgroup` when it's one app's (systemd's
/// `app-<launcher>-<id>.scope` or `.service`).
fn app_scope(cgroup: &str) -> Option<&str> {
    let path = cgroup.lines().find_map(|line| line.strip_prefix("0::"))?;
    let unit = path.rsplit('/').next()?;
    unit.starts_with("app-").then_some(path)
}

async fn run(bus: Connection, mut commands: mpsc::UnboundedReceiver<Command>) {
    let mut paused = Vec::new();
    while let Some(command) = commands.recv().await {
        match command {
            Command::Pause(streams) => paused = pause_playing(&bus, streams.as_deref()).await,
            Command::Resume => resume(&bus, std::mem::take(&mut paused)).await,
        }
    }
}

/// Pause every player playing here; returns the ones paused.
async fn pause_playing(bus: &Connection, streams: Option<&[u32]>) -> Vec<String> {
    let players = match player_names(bus).await {
        Ok(players) => players,
        Err(e) => {
            tracing::warn!("listing MPRIS players failed: {e}");
            return Vec::new();
        }
    };
    let paused = join_all(players.into_iter().map(|name| async move {
        pause_if_playing(bus, &name, streams)
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

async fn pause_if_playing(
    bus: &Connection,
    name: &str,
    streams: Option<&[u32]>,
) -> zbus::Result<bool> {
    let player = player(bus, name).await?;
    if player.get_property::<String>("PlaybackStatus").await? != "Playing" {
        return Ok(false);
    }
    if !audible(pid(bus, name).await?, streams) {
        tracing::debug!(name, "playing elsewhere, left alone");
        return Ok(false);
    }
    player.call_method("Pause", &()).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_survives_odd_process_names() {
        assert_eq!(parent_of("812 (a) b (c) S 77 812 812 0 -1"), Some(77));
        assert_eq!(parent_of("812 (init child) S 1 812"), None);
    }

    #[test]
    fn only_app_cgroups_are_scopes() {
        let app =
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-flatpak-x-1.scope\n";
        assert!(app_scope(app).is_some());
        let compositor =
            "0::/user.slice/user-1000.slice/user@1000.service/session.slice/niri.service\n";
        assert!(app_scope(compositor).is_none());
    }

    #[test]
    fn a_stream_is_its_process_and_its_ancestors() {
        let me = std::process::id();
        let parent = std::os::unix::process::parent_id();
        assert!(plays_for(me, me));
        assert!(plays_for(me, parent), "a descendant's stream");
        assert!(audible(me, None), "unknown streams count");
        assert!(!audible(me, Some(&[])));
    }
}

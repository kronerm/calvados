//! The pods' audio card profile, set natively through PipeWire. `off` keeps
//! the Bluetooth link but doesn't stream, so the laptop can't take the audio
//! from the phone just by connecting. Also restarts the pods' output node
//! when it doesn't stream after a take (see [`Audio::restart_stream`]).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use bluer::Address;
use pipewire as pw;
use pw::device::{Device, DeviceChangeMask, DeviceListener};
use pw::node::Node;
use pw::spa::node::command::{NodeCommand, NodeCommandId};
use pw::spa::param::ParamType;
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{Object, Pod, Property, Value, deserialize::PodDeserializer};
use pw::spa::sys::{
    SPA_PARAM_AVAILABILITY_no, SPA_PARAM_PROFILE_available, SPA_PARAM_PROFILE_index,
    SPA_PARAM_PROFILE_name, SPA_PARAM_PROFILE_save,
};
use pw::spa::utils::{Id, SpaTypes};
use pw::types::ObjectType;

/// How long after a request the profile is held against changes: the card
/// may not exist yet, and WirePlumber restores a remembered profile right
/// after creating it. Later changes (e.g. by hand) are left alone, but the
/// requested profile is selected again whenever it becomes available again:
/// when the pods drop A2DP (a stream start they never answered, aborted by
/// BlueZ) the card falls back to `off`, and nothing else restores it.
const ENFORCE_WINDOW: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy)]
pub enum Profile {
    Off,
    A2dp,
}

impl Profile {
    fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::A2dp => "a2dp-sink",
        }
    }

    /// Have WirePlumber remember `off` only, so the card starts `off` on
    /// later connects, before we're even asked.
    fn save(self) -> bool {
        matches!(self, Self::Off)
    }
}

struct Request {
    card: String,
    profile: Profile,
}

/// What the PipeWire thread is asked to do.
enum Message {
    Profile(Request),
    /// Suspend the output node whose name starts with this.
    RestartStream(String),
}

/// Sends requests to the PipeWire thread.
pub struct Audio(pw::channel::Sender<Message>);

impl Audio {
    pub fn spawn() -> Self {
        let (requests, rx) = pw::channel::channel();
        let spawned = std::thread::Builder::new()
            .name("pipewire".into())
            .spawn(|| {
                if let Err(e) = run(rx) {
                    tracing::warn!("PipeWire unavailable, audio profile left alone: {e}");
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("PipeWire thread: {e}");
        }
        Self(requests)
    }

    pub fn set(&self, pods: Address, profile: Profile) {
        let card = format!("bluez_card.{}", pods.to_string().replace(':', "_"));
        let _ = self.0.send(Message::Profile(Request { card, profile }));
    }

    /// Suspend the pods' output node; with streams linked it starts again
    /// at once, acquiring the A2DP transport anew. After a take, a start sent
    /// while the pods were still switching is lost: the sink runs, BlueZ's
    /// transport stays `idle`, and players block. Right at the take, the
    /// suspend rebooted the pods (once).
    pub fn restart_stream(&self, pods: Address) {
        let node = format!("bluez_output.{}", pods.to_string().replace(':', "_"));
        let _ = self.0.send(Message::RestartStream(node));
    }
}

/// A bound `bluez_card.*` device and what we know of its profiles.
struct Card {
    name: String,
    device: Rc<Device>,
    _listener: DeviceListener,
    /// Profile name → index and availability, from `EnumProfile`.
    profiles: HashMap<String, Listed>,
    active: Option<String>,
    /// Set but not yet reported active: don't send it again.
    pending: Option<&'static str>,
}

/// A profile as `EnumProfile` lists it.
#[derive(Debug, Clone, Copy)]
struct Listed {
    index: i32,
    available: bool,
}

/// Why [`State::enforce`] runs.
#[derive(Debug, Clone, Copy)]
enum Check {
    /// A request, or a change within the enforcement window.
    Change,
    /// The requested profile became available again.
    Revived,
}

#[derive(Default)]
struct State {
    /// Requested profile, enforced until the instant.
    desired: Option<(Request, Instant)>,
    /// By PipeWire global id.
    cards: HashMap<u32, Card>,
    /// `bluez_output.*` nodes by global id, with their names.
    outputs: HashMap<u32, (String, Node)>,
}

impl State {
    /// Correct the requested card's profile if it differs: while enforcing,
    /// or when it just became available again.
    fn enforce(&mut self, check: Check) {
        let Some((request, until)) = &self.desired else {
            return;
        };
        let wanted = request.profile.name();
        if matches!(check, Check::Change) && Instant::now() >= *until {
            return;
        }
        let Some(card) = self
            .cards
            .values_mut()
            .find(|card| card.name == request.card)
        else {
            return tracing::debug!(card = request.card, wanted, "card not there yet");
        };
        let (Some(active), Some(&Listed { index, available })) =
            (&card.active, card.profiles.get(wanted))
        else {
            return tracing::debug!(
                wanted,
                active = ?card.active,
                profiles = card.profiles.len(),
                "card profiles not known yet"
            );
        };
        if active == wanted || card.pending == Some(wanted) {
            return;
        }
        if !available {
            return tracing::debug!(profile = wanted, "profile unavailable for now");
        }
        tracing::info!(profile = wanted, was = %active, ?check, "setting audio profile");
        card.pending = Some(wanted);
        match profile_pod(index, request.profile.save()) {
            Ok(bytes) => match Pod::from_bytes(&bytes) {
                Some(pod) => card.device.set_param(ParamType::Profile, 0, pod),
                None => tracing::warn!("invalid profile pod"),
            },
            Err(e) => tracing::warn!("building profile pod: {e:?}"),
        }
    }
}

fn run(requests: pw::channel::Receiver<Message>) -> Result<(), pw::Error> {
    pw::init();
    let main_loop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&main_loop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    let state = Rc::new(RefCell::new(State::default()));

    let _requests = requests.attach(main_loop.loop_(), {
        let state = state.clone();
        move |message| {
            let mut state = state.borrow_mut();
            match message {
                Message::Profile(request) => {
                    state.desired = Some((request, Instant::now() + ENFORCE_WINDOW));
                    state.enforce(Check::Change);
                }
                Message::RestartStream(prefix) => {
                    match state
                        .outputs
                        .values()
                        .find(|(name, _)| name.starts_with(&prefix))
                    {
                        Some((name, node)) => {
                            tracing::info!(node = name, "restarting the A2DP stream");
                            node.send_command(&NodeCommand::new(NodeCommandId::SUSPEND));
                        }
                        None => tracing::debug!(prefix, "no output node to restart"),
                    }
                }
            }
        }
    });

    let _registry_listener = registry
        .add_listener_local()
        .global({
            let state = Rc::downgrade(&state);
            let registry = registry.downgrade();
            move |global| {
                if let (ObjectType::Node, Some(state), Some(registry)) =
                    (&global.type_, state.upgrade(), registry.upgrade())
                {
                    let name = global.props.and_then(|props| props.get("node.name"));
                    let Some(name) = name.filter(|name| name.starts_with("bluez_output.")) else {
                        return;
                    };
                    match registry.bind::<Node, _>(global) {
                        Ok(node) => {
                            tracing::debug!(name, id = global.id, "output node");
                            state
                                .borrow_mut()
                                .outputs
                                .insert(global.id, (name.to_owned(), node));
                        }
                        Err(e) => tracing::warn!("binding {name}: {e}"),
                    }
                    return;
                }
                let name = global.props.and_then(|props| props.get("device.name"));
                let (ObjectType::Device, Some(name), Some(state), Some(registry)) =
                    (&global.type_, name, state.upgrade(), registry.upgrade())
                else {
                    return;
                };
                if !name.starts_with("bluez_card.") {
                    return;
                }
                tracing::debug!(name, id = global.id, "audio card");
                let device: Rc<Device> = match registry.bind(global) {
                    Ok(device) => Rc::new(device),
                    Err(e) => return tracing::warn!("binding {name}: {e}"),
                };
                let listener = device
                    .add_listener_local()
                    // Subscribing reports the profiles once: re-read the list
                    // and the active one whenever the card's params change.
                    .info({
                        let device = Rc::downgrade(&device);
                        move |info| {
                            let changed = info.change_mask().contains(DeviceChangeMask::PARAMS);
                            if let (true, Some(device)) = (changed, device.upgrade()) {
                                [ParamType::EnumProfile, ParamType::Profile]
                                    .into_iter()
                                    .for_each(|id| device.enum_params(0, Some(id), 0, u32::MAX));
                            }
                        }
                    })
                    .param({
                        let state = Rc::downgrade(&state);
                        let id = global.id;
                        move |_seq, kind, _index, _next, pod| {
                            if let (Some(state), Some(pod)) = (state.upgrade(), pod) {
                                on_profile_param(&state, id, kind, pod);
                            }
                        }
                    })
                    .register();
                device.subscribe_params(&[ParamType::EnumProfile, ParamType::Profile]);
                let card = Card {
                    name: name.to_owned(),
                    device,
                    _listener: listener,
                    profiles: HashMap::new(),
                    active: None,
                    pending: None,
                };
                state.borrow_mut().cards.insert(global.id, card);
            }
        })
        .global_remove({
            let state = Rc::downgrade(&state);
            move |id| {
                if let Some(state) = state.upgrade() {
                    let mut state = state.borrow_mut();
                    state.cards.remove(&id);
                    state.outputs.remove(&id);
                }
            }
        })
        .register();

    main_loop.run();
    Ok(())
}

/// Record an `EnumProfile` entry or the active `Profile`, then re-check.
fn on_profile_param(state: &RefCell<State>, card: u32, kind: ParamType, pod: &Pod) {
    let Some((name, listed)) = parse_profile(pod) else {
        return tracing::debug!(?kind, "unparsed profile param");
    };
    let mut state = state.borrow_mut();
    let Some(card) = state.cards.get_mut(&card) else {
        return;
    };
    let check = match kind {
        ParamType::EnumProfile => {
            let was = card.profiles.insert(name.clone(), listed);
            let revived = listed.available && was.is_some_and(|was| !was.available);
            if revived {
                tracing::debug!(card = card.name, profile = name, "profile available again");
            }
            match revived {
                true => Check::Revived,
                false => Check::Change,
            }
        }
        ParamType::Profile => {
            tracing::debug!(card = card.name, profile = name, "active profile");
            card.pending = None;
            card.active = Some(name);
            Check::Change
        }
        _ => return,
    };
    state.enforce(check);
}

/// Name, index and availability (unknown counts as available) of a profile
/// object pod.
fn parse_profile(pod: &Pod) -> Option<(String, Listed)> {
    let (_, Value::Object(object)) = PodDeserializer::deserialize_any_from(pod.as_bytes()).ok()?
    else {
        return None;
    };
    let index = object.properties.iter().find_map(|p| match &p.value {
        Value::Int(index) if p.key == SPA_PARAM_PROFILE_index => Some(*index),
        _ => None,
    })?;
    let name = object.properties.iter().find_map(|p| match &p.value {
        Value::String(name) if p.key == SPA_PARAM_PROFILE_name => Some(name.clone()),
        _ => None,
    })?;
    let available = !object.properties.iter().any(|p| {
        p.key == SPA_PARAM_PROFILE_available
            && matches!(p.value, Value::Id(Id(id)) if id == SPA_PARAM_AVAILABILITY_no)
    });
    Some((name, Listed { index, available }))
}

/// The `Profile` param selecting `index`, as `wpctl set-profile` sends it.
fn profile_pod(index: i32, save: bool) -> Result<Vec<u8>, pw::spa::pod::serialize::GenError> {
    let object = Object {
        type_: SpaTypes::ObjectParamProfile.as_raw(),
        id: ParamType::Profile.as_raw(),
        properties: vec![
            Property::new(SPA_PARAM_PROFILE_index, Value::Int(index)),
            Property::new(SPA_PARAM_PROFILE_save, Value::Bool(save)),
        ],
    };
    let (cursor, _) =
        PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &Value::Object(object))?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_pod_round_trips() {
        let bytes = profile_pod(3, true).unwrap();
        let Ok((_, Value::Object(mut object))) = PodDeserializer::deserialize_any_from(&bytes)
        else {
            panic!("not an object pod");
        };
        object.properties.push(Property::new(
            SPA_PARAM_PROFILE_name,
            Value::String("off".into()),
        ));
        let bytes =
            PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &Value::Object(object))
                .unwrap()
                .0
                .into_inner();
        let pod = Pod::from_bytes(&bytes).unwrap();
        let (name, listed) = parse_profile(pod).unwrap();
        assert_eq!(
            (name.as_str(), listed.index, listed.available),
            ("off", 3, true)
        );
    }
}

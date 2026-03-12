//! Device state accumulated from received packets, for display.

use std::fmt;

use crate::MacAddress;
use crate::aacp::{
    AudioSource, BatteryComponent, BatteryInfo, Bud, ChargingStatus, ControlCommand, ControlId,
    DeviceConnectionStatus, DeviceRole, EarDetection, EarStatus, NoiseControlMode, Packet,
};

#[derive(Debug, Clone, Default)]
pub struct DeviceState {
    pub name: Option<String>,
    pub battery: Battery,
    /// Ear status by role; see [`Self::ears`] for physical buds.
    pub ear_detection: Option<EarDetection>,
    pub primary_bud: Option<Bud>,
    pub noise_mode: Option<NoiseControlMode>,
    /// Where `noise_mode` comes from.
    noise_source: NoiseSource,
    /// The modes last reported with both buds in (the one chosen) and with
    /// one out: the pods fall back to Transparency by themselves while a bud
    /// is out, and return to the chosen one when it's back.
    worn_noise_modes: WornNoiseModes,
    pub audio_source: Option<AudioSource>,
    /// Peer MACs seen this session: the hijack targets.
    pub peers: Vec<MacAddress>,
    /// Hosts (phones, computers) connected to the pods right now, us included.
    /// `None` until the first `ConnectedDevices` list: the per-device updates
    /// at session start arrive before it and are partial.
    pub hosts: Option<Vec<MacAddress>>,
}

/// Where [`DeviceState::noise_mode`] comes from.
#[derive(Debug, Clone, Copy, Default)]
enum NoiseSource {
    #[default]
    Reported,
    /// Carried over, and following the ears by the pods' rule until they
    /// report: after a reconnect that takes ~20 s.
    Expected,
}

#[derive(Debug, Clone, Copy, Default)]
struct WornNoiseModes {
    both: Option<NoiseControlMode>,
    one: Option<NoiseControlMode>,
}

/// Latest reading per component; `None` when unknown or absent.
#[derive(Debug, Clone, Default)]
pub struct Battery {
    pub left: Option<BatteryInfo>,
    pub right: Option<BatteryInfo>,
    pub case: Option<BatteryInfo>,
}

impl DeviceState {
    /// What a new session with the same pods starts from: what's only shown
    /// (name, noise mode, battery as last known) until the pods report it,
    /// which after a reconnect can take ~20 s (the settings half of the
    /// burst is held back). Routing state starts empty.
    pub fn carried_over(&self) -> Self {
        let last_known = |info: Option<BatteryInfo>| {
            info.map(|info| BatteryInfo {
                status: Some(ChargingStatus::Disconnected),
                ..info
            })
        };
        Self {
            name: self.name.clone(),
            noise_mode: self.noise_mode,
            noise_source: NoiseSource::Expected,
            worn_noise_modes: self.worn_noise_modes,
            battery: Battery {
                left: last_known(self.battery.left),
                right: last_known(self.battery.right),
                case: last_known(self.battery.case),
            },
            ..Self::default()
        }
    }

    /// The case's level from the pods' BLE advertisement: the level only,
    /// the status as the pods report it (last known while no bud is inside,
    /// which is when the advertisement knows better).
    pub fn advertised_case_level(&mut self, level: u8) {
        let status = self
            .battery
            .case
            .map_or(Some(ChargingStatus::Disconnected), |case| case.status);
        self.battery.case = Some(BatteryInfo {
            component: BatteryComponent::Case,
            level,
            status,
        });
    }

    /// Both buds put away: each in the case, or gone without a live battery
    /// reading. A link drop then is the case closing; otherwise it's
    /// unexpected (the primary bud handing over, the pods rebooting, out of
    /// range), also while a bud just taken out hasn't reported its ear yet.
    /// Unknown ears count as stowed.
    pub fn is_stowed(&self) -> bool {
        let gone = |info: Option<BatteryInfo>| {
            info.is_none_or(|info| matches!(info.status, Some(ChargingStatus::Disconnected)))
        };
        let away = |ear, battery| match ear {
            EarStatus::InCase => true,
            EarStatus::Disconnected => gone(battery),
            _ => false,
        };
        self.ears().is_none_or(|(left, right)| {
            away(left, self.battery.left) && away(right, self.battery.right)
        })
    }

    pub fn apply(&mut self, packet: &Packet) {
        match packet {
            Packet::Battery(infos) => infos.iter().for_each(|info| self.battery.update(*info)),
            Packet::EarDetection(ear) => {
                self.ear_detection = Some(*ear);
                self.battery_follows_ears();
                if let NoiseSource::Expected = self.noise_source {
                    self.noise_mode = self.expected_noise_mode();
                }
            }
            Packet::PrimaryBud(primary) => self.primary_bud = Some(primary.bud),
            Packet::DeviceInformation(fields) => self.name = fields.first().cloned(),
            Packet::AudioSource(source) => self.audio_source = Some(*source),
            Packet::Control(ControlCommand {
                id: ControlId::ListeningMode,
                value: [mode, ..],
            }) => {
                self.noise_mode = NoiseControlMode::try_from(*mode).ok();
                self.noise_source = NoiseSource::Reported;
                match self.both_in_ear() {
                    true => self.worn_noise_modes.both = self.noise_mode,
                    false => self.worn_noise_modes.one = self.noise_mode,
                }
            }
            Packet::ConnectedDevices(macs) => {
                macs.iter().for_each(|mac| self.record_peer(*mac));
                self.hosts = Some(macs.clone());
            }
            Packet::DeviceConnection(
                device @ DeviceConnectionStatus {
                    role: DeviceRole::Host,
                    ..
                },
            ) => {
                if let Some(hosts) = &mut self.hosts {
                    hosts.retain(|mac| *mac != device.mac);
                    if device.connected {
                        hosts.push(device.mac);
                    }
                }
            }
            // Not every session gets a device list: the history still names
            // the other hosts, as handoff targets.
            Packet::ConnectionHistory(history) => history
                .connected_hosts()
                .into_iter()
                .for_each(|mac| self.record_peer(mac)),
            Packet::RoutingResponse(response) => response
                .from
                .into_iter()
                .for_each(|mac| self.record_peer(mac)),
            _ => {}
        }
    }

    /// The pods send no battery report when a bud leaves the case (the next
    /// came a minute later): a bud out of the case isn't charging, one gone
    /// quiet has a last-known level, and with none in the case its level is
    /// only last known (as they report it later).
    fn battery_follows_ears(&mut self) {
        let Some((left, right)) = self.ears() else {
            return;
        };
        // A bud gone quiet (asleep in the closed case, out of range) keeps
        // its level as last known; a live report revives it.
        let follow = |ear, info: &mut Option<BatteryInfo>| {
            let Some(info) = info.as_mut() else {
                return;
            };
            match (ear, info.status) {
                (EarStatus::InEar | EarStatus::NotInEar, Some(ChargingStatus::Charging)) => {
                    info.status = Some(ChargingStatus::Discharging);
                }
                (EarStatus::Disconnected, _) => info.status = Some(ChargingStatus::Disconnected),
                _ => {}
            }
        };
        follow(left, &mut self.battery.left);
        follow(right, &mut self.battery.right);
        if !matches!(left, EarStatus::InCase)
            && !matches!(right, EarStatus::InCase)
            && let Some(case) = self.battery.case.as_mut()
        {
            case.status = Some(ChargingStatus::Disconnected);
        }
    }

    /// The mode the pods use with the ears as they are, as last seen.
    fn expected_noise_mode(&self) -> Option<NoiseControlMode> {
        let modes = self.worn_noise_modes;
        let expected = match self.both_in_ear() {
            true => modes.both,
            false => modes.one,
        };
        expected.or(self.noise_mode)
    }

    fn both_in_ear(&self) -> bool {
        self.ear_detection.is_some_and(|ears| {
            matches!(ears.primary, EarStatus::InEar) && matches!(ears.secondary, EarStatus::InEar)
        })
    }

    fn record_peer(&mut self, mac: MacAddress) {
        if !mac.is_nil() && !self.peers.contains(&mac) {
            self.peers.push(mac);
        }
    }

    /// Peers other than `local`.
    pub fn peers_except(&self, local: MacAddress) -> impl Iterator<Item = MacAddress> + '_ {
        self.peers.iter().copied().filter(move |mac| *mac != local)
    }

    /// `L:In R:Out`, once the sides are known.
    pub fn ears_summary(&self) -> Option<String> {
        self.ears()
            .map(|(left, right)| format!("L:{left} R:{right}"))
    }

    /// Ear status of the physical (left, right) buds, once the primary is known.
    pub fn ears(&self) -> Option<(EarStatus, EarStatus)> {
        let ear = self.ear_detection?;
        match self.primary_bud? {
            Bud::Left => Some((ear.primary, ear.secondary)),
            Bud::Right => Some((ear.secondary, ear.primary)),
        }
    }
}

impl Battery {
    fn update(&mut self, info: BatteryInfo) {
        let slot = match info.component {
            BatteryComponent::Left => &mut self.left,
            BatteryComponent::Right => &mut self.right,
            BatteryComponent::Case => &mut self.case,
        };
        // A disconnected component reports a bogus 0% (the case, whenever no
        // bud is inside); keep the last real level, marked disconnected.
        *slot = match (info.status, *slot) {
            (Some(ChargingStatus::Disconnected), last) => last.map(|last| BatteryInfo {
                status: info.status,
                ..last
            }),
            _ => Some(info),
        };
    }

    pub fn is_empty(&self) -> bool {
        [self.left, self.right, self.case]
            .iter()
            .all(Option::is_none)
    }

    /// Whether any component has a live reading (not only last-known ones,
    /// e.g. carried over from the previous session).
    pub fn is_live(&self) -> bool {
        [self.left, self.right, self.case]
            .iter()
            .flatten()
            .any(|info| !matches!(info.status, Some(ChargingStatus::Disconnected)))
    }
}

/// `L 80%+ R 75% Case (50%)`: `+` charging, parentheses for a last-known level.
impl fmt::Display for Battery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts = [("L", self.left), ("R", self.right), ("Case", self.case)]
            .into_iter()
            .filter_map(|(label, info)| {
                let info = info?;
                Some(match info.status {
                    Some(ChargingStatus::Charging) => format!("{label} {}%+", info.level),
                    Some(ChargingStatus::Disconnected) => format!("{label} ({}%)", info.level),
                    _ => format!("{label} {}%", info.level),
                })
            })
            .collect::<Vec<_>>();
        f.write_str(&parts.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aacp::PrimaryBud;

    fn battery(component: BatteryComponent, level: u8, status: ChargingStatus) -> BatteryInfo {
        BatteryInfo {
            component,
            level,
            status: Some(status),
        }
    }

    #[test]
    fn disconnected_component_keeps_last_known_level() {
        let mut state = DeviceState::default();
        let case =
            |level, status| Packet::Battery(vec![battery(BatteryComponent::Case, level, status)]);
        state.apply(&case(0, ChargingStatus::Disconnected));
        assert!(state.battery.is_empty());
        state.apply(&Packet::Battery(vec![
            battery(BatteryComponent::Left, 80, ChargingStatus::Charging),
            battery(BatteryComponent::Right, 75, ChargingStatus::Discharging),
            battery(BatteryComponent::Case, 60, ChargingStatus::NotCharging),
        ]));
        assert_eq!(state.battery.to_string(), "L 80%+ R 75% Case 60%");
        state.apply(&case(0, ChargingStatus::Disconnected));
        assert_eq!(state.battery.to_string(), "L 80%+ R 75% Case (60%)");
    }

    #[test]
    fn a_bud_leaving_the_case_stops_charging() {
        // Captured: the left bud left the case at 17:57:13, its battery
        // report (still charging) was renewed only a minute later.
        let mut state = DeviceState::default();
        state.apply(&Packet::PrimaryBud(PrimaryBud {
            bud: Bud::Right,
            active: false,
        }));
        state.apply(&Packet::Battery(vec![
            battery(BatteryComponent::Right, 66, ChargingStatus::Discharging),
            battery(BatteryComponent::Left, 66, ChargingStatus::Charging),
            battery(BatteryComponent::Case, 59, ChargingStatus::Discharging),
        ]));
        let ears = |secondary| {
            Packet::EarDetection(EarDetection {
                primary: EarStatus::InEar,
                secondary,
            })
        };
        state.apply(&ears(EarStatus::InCase));
        assert_eq!(state.battery.to_string(), "L 66%+ R 66% Case 59%");
        state.apply(&ears(EarStatus::NotInEar));
        assert_eq!(state.battery.to_string(), "L 66% R 66% Case (59%)");
    }

    #[test]
    fn a_carried_noise_mode_follows_the_ears_until_reported() {
        // Captured 17:47: ANC with both in, Transparency once a bud was out,
        // the primary into the case; after the reconnect one bud was in, then
        // both, and the pods reported ANC only 22 s later.
        let mut state = DeviceState::default();
        let mode = |mode: NoiseControlMode| {
            Packet::Control(ControlCommand {
                id: ControlId::ListeningMode,
                value: [mode as u8, 0, 0, 0],
            })
        };
        let ears = |secondary| {
            Packet::EarDetection(EarDetection {
                primary: EarStatus::InEar,
                secondary,
            })
        };
        let shown = |state: &DeviceState| state.noise_mode.map(|mode| mode as u8);
        state.apply(&ears(EarStatus::InEar));
        state.apply(&mode(NoiseControlMode::NoiseCancellation));
        state.apply(&ears(EarStatus::NotInEar));
        state.apply(&mode(NoiseControlMode::Transparency));
        let mut state = state.carried_over();
        state.apply(&ears(EarStatus::Disconnected));
        assert_eq!(shown(&state), Some(NoiseControlMode::Transparency as u8));
        state.apply(&ears(EarStatus::InEar));
        assert_eq!(
            shown(&state),
            Some(NoiseControlMode::NoiseCancellation as u8)
        );
        // Reported: the pods' word from then on.
        state.apply(&mode(NoiseControlMode::Adaptive));
        state.apply(&ears(EarStatus::NotInEar));
        assert_eq!(shown(&state), Some(NoiseControlMode::Adaptive as u8));
    }

    #[test]
    fn a_bud_asleep_in_the_case_counts_as_stowed() {
        // Captured Oct 6 17:22: both buds in the case, then the secondary
        // went quiet and the case closed; the laptop redialed in vain.
        let mut state = DeviceState::default();
        state.apply(&Packet::PrimaryBud(PrimaryBud {
            bud: Bud::Right,
            active: false,
        }));
        state.apply(&Packet::Battery(vec![
            battery(BatteryComponent::Right, 60, ChargingStatus::Discharging),
            battery(BatteryComponent::Left, 60, ChargingStatus::Discharging),
        ]));
        let ears = |secondary| {
            Packet::EarDetection(EarDetection {
                primary: EarStatus::InCase,
                secondary,
            })
        };
        state.apply(&ears(EarStatus::InCase));
        state.apply(&ears(EarStatus::Disconnected));
        assert!(state.is_stowed());
        // Taken out: its live report comes before its ear (Oct 4 17:14).
        state.apply(&Packet::Battery(vec![battery(
            BatteryComponent::Left,
            60,
            ChargingStatus::Discharging,
        )]));
        assert!(!state.is_stowed());
    }

    #[test]
    fn ears_map_roles_to_physical_buds() {
        let mut state = DeviceState::default();
        state.apply(&Packet::EarDetection(EarDetection {
            primary: EarStatus::InEar,
            secondary: EarStatus::InCase,
        }));
        assert_eq!(state.ears(), None);
        state.apply(&Packet::PrimaryBud(PrimaryBud {
            bud: Bud::Right,
            active: true,
        }));
        assert_eq!(state.ears(), Some((EarStatus::InCase, EarStatus::InEar)));
        assert_eq!(state.ears_summary().as_deref(), Some("L:Case R:In"));
    }
}

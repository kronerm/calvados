//! What the hosts show, shared so the Android notification and Linux's
//! `calvados watch` read the same: `L 80%👂 · R 🪫9%⚡ · Case 60%`.

use crate::aacp::{BatteryInfo, Bud, ChargingStatus, EarStatus, NoiseControlMode};
use crate::engine::Ownership;
use crate::firmware::{Firmware, Version};
use crate::state::DeviceState;

/// Where a bud is, as far as the display goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    InEar,
    InCase,
}

/// One battery: a bud or the case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Component {
    pub level: u8,
    pub charging: bool,
    /// Disconnected: `level` is the last one seen.
    pub last_known: bool,
    /// Buds only; `None` when out (or unknown).
    pub place: Option<Place>,
    /// The bud holding the link to the hosts (and the microphone).
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Buds {
    pub left: Option<Component>,
    pub right: Option<Component>,
    pub case: Option<Component>,
}

impl Buds {
    pub fn of(state: &DeviceState) -> Self {
        let (left, right) = state.ears().unzip();
        let battery = &state.battery;
        let primary = |bud| {
            matches!(
                (state.primary_bud, bud),
                (Some(Bud::Left), Bud::Left) | (Some(Bud::Right), Bud::Right)
            )
        };
        Self {
            left: battery
                .left
                .map(|info| component(info, left, primary(Bud::Left))),
            right: battery
                .right
                .map(|info| component(info, right, primary(Bud::Right))),
            case: battery.case.map(|info| component(info, None, false)),
        }
    }
}

fn component(info: BatteryInfo, ear: Option<EarStatus>, primary: bool) -> Component {
    Component {
        level: info.level,
        charging: matches!(info.status, Some(ChargingStatus::Charging)),
        last_known: matches!(info.status, Some(ChargingStatus::Disconnected)),
        place: match ear {
            Some(EarStatus::InEar) => Some(Place::InEar),
            Some(EarStatus::InCase) => Some(Place::InCase),
            _ => None,
        },
        primary,
    }
}

/// `Active`, `Taking…`, `Standby`.
pub fn ownership_label(ownership: Ownership) -> &'static str {
    match ownership {
        Ownership::Owning(_) => "Active",
        Ownership::Claiming(_) => "Taking…",
        Ownership::Idle => "Standby",
    }
}

/// The mode's name (`anc`, `transparency`, `adaptive`, `off`), as D-Bus
/// `NoiseMode` and the Android notification's actions have it; empty while
/// unknown.
pub fn mode_name(mode: Option<NoiseControlMode>) -> &'static str {
    match mode {
        Some(NoiseControlMode::Off) => "off",
        Some(NoiseControlMode::NoiseCancellation) => "anc",
        Some(NoiseControlMode::Transparency) => "transparency",
        Some(NoiseControlMode::Adaptive) => "adaptive",
        None => "",
    }
}

pub fn parse_mode(name: &str) -> Option<NoiseControlMode> {
    [
        NoiseControlMode::Off,
        NoiseControlMode::NoiseCancellation,
        NoiseControlMode::Transparency,
        NoiseControlMode::Adaptive,
    ]
    .into_iter()
    .find(|mode| mode_name(Some(*mode)) == name)
}

/// The running firmware as both hosts show it: `7.1.21` when known
/// exactly (UARP), else from `0x1D` major.minor and the build counter,
/// `7.1 (6663)`, or the reported string when it isn't in the known format.
pub fn firmware(firmware: &Firmware) -> Option<String> {
    let reported = |raw: &str| match (Version::of_running(raw), raw.rsplit_once('.')) {
        (Some(version), Some((_, build))) => format!("{version} ({build})"),
        _ => raw.to_owned(),
    };
    firmware
        .exact
        .map(|exact| exact.to_string())
        .or_else(|| firmware.reported.as_deref().map(reported))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aacp::{BatteryComponent, Bud, EarDetection, Packet, PrimaryBud};

    #[test]
    fn buds_are_placed_by_side_with_the_primary_marked() {
        let mut state = DeviceState::default();
        let info = |component, level, status| BatteryInfo {
            component,
            level,
            status: Some(status),
        };
        state.apply(&Packet::Battery(vec![
            info(BatteryComponent::Left, 80, ChargingStatus::Discharging),
            info(BatteryComponent::Right, 9, ChargingStatus::Charging),
            info(BatteryComponent::Case, 60, ChargingStatus::Discharging),
        ]));
        // The case keeps its level once no bud is inside.
        state.apply(&Packet::Battery(vec![info(
            BatteryComponent::Case,
            0,
            ChargingStatus::Disconnected,
        )]));
        state.apply(&Packet::EarDetection(EarDetection {
            primary: EarStatus::InEar,
            secondary: EarStatus::InCase,
        }));
        state.apply(&Packet::PrimaryBud(PrimaryBud {
            bud: Bud::Left,
            active: false,
        }));
        let buds = Buds::of(&state);
        let (left, right, case) = (buds.left.unwrap(), buds.right.unwrap(), buds.case.unwrap());
        assert!(left.primary && matches!(left.place, Some(Place::InEar)));
        assert!(!right.primary && right.charging && matches!(right.place, Some(Place::InCase)));
        assert!(case.last_known && case.level == 60);
        assert_eq!(Buds::of(&DeviceState::default()), Buds::default());
    }

    #[test]
    fn firmware_shows_the_exact_version_or_the_build() {
        let mut running = Firmware {
            reported: Some("71.2256000021000000.6663".into()),
            ..Firmware::default()
        };
        assert_eq!(firmware(&running).as_deref(), Some("7.1 (6663)"));
        running.exact = "7.1.21".parse().ok();
        assert_eq!(firmware(&running).as_deref(), Some("7.1.21"));
        let odd = Firmware {
            reported: Some("6A300".into()),
            ..Firmware::default()
        };
        assert_eq!(firmware(&odd).as_deref(), Some("6A300"));
        assert_eq!(firmware(&Firmware::default()), None);
    }
}

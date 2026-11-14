//! The pods' own settings: one control command each (opcode 0x09), sent by
//! a host and reported back by the pods in the startup burst and on every
//! change. The pods are the store: a value shows only once reported.
//! Encodings from LibrePods (`AAP Definitions.md`, 7A305) and our 9A348
//! captures; docs/features.md.

use crate::aacp::ControlId;

/// A setting the hosts offer, by its control id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodSetting {
    /// The pods lower the media and switch to transparency while you speak;
    /// the host ducks its volume ([`crate::engine::Effect::Duck`]).
    ConversationAwareness,
    /// Volume adapting to the surroundings (`AdaptiveVolume`).
    PersonalizedVolume,
    /// Adaptive mode's balance, 0 (more noise) to 100 (less).
    AdaptiveLevel,
    Mic,
    /// Optimized charge limit (`DynamicEndOfCharge`).
    ChargeLimit,
    /// Sounds from the case while charging (`InCaseTone`).
    CaseSounds,
    /// Noise control with only one bud in an ear.
    OneBudAnc,
    VolumeSwipe,
    VolumeSwipeLength,
    PressSpeed,
    HoldDuration,
    SleepDetection,
}

/// How a setting's value reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `01` on, `02` off.
    Toggle,
    /// 0-100.
    Level,
    /// `00`, `01`, … by these names.
    Choice(&'static [&'static str]),
}

const LENGTHS: &[&str] = &["default", "longer", "longest"];
const SPEEDS: &[&str] = &["default", "slower", "slowest"];

impl PodSetting {
    pub const ALL: [Self; 12] = [
        Self::ConversationAwareness,
        Self::PersonalizedVolume,
        Self::AdaptiveLevel,
        Self::Mic,
        Self::ChargeLimit,
        Self::CaseSounds,
        Self::OneBudAnc,
        Self::VolumeSwipe,
        Self::VolumeSwipeLength,
        Self::PressSpeed,
        Self::HoldDuration,
        Self::SleepDetection,
    ];

    pub fn id(self) -> ControlId {
        match self {
            Self::ConversationAwareness => ControlId::ConversationAwareness,
            Self::PersonalizedVolume => ControlId::AdaptiveVolume,
            Self::AdaptiveLevel => ControlId::AdaptiveNoiseLevel,
            Self::Mic => ControlId::MicMode,
            Self::ChargeLimit => ControlId::DynamicEndOfCharge,
            Self::CaseSounds => ControlId::InCaseTone,
            Self::OneBudAnc => ControlId::OneBudAncMode,
            Self::VolumeSwipe => ControlId::VolumeSwipeMode,
            Self::VolumeSwipeLength => ControlId::VolumeSwipeInterval,
            Self::PressSpeed => ControlId::DoubleClickInterval,
            Self::HoldDuration => ControlId::ClickHoldInterval,
            Self::SleepDetection => ControlId::SleepDetection,
        }
    }

    pub fn of(id: ControlId) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|setting| setting.id() as u8 == id as u8)
    }

    pub fn kind(self) -> Kind {
        match self {
            Self::AdaptiveLevel => Kind::Level,
            Self::Mic => Kind::Choice(&["auto", "right", "left"]),
            Self::VolumeSwipeLength => Kind::Choice(LENGTHS),
            Self::PressSpeed | Self::HoldDuration => Kind::Choice(SPEEDS),
            _ => Kind::Toggle,
        }
    }

    /// The key on D-Bus, the CLI and the Android intents.
    pub fn name(self) -> &'static str {
        match self {
            Self::ConversationAwareness => "conversation-awareness",
            Self::PersonalizedVolume => "personalized-volume",
            Self::AdaptiveLevel => "adaptive-level",
            Self::Mic => "mic",
            Self::ChargeLimit => "charge-limit",
            Self::CaseSounds => "case-sounds",
            Self::OneBudAnc => "one-bud-anc",
            Self::VolumeSwipe => "volume-swipe",
            Self::VolumeSwipeLength => "volume-swipe-length",
            Self::PressSpeed => "press-speed",
            Self::HoldDuration => "hold-duration",
            Self::SleepDetection => "sleep-detection",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|setting| setting.name() == name)
    }

    /// As both UIs show it.
    pub fn title(self) -> &'static str {
        match self {
            Self::ConversationAwareness => "Conversation Awareness",
            Self::PersonalizedVolume => "Personalized Volume",
            Self::AdaptiveLevel => "Adaptive level",
            Self::Mic => "Microphone",
            Self::ChargeLimit => "Optimized charge limit",
            Self::CaseSounds => "Case sounds",
            Self::OneBudAnc => "Noise control with one bud",
            Self::VolumeSwipe => "Volume swipe",
            Self::VolumeSwipeLength => "Swipe length",
            Self::PressSpeed => "Press speed",
            Self::HoldDuration => "Hold duration",
            Self::SleepDetection => "Sleep detection",
        }
    }

    /// A reported byte as text: `on`/`off`, the level, or the choice's
    /// name; `None` for a value outside the encoding.
    pub fn text(self, raw: u8) -> Option<String> {
        match self.kind() {
            Kind::Toggle => match raw {
                1 => Some("on".into()),
                2 => Some("off".into()),
                _ => None,
            },
            Kind::Level => (raw <= 100).then(|| raw.to_string()),
            Kind::Choice(names) => names.get(usize::from(raw)).map(|&name| name.to_owned()),
        }
    }

    /// Text as the byte to send: the inverse of [`Self::text`].
    pub fn parse(self, text: &str) -> Option<u8> {
        match self.kind() {
            Kind::Toggle => match text {
                "on" => Some(1),
                "off" => Some(2),
                _ => None,
            },
            Kind::Level => text.parse().ok().filter(|&level| level <= 100),
            Kind::Choice(names) => names
                .iter()
                .position(|&name| name == text)
                .and_then(|at| u8::try_from(at).ok()),
        }
    }

    /// The next value a tap gives: a toggle flips, a choice steps on, a
    /// level steps by a third (iOS offers 0, 50, 100).
    pub fn next(self, raw: u8) -> u8 {
        match self.kind() {
            Kind::Toggle => match raw {
                1 => 2,
                _ => 1,
            },
            Kind::Level => match raw {
                0..50 => 50,
                50..100 => 100,
                _ => 0,
            },
            Kind::Choice(names) => {
                let count = u8::try_from(names.len()).unwrap_or(1);
                (raw + 1) % count
            }
        }
    }
}

/// The values as the pods last reported them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PodSettings([Option<u8>; PodSetting::ALL.len()]);

impl PodSettings {
    pub fn get(&self, setting: PodSetting) -> Option<u8> {
        self.0[Self::index(setting)]
    }

    /// A control command's report; `false` when it's none of ours.
    pub fn reported(&mut self, id: ControlId, raw: u8) -> bool {
        PodSetting::of(id)
            .filter(|setting| setting.text(raw).is_some())
            .map(|setting| self.0[Self::index(setting)] = Some(raw))
            .is_some()
    }

    /// Whether the toggle `setting` is on.
    pub fn is_on(&self, setting: PodSetting) -> bool {
        self.get(setting) == Some(1)
    }

    fn index(setting: PodSetting) -> usize {
        PodSetting::ALL
            .iter()
            .position(|&each| each == setting)
            .expect("in ALL")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_and_parse_both_ways() {
        for setting in PodSetting::ALL {
            assert_eq!(PodSetting::from_name(setting.name()), Some(setting));
            assert_eq!(PodSetting::of(setting.id()), Some(setting));
            for raw in 0..=100 {
                if let Some(text) = setting.text(raw) {
                    assert_eq!(setting.parse(&text), Some(raw), "{setting:?} {raw}");
                }
            }
        }
        assert_eq!(PodSetting::Mic.text(2).as_deref(), Some("left"));
        assert_eq!(PodSetting::Mic.text(3), None);
        assert_eq!(PodSetting::ConversationAwareness.parse("on"), Some(1));
        assert_eq!(PodSetting::AdaptiveLevel.parse("101"), None);
    }

    #[test]
    fn taps_step_through_the_values() {
        assert_eq!(PodSetting::CaseSounds.next(1), 2);
        assert_eq!(PodSetting::CaseSounds.next(2), 1);
        assert_eq!(PodSetting::Mic.next(2), 0);
        assert_eq!(PodSetting::AdaptiveLevel.next(50), 100);
        assert_eq!(PodSetting::AdaptiveLevel.next(100), 0);
    }

    #[test]
    fn reports_of_ours_are_kept() {
        let mut settings = PodSettings::default();
        assert!(settings.reported(ControlId::AdaptiveVolume, 1));
        assert!(settings.is_on(PodSetting::PersonalizedVolume));
        assert!(!settings.reported(ControlId::ListeningMode, 2));
        assert!(!settings.reported(ControlId::SleepDetection, 7), "not a toggle value");
        assert_eq!(settings.get(PodSetting::SleepDetection), None);
    }
}

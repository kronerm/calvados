use nom::{IResult, Parser as _, bytes::complete::take};

use num_enum::TryFromPrimitive;

use super::byte;

pub const OPCODE: u16 = 0x09;

/// Control command identifiers (opcode 0x09). Toggles are `01`=on, `02`=off.
#[derive(Debug, Clone, Copy, Hash, TryFromPrimitive)]
#[cfg_attr(test, derive(PartialEq, Eq))]
#[repr(u8)]
pub enum ControlId {
    /// 00=auto, 01=right, 02=left
    MicMode = 0x01,
    ButtonSendMode = 0x05,
    /// 01=this host owns the audio link
    OwnsConnection = 0x06,
    EarDetectionConfig = 0x0A,
    /// See [`NoiseControlMode`].
    ListeningMode = 0x0D,
    VoiceTrigger = 0x12,
    SingleClickMode = 0x14,
    DoubleClickMode = 0x15,
    /// d1=right, d2=left (01=noise control, 05=Siri)
    ClickHoldMode = 0x16,
    /// 00=default, 01=slower, 02=slowest
    DoubleClickInterval = 0x17,
    /// 00=default, 01=slower, 02=slowest
    ClickHoldInterval = 0x18,
    /// Cycle bitmask: off=01, ANC=02, transparency=04, adaptive=08
    ListeningModeConfigs = 0x1A,
    OneBudAncMode = 0x1B,
    /// AirPods Max: 01=reversed, 02=default
    CrownRotationDirection = 0x1C,
    AutoAnswerMode = 0x1E,
    /// 0-100
    ChimeVolume = 0x1F,
    AutomaticConnection = 0x20,
    /// 00=default, 01=longer, 02=longest
    VolumeSwipeInterval = 0x23,
    CallManagement = 0x24,
    VolumeSwipeMode = 0x25,
    AdaptiveVolume = 0x26,
    SoftwareMute = 0x27,
    ConversationAwareness = 0x28,
    Ssl = 0x29,
    /// d1=enrolled, d2=enabled
    HearingAid = 0x2C,
    /// 0-100 (0 = max noise, 100 = max filtering)
    AdaptiveNoiseLevel = 0x2E,
    HpsGainSwipe = 0x2F,
    HeartRateMonitor = 0x30,
    InCaseTone = 0x31,
    SiriMultitone = 0x32,
    HearingAssist = 0x33,
    /// Allow "off" in the listening mode cycle
    AllowOffOption = 0x34,
    SleepDetection = 0x35,
    AllowAutoConnect = 0x36,
    PpeToggle = 0x37,
    PpeCapLevel = 0x38,
    /// Raw gestures bitmask: single=01, double=02, triple=04, long=08
    StemConfig = 0x39,
    /// 01=temporary, 02=permanent
    TemporaryPairing = 0x3A,
    DynamicEndOfCharge = 0x3B,
    SystemSiriMessage = 0x3C,
    HearingAidGeneric = 0x3D,
    UplinkEqBud = 0x3E,
    UplinkEqSource = 0x3F,
    /// 0-100
    InCaseToneVolume = 0x40,
    DisableButtonInput = 0x41,
}

/// Value of [`ControlId::ListeningMode`].
#[derive(Debug, Clone, Copy, Hash, TryFromPrimitive)]
#[cfg_attr(test, derive(PartialEq, Eq))]
#[repr(u8)]
pub enum NoiseControlMode {
    Off = 1,
    NoiseCancellation = 2,
    Transparency = 3,
    Adaptive = 4,
}

impl NoiseControlMode {
    /// The mode cycle the hosts offer: ANC → Transparency → Adaptive → ANC.
    /// Off is never offered; from Off or unknown, ANC.
    pub fn next(mode: Option<Self>) -> Self {
        match mode {
            Some(Self::NoiseCancellation) => Self::Transparency,
            Some(Self::Transparency) => Self::Adaptive,
            Some(Self::Adaptive | Self::Off) | None => Self::NoiseCancellation,
        }
    }
}

bitflags::bitflags! {
    /// Value of [`ControlId::ListeningModeConfigs`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ListeningModes: u8 {
        const OFF = 0x01;
        const NOISE_CANCELLATION = 0x02;
        const TRANSPARENCY = 0x04;
        const ADAPTIVE = 0x08;
    }
}

impl ListeningModes {
    pub fn of(mode: NoiseControlMode) -> Self {
        match mode {
            NoiseControlMode::Off => Self::OFF,
            NoiseControlMode::NoiseCancellation => Self::NOISE_CANCELLATION,
            NoiseControlMode::Transparency => Self::TRANSPARENCY,
            NoiseControlMode::Adaptive => Self::ADAPTIVE,
        }
    }
}

/// The modes press and hold cycles through: two at least, as the pods
/// require.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cycle(ListeningModes);

impl Cycle {
    /// The three modes the hosts offer.
    pub const OFFERED: Self = Self(
        ListeningModes::NOISE_CANCELLATION
            .union(ListeningModes::TRANSPARENCY)
            .union(ListeningModes::ADAPTIVE),
    );

    /// `None` with fewer than two modes.
    pub fn new(modes: ListeningModes) -> Option<Self> {
        (modes.iter().count() >= 2).then_some(Self(modes))
    }

    /// As the pods report it, unknown bits dropped.
    pub fn from_bits(bits: u8) -> Option<Self> {
        Self::new(ListeningModes::from_bits_truncate(bits))
    }

    pub fn modes(self) -> ListeningModes {
        self.0
    }

    pub fn contains(self, mode: NoiseControlMode) -> bool {
        self.0.contains(ListeningModes::of(mode))
    }

    /// With `mode` added or removed; `None` when fewer than two would be left.
    pub fn toggled(self, mode: NoiseControlMode) -> Option<Self> {
        Self::new(self.0.symmetric_difference(ListeningModes::of(mode)))
    }
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct ControlCommand {
    pub id: ControlId,
    pub value: [u8; 4],
}

/// `[id] [d1 d2 d3 d4]`
pub fn parse(input: &[u8]) -> IResult<&[u8], ControlCommand> {
    (byte, take(4usize))
        .map(|(id, value): (_, &[u8])| ControlCommand {
            id,
            value: value.try_into().expect("take(4)"),
        })
        .parse(input)
}

#[cfg(test)]
mod tests {
    use super::NoiseControlMode::{self, Adaptive, NoiseCancellation, Off, Transparency};
    use super::{Cycle, ListeningModes};

    #[test]
    fn a_cycle_keeps_two_modes() {
        let two = Cycle::OFFERED.toggled(Adaptive).unwrap();
        assert_eq!(two.modes().bits(), 0x06);
        assert!(two.contains(Transparency) && !two.contains(Adaptive));
        assert!(two.toggled(Transparency).is_none());
        assert_eq!(two.toggled(Adaptive), Some(Cycle::OFFERED));
        // Off counts: the pods take it once allowed.
        assert_eq!(
            two.toggled(Off).and_then(|c| c.toggled(NoiseCancellation)),
            Cycle::new(ListeningModes::OFF | ListeningModes::TRANSPARENCY)
        );
        assert_eq!(Cycle::from_bits(0xF4), None);
        assert_eq!(Cycle::from_bits(0x0E), Some(Cycle::OFFERED));
    }

    #[test]
    fn modes_cycle_without_off() {
        let cycle = std::iter::successors(Some(NoiseControlMode::next(None)), |m| {
            Some(NoiseControlMode::next(Some(*m)))
        })
        .take(4)
        .collect::<Vec<_>>();
        assert!(
            matches!(
                cycle[..],
                [NoiseCancellation, Transparency, Adaptive, NoiseCancellation]
            ),
            "{cycle:?}"
        );
        assert!(matches!(
            NoiseControlMode::next(Some(Off)),
            NoiseCancellation
        ));
    }
}

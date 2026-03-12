//! The two remembered switches, the same on every platform. Platforms store
//! them (under [`Setting::key`]); the [`crate::engine::Engine`] obeys them.

/// A remembered on/off setting, on until first turned off.
#[derive(Debug, Clone, Copy)]
pub enum Setting {
    /// Media and calls starting here take the audio.
    AutoSwitch,
    /// Connect the pods when they show up nearby, or drop while worn.
    AutoConnect,
}

impl Setting {
    /// The storage key (Linux: file name, Android: preference).
    pub fn key(self) -> &'static str {
        match self {
            Self::AutoSwitch => "auto_switch",
            Self::AutoConnect => "auto_connect",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub auto_switch: bool,
    pub auto_connect: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_switch: true,
            auto_connect: true,
        }
    }
}

impl Settings {
    /// Each setting as `stored` reports it.
    pub fn load(stored: impl Fn(Setting) -> bool) -> Self {
        Self {
            auto_switch: stored(Setting::AutoSwitch),
            auto_connect: stored(Setting::AutoConnect),
        }
    }

    pub fn get(self, setting: Setting) -> bool {
        match setting {
            Setting::AutoSwitch => self.auto_switch,
            Setting::AutoConnect => self.auto_connect,
        }
    }

    pub fn set(&mut self, setting: Setting, enabled: bool) {
        let slot = match setting {
            Setting::AutoSwitch => &mut self.auto_switch,
            Setting::AutoConnect => &mut self.auto_connect,
        };
        *slot = enabled;
    }
}

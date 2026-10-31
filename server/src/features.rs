// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hidden features: functions that are legally unclear and therefore off unless the administrator of the machine
//! switches them on in `/etc/david.toml` (`[features]`) or, above the file, in the environment (`DAVID_FEATURE_*`).
//! They are not a setting of the pages: what is off has no route (404), no button and no word anywhere in the
//! interface. The switches are read once at the start and carried in the state of the app.

use serde::Deserialize;

/// The switches. Every one is off by default.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Features {
    /// Administrators may download the original files of the library (one title, or an album as a ZIP archive).
    pub download: bool,
}

impl Features {
    /// The environment variable of the download switch (`1`, `true`, `yes` or `on` switch it on; anything else is off).
    pub const DOWNLOAD_ENV: &'static str = "DAVID_FEATURE_DOWNLOAD";

    /// The switches as the environment (and, below it, the file) set them: `env` looks up a variable.
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Features {
        let on = |key: &str| env(key).is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"));
        Features { download: on(Self::DOWNLOAD_ENV) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_is_off_unless_switched_on() {
        assert!(!Features::default().download);
        assert!(!Features::from_env(|_| None).download);
        for on in ["1", "true", "TRUE", " yes ", "on"] {
            assert!(Features::from_env(|_| Some(on.to_owned())).download, "{on}");
        }
        for off in ["0", "false", "", "no", "2"] {
            assert!(!Features::from_env(|_| Some(off.to_owned())).download, "{off}");
        }
    }

    #[test]
    fn the_table_of_the_file_parses_and_an_unknown_switch_is_a_mistake() {
        let f: Features = toml::from_str("download = true").unwrap();
        assert!(f.download);
        assert!(toml::from_str::<Features>("downloads = true").is_err());
    }
}

use std::fs;
use std::io;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImeTargetMode {
    FocusedControl = 1,
    MouseControl = 2,
}

impl ImeTargetMode {
    pub fn from_ini(value: i32) -> Self {
        match value {
            2 => Self::MouseControl,
            _ => Self::FocusedControl,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub ime_target_mode: ImeTargetMode,
    pub show_english_ibeam: bool,
    pub show_japanese_ibeam: bool,
    pub show_korean_ibeam: bool,
    pub show_fallback_badge: bool,
    pub play_english_sound: bool,
    pub play_japanese_sound: bool,
    pub play_korean_sound: bool,
    pub show_ime_tray_icon: bool,
    pub play_sounds: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ime_target_mode: ImeTargetMode::FocusedControl,
            show_english_ibeam: true,
            show_japanese_ibeam: true,
            show_korean_ibeam: true,
            show_fallback_badge: true,
            play_english_sound: true,
            play_japanese_sound: true,
            play_korean_sound: true,
            show_ime_tray_icon: true,
            play_sounds: true,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Self {
        let Ok(bytes) = fs::read(path) else {
            return Self::default();
        };
        let text = decode_ini_text(&bytes);

        let mut config = Self::default();
        let mut in_settings = false;

        for raw_line in text.lines() {
            let line = raw_line.trim().trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                in_settings = line[1..line.len() - 1].trim().eq_ignore_ascii_case("Settings");
                continue;
            }
            if !in_settings {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().parse::<i32>().ok();
            let as_bool = match value {
                Some(0) => Some(false),
                Some(1) => Some(true),
                _ => None,
            };

            match key.as_str() {
                "getimestatus" => {
                    if let Some(value) = value {
                        config.ime_target_mode = ImeTargetMode::from_ini(value);
                    }
                }
                "showenglishibeam" => set_bool(&mut config.show_english_ibeam, as_bool),
                "showjapaneseibeam" => set_bool(&mut config.show_japanese_ibeam, as_bool),
                "showkoreanibeam" => set_bool(&mut config.show_korean_ibeam, as_bool),
                "showfallbackbadge" => set_bool(&mut config.show_fallback_badge, as_bool),
                "playenglishsound" => set_bool(&mut config.play_english_sound, as_bool),
                "playjapanesesound" => set_bool(&mut config.play_japanese_sound, as_bool),
                "playkoreansound" => set_bool(&mut config.play_korean_sound, as_bool),
                "showimetrayicon" => set_bool(&mut config.show_ime_tray_icon, as_bool),
                "playsounds" => set_bool(&mut config.play_sounds, as_bool),
                _ => {}
            }
        }

        config
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let body = format!(
            concat!(
                "[Settings]\r\n",
                "GetIMEStatus={}\r\n",
                "ShowEnglishIBeam={}\r\n",
                "ShowJapaneseIBeam={}\r\n",
                "ShowKoreanIBeam={}\r\n",
                "ShowFallbackBadge={}\r\n",
                "PlayEnglishSound={}\r\n",
                "PlayJapaneseSound={}\r\n",
                "PlayKoreanSound={}\r\n",
                "ShowIMETrayIcon={}\r\n",
                "PlaySounds={}\r\n"
            ),
            self.ime_target_mode as i32,
            as_ini_bool(self.show_english_ibeam),
            as_ini_bool(self.show_japanese_ibeam),
            as_ini_bool(self.show_korean_ibeam),
            as_ini_bool(self.show_fallback_badge),
            as_ini_bool(self.play_english_sound),
            as_ini_bool(self.play_japanese_sound),
            as_ini_bool(self.play_korean_sound),
            as_ini_bool(self.show_ime_tray_icon),
            as_ini_bool(self.play_sounds),
        );

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, body)
    }
}


fn decode_ini_text(bytes: &[u8]) -> String {
    // AutoHotkey/Win32 INI files are commonly ASCII/ANSI, UTF-8, or UTF-16
    // with a BOM. The supported setting names and values are ASCII, so a
    // lossy fallback also preserves them in legacy code pages such as CP949.
    if bytes.starts_with(&[0xff, 0xfe]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn set_bool(slot: &mut bool, value: Option<bool>) {
    if let Some(value) = value {
        *slot = value;
    }
}

fn as_ini_bool(value: bool) -> i32 {
    i32::from(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_original_program() {
        let config = Config::default();
        assert_eq!(config.ime_target_mode, ImeTargetMode::FocusedControl);
        assert!(config.show_english_ibeam);
        assert!(config.show_japanese_ibeam);
        assert!(config.show_korean_ibeam);
        assert!(config.show_fallback_badge);
        assert!(config.play_sounds);
    }

    #[test]
    fn decodes_utf16_ini() {
        let source = "[Settings]\r\nShowKoreanIBeam=0\r\n";
        let mut bytes = vec![0xff, 0xfe];
        for unit in source.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let decoded = decode_ini_text(&bytes);
        assert!(decoded.contains("ShowKoreanIBeam=0"));
    }

}

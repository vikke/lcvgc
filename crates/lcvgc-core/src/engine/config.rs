use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

/// エンジン設定（TOMLファイルから読み込み可能）
#[derive(Debug, Deserialize, Default)]
pub struct Config {
    pub default_bpm: Option<f64>,
    pub ppq: Option<u16>,
    pub midi_ports: Option<HashMap<String, String>>,
}

impl Config {
    /// TOMLファイルから読み込み。ファイルが存在しない場合はデフォルト値を返す。
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        toml::from_str(&content).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn config_default_all_none() {
        let config = Config::default();
        assert!(config.default_bpm.is_none());
        assert!(config.ppq.is_none());
        assert!(config.midi_ports.is_none());
    }

    #[test]
    fn config_load_nonexistent_returns_default() {
        let path = Path::new("/tmp/lcvgc_test_nonexistent_config.toml");
        let config = Config::load(path).unwrap();
        assert!(config.default_bpm.is_none());
    }

    #[test]
    fn config_load_bpm() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "default_bpm = 140.0").unwrap();
        let config = Config::load(f.path()).unwrap();
        assert_eq!(config.default_bpm, Some(140.0));
        assert!(config.ppq.is_none());
    }

    #[test]
    fn config_load_ppq_and_midi_ports() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"ppq = 480

[midi_ports]
synth = "MIDI Port 1"
drums = "MIDI Port 2"
"#
        )
        .unwrap();
        let config = Config::load(f.path()).unwrap();
        assert_eq!(config.ppq, Some(480));
        let ports = config.midi_ports.unwrap();
        assert_eq!(ports.get("synth").unwrap(), "MIDI Port 1");
    }

    #[test]
    fn config_load_invalid_toml_returns_err() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "{{{{ invalid toml").unwrap();
        let result = Config::load(f.path());
        assert!(result.is_err());
    }
}

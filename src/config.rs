use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

pub const CONFIG_FILE_NAME: &str = "tui_config.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub show_footer: bool,
    pub show_preview: bool,
    pub sort_mode: String,
    pub pinned: Vec<String>,
    pub recent: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            show_footer: true,
            show_preview: true,
            sort_mode: "default".to_string(),
            pinned: vec![
                "crystaldiskinfo".to_string(),
                "crystaldiskmark".to_string(),
                "prime95_bat".to_string(),
                "rufus".to_string(),
                "tui_screen_test".to_string(),
            ],
            recent: Vec::new(),
        }
    }
}

fn dedup_strings(input: &[String], max_len: usize) -> Vec<String> {
    let mut seen = HashSet::with_capacity(input.len());
    let mut out = Vec::with_capacity(input.len());
    for raw in input {
        let s = raw.trim();
        if s.is_empty() {
            continue;
        }
        if seen.insert(s.to_string()) {
            out.push(s.to_string());
            if max_len > 0 && out.len() >= max_len {
                break;
            }
        }
    }
    out
}

impl Config {
    /// Reads tui_config.json from `config_dir`, returning Default if absent or corrupt (backing up corrupt files to .bak).
    pub fn load(config_dir: &Path) -> Self {
        let path = config_dir.join(CONFIG_FILE_NAME);
        let Ok(bytes) = fs::read(&path) else {
            return Self::default();
        };

        let mut cfg: Config = match serde_json::from_slice(&bytes) {
            Ok(c) => c,
            Err(_) => {
                let mut bak = path.as_os_str().to_os_string();
                bak.push(".bak");
                let _ = fs::rename(&path, Path::new(&bak));
                return Self::default();
            }
        };

        match cfg.sort_mode.as_str() {
            "default" | "name" | "type" => {}
            _ => cfg.sort_mode = "default".to_string(),
        }
        cfg.pinned = dedup_strings(&cfg.pinned, 0);
        cfg.recent = dedup_strings(&cfg.recent, 10);
        cfg
    }

    /// Writes `self` as indented UTF-8 JSON to tui_config.json inside `config_dir` atomically.
    pub fn save(&self, config_dir: &Path) -> io::Result<()> {
        let path = config_dir.join(CONFIG_FILE_NAME);
        let mut data = serde_json::to_vec_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        data.push(b'\n');

        let mut tmp_os = path.as_os_str().to_os_string();
        tmp_os.push(".tmp");
        let tmp_path = Path::new(&tmp_os);

        let write_res = (|| -> io::Result<()> {
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(tmp_path)?;
            f.write_all(&data)?;
            f.sync_all()?;
            Ok(())
        })();

        if let Err(e) = write_res {
            let _ = fs::remove_file(tmp_path);
            return Err(e);
        }

        if let Err(e) = fs::rename(tmp_path, &path) {
            let _ = fs::remove_file(tmp_path);
            return Err(e);
        }
        Ok(())
    }

    pub fn is_pinned(&self, tool_id: &str) -> bool {
        self.pinned.iter().any(|id| id == tool_id)
    }

    /// Adds or removes `tool_id` from `pinned`. Returns `true` if now pinned.
    pub fn toggle_pin(&mut self, tool_id: &str) -> bool {
        if let Some(pos) = self.pinned.iter().position(|id| id == tool_id) {
            self.pinned.remove(pos);
            false
        } else {
            self.pinned.push(tool_id.to_string());
            true
        }
    }

    /// Records `tool_id` at the front of `recent` (deduplicated, capped at 10).
    pub fn add_recent(&mut self, tool_id: &str) {
        if tool_id.is_empty() {
            return;
        }
        let mut out = Vec::with_capacity(10);
        out.push(tool_id.to_string());
        for id in &self.recent {
            if id != tool_id {
                out.push(id.clone());
                if out.len() >= 10 {
                    break;
                }
            }
        }
        self.recent = out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_save_corrupt_heal_and_dedup() {
        let dir = std::env::temp_dir().join(format!("tb_cfg_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut cfg = Config::default();
        cfg.pinned = vec![
            "aida64".into(),
            "cpuz".into(),
            "aida64".into(),
            "".into(),
            " gpuz ".into(),
        ];
        cfg.recent = (1..=12).map(|i| format!("r{i}")).collect();
        cfg.recent.insert(2, "r1".into());

        cfg.save(&dir).expect("save config");
        assert!(!dir.join("tui_config.json.tmp").exists());

        let loaded = Config::load(&dir);
        assert_eq!(loaded.pinned, vec!["aida64", "cpuz", "gpuz"]);
        assert_eq!(loaded.recent.len(), 10);

        let corrupt_path = dir.join(CONFIG_FILE_NAME);
        fs::write(&corrupt_path, b"{corrupt json!!!").unwrap();

        let healed = Config::load(&dir);
        assert_eq!(healed.pinned.len(), Config::default().pinned.len());
        assert!(dir.join("tui_config.json.bak").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}

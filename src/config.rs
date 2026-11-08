use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// File and directory names to leave out of the tree.
    pub ignore: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config { ignore: vec![".DS_Store".into()] }
    }
}

/// `$XDG_CONFIG_HOME/sbar/config.toml`, falling back to `~/.config/sbar/config.toml`.
fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("sbar/config.toml"))
}

/// Load the config file, or the defaults if there is none. A file that exists
/// but doesn't parse is an error, so typos don't go unnoticed.
pub fn load() -> Result<Config, String> {
    let Some(path) = path() else { return Ok(Config::default()) };
    match fs::read_to_string(&path) {
        Ok(s) => toml::from_str(&s).map_err(|e| format!("{}: {e}", path.display())),
        Err(_) => Ok(Config::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse() {
        assert_eq!(toml::from_str::<Config>("").unwrap().ignore, [".DS_Store"]);
        assert_eq!(toml::from_str::<Config>("ignore = [\".git\"]").unwrap().ignore, [".git"]);
        assert!(toml::from_str::<Config>("ignored = []").is_err());
    }
}

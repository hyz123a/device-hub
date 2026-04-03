use std::{fs, path::Path};

use device_hub_shared::BoardProfile;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BoardProfilesError {
    #[error("failed to read board profiles: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse board profiles: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

#[derive(Debug, Default, Deserialize)]
struct BoardProfilesFile {
    #[serde(default)]
    profiles: Vec<BoardProfile>,
}

pub fn load(path: impl AsRef<Path>) -> Result<Vec<BoardProfile>, BoardProfilesError> {
    let content = fs::read_to_string(path)?;
    let mut file: BoardProfilesFile = serde_yaml::from_str(&content)?;
    file.profiles.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(file.profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_profiles_from_yaml_file() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("board-profiles.yaml");
        std::fs::write(
            &path,
            "profiles:\n  - id: brd4186c\n    label: BRD4186C\n    jlink:\n      device: EFR32MG24BXXXF1536\n      interface: SWD\n      speed_khz: 2700\n",
        )
        .expect("write board profiles");

        let profiles = load(&path).expect("load board profiles");

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "brd4186c");
        assert_eq!(profiles[0].jlink.device, "EFR32MG24BXXXF1536");
    }
}

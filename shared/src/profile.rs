use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoardProfile {
    pub id: String,
    pub label: String,
    pub jlink: JlinkBoardProfile,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JlinkBoardProfile {
    pub device: String,
    pub interface: String,
    pub speed_khz: u32,
}

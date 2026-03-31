use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionRequest {
    pub payload: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActionCapability {
    SshExec,
    SshTail,
    SshReboot,
    SerialTail,
    SerialWrite,
    JlinkProbe,
    JlinkFlash,
    JlinkVerify,
    JlinkReset,
    JlinkErase,
}

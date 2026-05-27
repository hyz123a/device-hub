use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionRequest {
    pub payload: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SerialExecRequest {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SerialExecResult {
    pub command: String,
    pub output: String,
    pub matched: Option<String>,
    pub timed_out: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionStartResponse {
    #[serde(flatten)]
    pub job: crate::job::Job,
    pub already_running: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActionCapability {
    SshExec,
    SshTail,
    SshReboot,
    SerialTail,
    SerialWrite,
    SerialExec,
    JlinkProbe,
    JlinkFlash,
    JlinkVerify,
    JlinkReset,
    JlinkErase,
}

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdapterCapability {
    pub adapter_id: String,
    pub compiled: bool,
    pub enabled: bool,
    pub available: bool,
    pub actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilitySnapshot {
    pub adapters: Vec<AdapterCapability>,
}

impl CapabilitySnapshot {
    pub fn adapter(&self, adapter_id: &str) -> Option<&AdapterCapability> {
        self.adapters
            .iter()
            .find(|capability| capability.adapter_id == adapter_id)
    }

    pub fn action(&self, action: &str) -> Option<&AdapterCapability> {
        self.adapters
            .iter()
            .find(|capability| capability.actions.iter().any(|item| item == action))
    }

    pub fn is_action_available(&self, action: &str) -> bool {
        self.action(action)
            .is_some_and(|capability| capability.available)
    }
}

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

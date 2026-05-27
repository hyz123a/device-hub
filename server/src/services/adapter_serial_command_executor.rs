use device_hub_adapter_serial::SerialAdapter;
use device_hub_shared::{Device, SerialExecRequest, SerialExecResult};
use futures::future::BoxFuture;

use crate::ports::{SerialCommandError, SerialCommandExecutorPort};

#[derive(Clone)]
pub struct AdapterSerialCommandExecutor {
    serial_adapter: SerialAdapter,
}

impl AdapterSerialCommandExecutor {
    pub fn new(serial_adapter: SerialAdapter) -> Self {
        Self { serial_adapter }
    }
}

impl SerialCommandExecutorPort for AdapterSerialCommandExecutor {
    fn exec(
        &self,
        device: Device,
        request: SerialExecRequest,
    ) -> BoxFuture<'static, Result<SerialExecResult, SerialCommandError>> {
        let serial_adapter = self.serial_adapter.clone();
        Box::pin(async move {
            let Some(config) = device.serial.as_ref() else {
                return Err(SerialCommandError::Failed(
                    "device is missing serial configuration".into(),
                ));
            };

            serial_adapter
                .exec(config, &request)
                .await
                .map_err(|error| SerialCommandError::Failed(error.to_string()))
        })
    }
}

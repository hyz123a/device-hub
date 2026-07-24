use std::sync::Arc;

use futures::future::BoxFuture;

use crate::ports::{HostDiscoveryError, HostDiscoveryPort};

type SerialDiscovery = Arc<
    dyn Fn() -> BoxFuture<
            'static,
            Result<Vec<device_hub_shared::DiscoveredSerialPort>, HostDiscoveryError>,
        > + Send
        + Sync,
>;
type JlinkDiscovery = Arc<
    dyn Fn() -> BoxFuture<
            'static,
            Result<Vec<device_hub_shared::DiscoveredJlinkProbe>, HostDiscoveryError>,
        > + Send
        + Sync,
>;

#[derive(Clone, Default)]
pub struct AdapterHostDiscovery {
    serial: Option<SerialDiscovery>,
    jlink: Option<JlinkDiscovery>,
}

impl AdapterHostDiscovery {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(feature = "serial")]
    pub fn with_serial(mut self, adapter: device_hub_adapter_serial::SerialAdapter) -> Self {
        self.serial = Some(Arc::new(move || {
            let adapter = adapter.clone();
            Box::pin(async move {
                adapter
                    .discover_ports()
                    .map_err(|error| HostDiscoveryError::Serial(error.to_string()))
            })
        }));
        self
    }

    #[cfg(feature = "jlink")]
    pub fn with_jlink(mut self, adapter: device_hub_adapter_jlink::JlinkAdapter) -> Self {
        self.jlink = Some(Arc::new(move || {
            let adapter = adapter.clone();
            Box::pin(async move {
                adapter
                    .scan_probes()
                    .await
                    .map_err(|error| HostDiscoveryError::Jlink(error.to_string()))
            })
        }));
        self
    }
}

impl HostDiscoveryPort for AdapterHostDiscovery {
    fn list_serial_ports(
        &self,
    ) -> BoxFuture<'static, Result<Vec<device_hub_shared::DiscoveredSerialPort>, HostDiscoveryError>>
    {
        match self.serial.clone() {
            Some(discover) => discover(),
            None => Box::pin(async {
                Err(HostDiscoveryError::Serial(
                    "serial capability is unavailable".into(),
                ))
            }),
        }
    }

    fn list_jlink_probes(
        &self,
    ) -> BoxFuture<'static, Result<Vec<device_hub_shared::DiscoveredJlinkProbe>, HostDiscoveryError>>
    {
        match self.jlink.clone() {
            Some(discover) => discover(),
            None => Box::pin(async {
                Err(HostDiscoveryError::Jlink(
                    "jlink capability is unavailable".into(),
                ))
            }),
        }
    }
}

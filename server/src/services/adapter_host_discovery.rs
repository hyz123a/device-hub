use device_hub_adapter_jlink::JlinkAdapter;
use device_hub_adapter_serial::SerialAdapter;
use futures::future::BoxFuture;

use crate::ports::{HostDiscoveryError, HostDiscoveryPort};

#[derive(Clone)]
pub struct AdapterHostDiscovery {
    serial_adapter: SerialAdapter,
    jlink_adapter: JlinkAdapter,
}

impl AdapterHostDiscovery {
    pub fn new(serial_adapter: SerialAdapter, jlink_adapter: JlinkAdapter) -> Self {
        Self {
            serial_adapter,
            jlink_adapter,
        }
    }
}

impl HostDiscoveryPort for AdapterHostDiscovery {
    fn list_serial_ports(
        &self,
    ) -> BoxFuture<'static, Result<Vec<device_hub_shared::DiscoveredSerialPort>, HostDiscoveryError>>
    {
        let serial_adapter = self.serial_adapter.clone();
        Box::pin(async move {
            serial_adapter
                .discover_ports()
                .map_err(|error| HostDiscoveryError::Serial(error.to_string()))
        })
    }

    fn list_jlink_probes(
        &self,
    ) -> BoxFuture<'static, Result<Vec<device_hub_shared::DiscoveredJlinkProbe>, HostDiscoveryError>>
    {
        let jlink_adapter = self.jlink_adapter.clone();
        Box::pin(async move {
            jlink_adapter
                .scan_probes()
                .await
                .map_err(|error| HostDiscoveryError::Jlink(error.to_string()))
        })
    }
}

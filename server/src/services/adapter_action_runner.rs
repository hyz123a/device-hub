use chrono::Utc;
use device_hub_adapter_jlink::{
    FlashRequest, JlinkAdapter, JlinkAdapterError, JlinkArtifact, JlinkExecution,
};
use device_hub_adapter_serial::SerialAdapter;
use device_hub_adapter_ssh::{SshAdapter, SshCommand, SshStreamChannel};
use device_hub_shared::{ActionRequest, Device, Job, LogChannel, LogEvent, LogLevel, LogSource};
use futures::future::BoxFuture;
use serde::Serialize;

use crate::{
    ports::{ActionExecutionContext, ActionRunner},
    services::job_runtime::{
        adapter_event, adapter_event_with_source, fail_job, fail_job_with_source, succeed_job,
    },
};

#[derive(Clone)]
pub struct AdapterActionRunner {
    ssh_adapter: SshAdapter,
    jlink_adapter: JlinkAdapter,
    serial_adapter: SerialAdapter,
}

impl AdapterActionRunner {
    pub fn new(
        ssh_adapter: SshAdapter,
        jlink_adapter: JlinkAdapter,
        serial_adapter: SerialAdapter,
    ) -> Self {
        Self {
            ssh_adapter,
            jlink_adapter,
            serial_adapter,
        }
    }
}

impl ActionRunner for AdapterActionRunner {
    fn run(
        &self,
        context: ActionExecutionContext,
        job: Job,
        device: Device,
        action: String,
        payload: Option<ActionRequest>,
    ) -> BoxFuture<'static, ()> {
        let ssh_adapter = self.ssh_adapter.clone();
        let jlink_adapter = self.jlink_adapter.clone();
        let serial_adapter = self.serial_adapter.clone();

        Box::pin(async move {
            match action.as_str() {
                "ssh.exec" => {
                    execute_ssh_exec(&context, &ssh_adapter, &job, &device, payload.as_ref()).await
                }
                "ssh.reboot" => execute_ssh_reboot(&context, &ssh_adapter, &job, &device).await,
                "ssh.tail" => {
                    execute_ssh_tail(&context, &ssh_adapter, &job, &device, payload.as_ref()).await
                }
                "jlink.probe" => execute_jlink_probe(&context, &jlink_adapter, &job, &device).await,
                "jlink.flash" => {
                    execute_jlink_flash(&context, &jlink_adapter, &job, &device, payload.as_ref())
                        .await
                }
                "jlink.verify" => {
                    execute_jlink_verify(&context, &jlink_adapter, &job, &device).await
                }
                "jlink.reset" => execute_jlink_reset(&context, &jlink_adapter, &job, &device).await,
                "jlink.erase" => execute_jlink_erase(&context, &jlink_adapter, &job, &device).await,
                "serial.tail" => {
                    execute_serial_tail(&context, &serial_adapter, &job, &device).await
                }
                "serial.write" => {
                    execute_serial_write(&context, &serial_adapter, &job, &device, payload.as_ref())
                        .await
                }
                _ => fail_job(
                    &context.jobs,
                    &context.events,
                    &context.artifacts,
                    &job,
                    format!("unsupported action `{action}` reached executor"),
                ),
            }
        })
    }
}

async fn execute_ssh_exec(
    context: &ActionExecutionContext,
    ssh_adapter: &SshAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.ssh.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing ssh configuration",
        );
        return;
    };
    let command = payload
        .and_then(|request| request.payload.as_ref())
        .map(|value| value.trim())
        .unwrap_or_default();

    context.events.publish(adapter_event(
        job,
        LogChannel::System,
        LogLevel::Info,
        format!("exec: {command}"),
    ));

    match ssh_adapter
        .exec(
            config,
            &SshCommand {
                command: command.to_owned(),
            },
        )
        .await
    {
        Ok(result) => {
            publish_stream(&context.events, job, LogChannel::Stdout, &result.stdout);
            publish_stream(&context.events, job, LogChannel::Stderr, &result.stderr);

            if result.exit_code == 0 {
                succeed_job(
                    &context.jobs,
                    &context.events,
                    &context.artifacts,
                    job,
                    "ssh.exec completed",
                );
            } else {
                fail_job(
                    &context.jobs,
                    &context.events,
                    &context.artifacts,
                    job,
                    format!("ssh.exec exited with status {}", result.exit_code),
                );
            }
        }
        Err(error) => fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            error.to_string(),
        ),
    }
}

async fn execute_ssh_reboot(
    context: &ActionExecutionContext,
    ssh_adapter: &SshAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.ssh.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing ssh configuration",
        );
        return;
    };

    context.events.publish(adapter_event(
        job,
        LogChannel::System,
        LogLevel::Info,
        "reboot requested",
    ));

    match ssh_adapter.reboot(config).await {
        Ok(()) => succeed_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "ssh.reboot completed",
        ),
        Err(error) => fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            error.to_string(),
        ),
    }
}

async fn execute_ssh_tail(
    context: &ActionExecutionContext,
    ssh_adapter: &SshAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.ssh.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing ssh configuration",
        );
        return;
    };
    let path = payload
        .and_then(|request| request.payload.as_ref())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(config.default_log_path.trim());

    context.events.publish(adapter_event(
        job,
        LogChannel::System,
        LogLevel::Info,
        format!("tailing logs from {path}"),
    ));

    match ssh_adapter.tail_logs_stream(config, path).await {
        Ok(mut session) => loop {
            match session.next_chunk().await {
                Ok(Some(chunk)) => context.events.publish(adapter_event(
                    job,
                    match chunk.channel {
                        SshStreamChannel::Stdout => LogChannel::Stdout,
                        SshStreamChannel::Stderr => LogChannel::Stderr,
                    },
                    LogLevel::Info,
                    chunk.line,
                )),
                Ok(None) => {
                    succeed_job(
                        &context.jobs,
                        &context.events,
                        &context.artifacts,
                        job,
                        "ssh.tail completed",
                    );
                    break;
                }
                Err(error) => {
                    fail_job(
                        &context.jobs,
                        &context.events,
                        &context.artifacts,
                        job,
                        error.to_string(),
                    );
                    break;
                }
            }
        },
        Err(error) => fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            error.to_string(),
        ),
    }
}

async fn execute_serial_tail(
    context: &ActionExecutionContext,
    serial_adapter: &SerialAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.serial.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing serial configuration",
        );
        return;
    };

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Serial,
        LogChannel::System,
        LogLevel::Info,
        format!(
            "tailing serial logs from {} @ {} baud",
            config.port, config.baud
        ),
    ));

    match serial_adapter.tail_logs_stream(config).await {
        Ok(mut session) => loop {
            match session.next_chunk().await {
                Ok(Some(chunk)) => context.events.publish(LogEvent {
                    ts: Utc::now(),
                    device_id: job.device_id.clone(),
                    job_id: job.id.clone(),
                    source: LogSource::Serial,
                    channel: LogChannel::Stdout,
                    level: LogLevel::Info,
                    message: chunk.line,
                }),
                Ok(None) => {
                    succeed_job(
                        &context.jobs,
                        &context.events,
                        &context.artifacts,
                        job,
                        "serial.tail completed",
                    );
                    break;
                }
                Err(error) => {
                    fail_job_with_source(
                        &context.jobs,
                        &context.events,
                        &context.artifacts,
                        job,
                        LogSource::Serial,
                        error.to_string(),
                    );
                    break;
                }
            }
        },
        Err(error) => fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Serial,
            error.to_string(),
        ),
    }
}

async fn execute_serial_write(
    context: &ActionExecutionContext,
    serial_adapter: &SerialAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.serial.as_ref() else {
        fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Serial,
            "device is missing serial configuration",
        );
        return;
    };
    let input = payload
        .and_then(|request| request.payload.as_deref())
        .unwrap_or_default();

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Serial,
        LogChannel::System,
        LogLevel::Info,
        format!("writing serial input {}", input.escape_debug()),
    ));

    match serial_adapter.write(config, input.as_bytes()).await {
        Ok(bytes_written) => {
            context.events.publish(adapter_event_with_source(
                job,
                LogSource::Serial,
                LogChannel::System,
                LogLevel::Info,
                format!(
                    "wrote {bytes_written} bytes to {} @ {} baud",
                    config.port, config.baud
                ),
            ));
            succeed_job(
                &context.jobs,
                &context.events,
                &context.artifacts,
                job,
                "serial.write completed",
            );
        }
        Err(error) => fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Serial,
            error.to_string(),
        ),
    }
}

async fn execute_jlink_probe(
    context: &ActionExecutionContext,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!("probing J-Link {} for {}", config.serial, config.device),
    ));

    match jlink_adapter.probe(config).await {
        Ok(execution) => {
            persist_jlink_execution(&context.events, &context.artifacts, job, &execution);
            succeed_job(
                &context.jobs,
                &context.events,
                &context.artifacts,
                job,
                "jlink.probe completed",
            );
        }
        Err(error) => fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Jlink,
            persist_jlink_failure(&context.events, &context.artifacts, job, error),
        ),
    }
}

async fn execute_jlink_flash(
    context: &ActionExecutionContext,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };
    let firmware_path = payload
        .and_then(|request| request.payload.as_deref())
        .map(str::trim)
        .unwrap_or_default();

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!("flashing {} via J-Link {}", firmware_path, config.serial),
    ));

    match jlink_adapter
        .flash(
            config,
            &FlashRequest {
                firmware_path: firmware_path.to_owned(),
            },
        )
        .await
    {
        Ok(execution) => {
            persist_jlink_execution(&context.events, &context.artifacts, job, &execution);
            succeed_job(
                &context.jobs,
                &context.events,
                &context.artifacts,
                job,
                "jlink.flash completed",
            );
        }
        Err(error) => fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Jlink,
            persist_jlink_failure(&context.events, &context.artifacts, job, error),
        ),
    }
}

async fn execute_jlink_verify(
    context: &ActionExecutionContext,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!(
            "verifying target {} via J-Link {}",
            config.device, config.serial
        ),
    ));

    match jlink_adapter.verify(config).await {
        Ok(execution) => {
            persist_jlink_execution(&context.events, &context.artifacts, job, &execution);
            succeed_job(
                &context.jobs,
                &context.events,
                &context.artifacts,
                job,
                "jlink.verify completed",
            );
        }
        Err(error) => fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Jlink,
            persist_jlink_failure(&context.events, &context.artifacts, job, error),
        ),
    }
}

async fn execute_jlink_reset(
    context: &ActionExecutionContext,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!(
            "resetting target {} via J-Link {}",
            config.device, config.serial
        ),
    ));

    match jlink_adapter.reset(config).await {
        Ok(execution) => {
            persist_jlink_execution(&context.events, &context.artifacts, job, &execution);
            succeed_job(
                &context.jobs,
                &context.events,
                &context.artifacts,
                job,
                "jlink.reset completed",
            );
        }
        Err(error) => fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Jlink,
            persist_jlink_failure(&context.events, &context.artifacts, job, error),
        ),
    }
}

async fn execute_jlink_erase(
    context: &ActionExecutionContext,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!(
            "erasing target {} via J-Link {}",
            config.device, config.serial
        ),
    ));

    match jlink_adapter.erase(config).await {
        Ok(execution) => {
            persist_jlink_execution(&context.events, &context.artifacts, job, &execution);
            succeed_job(
                &context.jobs,
                &context.events,
                &context.artifacts,
                job,
                "jlink.erase completed",
            );
        }
        Err(error) => fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            error.to_string(),
        ),
    }
}

fn publish_stream(events: &crate::events::EventBus, job: &Job, channel: LogChannel, output: &str) {
    for line in output.lines().filter(|line| !line.is_empty()) {
        events.publish(adapter_event(job, channel.clone(), LogLevel::Info, line));
    }
}

fn persist_jlink_execution<T: Serialize>(
    events: &crate::events::EventBus,
    artifacts: &crate::storage::artifact_store::ArtifactStore,
    job: &Job,
    execution: &JlinkExecution<T>,
) {
    for artifact in &execution.artifacts {
        publish_jlink_artifact_logs(events, job, artifact);
        if let Err(error) =
            artifacts.write_named_artifact(&job.id, &artifact.name, &artifact.contents)
        {
            eprintln!(
                "failed to persist job {} artifact {}: {error}",
                job.id, artifact.name
            );
        }
    }

    match serde_json::to_vec_pretty(&execution.result) {
        Ok(result_json) => {
            if let Err(error) = artifacts.write_named_artifact(&job.id, "result.json", &result_json)
            {
                eprintln!("failed to persist job {} result.json: {error}", job.id);
            }
        }
        Err(error) => eprintln!("failed to serialize job {} result payload: {error}", job.id),
    }
}

fn publish_jlink_artifact_logs(
    events: &crate::events::EventBus,
    job: &Job,
    artifact: &JlinkArtifact,
) {
    let (channel, level) = match artifact.name.as_str() {
        "jlink.stdout.log" => (LogChannel::Stdout, LogLevel::Info),
        "jlink.stderr.log" => (LogChannel::Stderr, LogLevel::Warn),
        _ => return,
    };
    let text = String::from_utf8_lossy(&artifact.contents);
    for line in text.lines().filter(|line| !line.is_empty()) {
        events.publish(adapter_event_with_source(
            job,
            LogSource::Jlink,
            channel.clone(),
            level.clone(),
            line,
        ));
    }
}

fn persist_jlink_failure(
    events: &crate::events::EventBus,
    artifacts: &crate::storage::artifact_store::ArtifactStore,
    job: &Job,
    error: JlinkAdapterError,
) -> String {
    if let Some(jlink_artifacts) = error.artifacts() {
        for artifact in jlink_artifacts {
            publish_jlink_artifact_logs(events, job, artifact);
            if let Err(write_error) =
                artifacts.write_named_artifact(&job.id, &artifact.name, &artifact.contents)
            {
                eprintln!(
                    "failed to persist job {} artifact {}: {write_error}",
                    job.id, artifact.name
                );
            }
        }
    }

    error.to_string()
}

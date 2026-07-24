#[cfg(windows)]
use std::sync::mpsc;
#[cfg(windows)]
use std::thread::JoinHandle;

#[cfg(windows)]
use windows_sys::Win32::System::Power::{
    ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
};

pub struct SystemAwakeGuard {
    #[cfg(windows)]
    stop: Option<mpsc::Sender<()>>,
    #[cfg(windows)]
    thread: Option<JoinHandle<()>>,
}

impl SystemAwakeGuard {
    pub const fn supported() -> bool {
        cfg!(windows)
    }

    #[cfg(not(windows))]
    pub fn acquire() -> Result<Self, std::io::Error> {
        Ok(Self {})
    }

    #[cfg(windows)]
    pub fn acquire() -> Result<Self, std::io::Error> {
        let (stop, wait_for_stop) = mpsc::channel();
        let (ready, wait_for_ready) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("device-hub-system-awake".into())
            .spawn(move || {
                let result = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
                if result == 0 {
                    let _ = ready.send(Err(std::io::Error::last_os_error()));
                    return;
                }
                let _ = ready.send(Ok(()));
                let _ = wait_for_stop.recv();
                unsafe {
                    SetThreadExecutionState(ES_CONTINUOUS);
                }
            })?;

        match wait_for_ready.recv() {
            Ok(Ok(())) => Ok(Self {
                stop: Some(stop),
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(std::io::Error::other(
                    "system-awake worker stopped before initialization",
                ))
            }
        }
    }
}

impl Drop for SystemAwakeGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            if let Some(stop) = self.stop.take() {
                let _ = stop.send(());
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SystemAwakeGuard;

    #[test]
    fn support_matches_the_target_platform() {
        assert_eq!(SystemAwakeGuard::supported(), cfg!(windows));
    }
}

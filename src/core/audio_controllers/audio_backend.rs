use cpal::platform::{Device, Host};

use crate::core::audio_controllers::cpal::CpalBackend;
#[cfg(feature = "pulse")]
use crate::core::audio_controllers::pulseaudio::PulseBackend;

use crate::core::thread_messages::{AppListItem, DeviceListItem};

pub fn get_any_backend() -> Box<dyn AudioBackend> {
    #[cfg(not(feature = "pulse"))]
    return Box::new(CpalBackend {});

    #[cfg(feature = "pulse")]
    if let Some(backend) = PulseBackend::try_init() {
        return Box::new(backend);
    } else {
        return Box::new(CpalBackend {});
    }
}

pub trait AudioBackend {
    fn list_devices(&mut self, host: &Host) -> Vec<DeviceListItem>;

    fn set_device(&mut self, host: &Host, inner_name: &str) -> Device;

    /// Return the list of audio-playing applications visible to the audio backend.
    /// Returns an empty list on backends that do not support this (e.g. plain CPAL).
    fn list_apps(&mut self) -> Vec<AppListItem>;

    /// Set up per-application audio capture for the sink input identified by
    /// `app_index`.  On success returns the name of the PulseAudio source to
    /// record from.  Returns `None` on failure or on unsupported backends.
    fn start_app_capture(&mut self, app_index: u32) -> Option<String>;

    /// Tear down any per-application audio capture that was previously started
    /// with `start_app_capture`.  No-op if no capture is in progress.
    fn stop_app_capture(&mut self);
}

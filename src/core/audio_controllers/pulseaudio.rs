use cpal::platform::{Device, Host};
use cpal::traits::HostTrait;

use pulsectl::controllers::{AppControl, DeviceControl, SinkController, SourceController};

use crate::core::audio_controllers::audio_backend::AudioBackend;
use crate::core::thread_messages::{AppListItem, DeviceListItem};

use log::{debug, error};

pub struct PulseBackend {
    handler: SourceController,
    /// Module index of the null-sink loaded for per-app capture (if any).
    app_capture_null_sink_module: Option<u32>,
    /// Module index of the loopback loaded to route app audio to speakers (if any).
    app_capture_loopback_module: Option<u32>,
    /// Sink-input index of the application being captured (if any).
    app_capture_sink_input_index: Option<u32>,
}

impl PulseBackend {
    pub fn try_init() -> Option<Self> {
        match SourceController::create() {
            Ok(mut handler) => {
                if let Err(error) = handler.get_server_info() {
                    error!("Could not get PulseAudio server info: {:?}", error);
                } else if let Err(error) = handler.list_devices() {
                    error!("Could not list PulseAudio devices: {:?}", error);
                } else {
                    return Some(Self {
                        handler,
                        app_capture_null_sink_module: None,
                        app_capture_loopback_module: None,
                        app_capture_sink_input_index: None,
                    });
                }
            }
            Err(error) => {
                error!("Could not initialize PulseAudio backend: {:?}", error);
            }
        }
        None
    }

    fn get_app_idx(&mut self) -> Option<u32> {
        // Get SongRec's source-output index

        let applications = self.handler.list_applications().unwrap();

        let criteria: Vec<String> = vec![
            format!("process.id = \"{}\"", std::process::id()),
            "alsa plug-in [songrec]".to_string(),
            "songrec".to_string(),
            format!("{}", std::process::id()),
        ];

        for criterion in criteria {
            for app in applications.clone() {
                if app
                    .proplist
                    .to_string()
                    .unwrap()
                    .to_lowercase()
                    .contains(&criterion)
                {
                    return Some(app.index);
                }
            }
        }
        None
    }

    /// Run a `pactl` command, returning the trimmed stdout on success.
    fn run_pactl(args: &[&str]) -> Option<String> {
        match std::process::Command::new("pactl").args(args).output() {
            Ok(output) if output.status.success() => {
                Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
            }
            Ok(output) => {
                error!(
                    "pactl {:?} failed: {}",
                    args,
                    String::from_utf8_lossy(&output.stderr)
                );
                None
            }
            Err(err) => {
                error!("Could not run pactl: {:?}", err);
                None
            }
        }
    }
}

impl AudioBackend for PulseBackend {
    fn list_devices(&mut self, _host: &Host) -> Vec<DeviceListItem> {
        let mut device_names: Vec<DeviceListItem> = vec![];
        let mut monitor_device_names: Vec<DeviceListItem> = vec![];

        match self.handler.get_server_info() {
            Ok(info) => match self.handler.list_devices() {
                Ok(devices) => {
                    for dev in devices {
                        if let Some(desc) = &dev.description {
                            if let Some(name) = &dev.name {
                                if &dev.name == &info.default_source_name {
                                    device_names.insert(
                                        0,
                                        DeviceListItem {
                                            inner_name: name.to_string(),
                                            display_name: desc.to_string(),
                                            is_monitor: dev.monitor != None,
                                        },
                                    );
                                } else if dev.monitor != None {
                                    monitor_device_names.push(DeviceListItem {
                                        inner_name: name.to_string(),
                                        display_name: desc.to_string(),
                                        is_monitor: true,
                                    });
                                } else {
                                    device_names.push(DeviceListItem {
                                        inner_name: name.to_string(),
                                        display_name: desc.to_string(),
                                        is_monitor: false,
                                    });
                                }
                            }
                        }
                    }
                }
                Err(error) => {
                    error!("Could not list PulseAudio devices: {:?}", error);
                }
            },
            Err(error) => {
                error!("Could not get PulseAudio server info: {:?}", error);
            }
        }

        device_names.extend(monitor_device_names);
        device_names
    }

    fn set_device(&mut self, host: &Host, inner_name: &str) -> Device {
        match self.handler.list_devices() {
            Ok(devices) => {
                if let Some(app_idx) = self.get_app_idx() {
                    for dev in devices {
                        debug!(
                            "Comparing libpulse device names: {:?} / {:?}",
                            dev.name, inner_name
                        );
                        if Some(inner_name) == dev.name.as_deref() {
                            debug!("Selected libpulse device found: {:?}", dev);

                            self.handler.move_app_by_name(app_idx, inner_name).unwrap();
                            break;
                        }
                    }
                }
            }
            Err(error) => {
                error!("Could not list PulseAudio devices: {:?}", error);
            }
        }

        host.default_input_device().unwrap()
    }

    fn list_apps(&mut self) -> Vec<AppListItem> {
        let mut sink_handler = match SinkController::create() {
            Ok(h) => h,
            Err(err) => {
                error!("Could not create PulseAudio SinkController: {:?}", err);
                return vec![];
            }
        };

        let applications = match sink_handler.list_applications() {
            Ok(apps) => apps,
            Err(err) => {
                error!("Could not list PulseAudio sink inputs: {:?}", err);
                return vec![];
            }
        };

        let own_pid = std::process::id().to_string();

        applications
            .into_iter()
            .filter_map(|app| {
                // Skip SongRec itself
                let proplist_str = app
                    .proplist
                    .to_string()
                    .unwrap_or_default()
                    .to_lowercase();
                if proplist_str.contains("songrec")
                    || proplist_str.contains(&format!("process.id = \"{}\"", own_pid))
                {
                    return None;
                }

                let display_name = app
                    .name
                    .as_deref()
                    .unwrap_or("Unknown Application")
                    .to_string();

                Some(AppListItem {
                    index: app.index,
                    display_name,
                })
            })
            .collect()
    }

    fn start_app_capture(&mut self, app_index: u32) -> Option<String> {
        // Clean up any previous capture session first.
        self.stop_app_capture();

        let null_sink_name = format!("songrec_cap_{}", std::process::id());

        // 1. Create a null sink so we have an exclusive capture point.
        let module_id_str = Self::run_pactl(&[
            "load-module",
            "module-null-sink",
            &format!("sink_name={}", null_sink_name),
            "sink_properties=device.description=\"SongRec\\ Capture\"",
        ])?;

        let null_sink_module_id: u32 = match module_id_str.parse() {
            Ok(id) => id,
            Err(err) => {
                error!("Could not parse null sink module id {:?}: {:?}", module_id_str, err);
                return None;
            }
        };

        // 2. Find the default sink so we can mirror audio back to the user's speakers.
        let default_sink_name = match SinkController::create() {
            Ok(mut sink_ctl) => {
                if let Ok(info) = sink_ctl.get_server_info() {
                    info.default_sink_name.unwrap_or_default()
                } else {
                    String::new()
                }
            }
            Err(_) => String::new(),
        };

        // 3. Load a loopback so the user still hears the application's audio.
        let loopback_module_id: Option<u32> = if !default_sink_name.is_empty() {
            Self::run_pactl(&[
                "load-module",
                "module-loopback",
                &format!("source={}.monitor", null_sink_name),
                &format!("sink={}", default_sink_name),
                "latency_msec=1",
            ])
            .and_then(|s| s.parse::<u32>().ok())
        } else {
            None
        };

        // 4. Move the target application's sink input to our null sink.
        let mut sink_ctl = match SinkController::create() {
            Ok(h) => h,
            Err(err) => {
                error!("Could not create SinkController for move: {:?}", err);
                // Roll back the null-sink module.
                Self::run_pactl(&["unload-module", &null_sink_module_id.to_string()]);
                return None;
            }
        };

        if let Err(err) = sink_ctl.move_app_by_name(app_index, &null_sink_name) {
            error!("Could not move sink input {} to {}: {:?}", app_index, null_sink_name, err);
            // Roll back.
            if let Some(lb_id) = loopback_module_id {
                Self::run_pactl(&["unload-module", &lb_id.to_string()]);
            }
            Self::run_pactl(&["unload-module", &null_sink_module_id.to_string()]);
            return None;
        }

        self.app_capture_null_sink_module = Some(null_sink_module_id);
        self.app_capture_loopback_module = loopback_module_id;
        self.app_capture_sink_input_index = Some(app_index);

        debug!(
            "App capture started: null-sink module={}, loopback module={:?}, sink-input={}",
            null_sink_module_id, loopback_module_id, app_index
        );

        Some(format!("{}.monitor", null_sink_name))
    }

    fn stop_app_capture(&mut self) {
        if self.app_capture_null_sink_module.is_none() {
            return;
        }

        // Unload the loopback first so audio routing is restored before we
        // remove the null sink.
        if let Some(module_id) = self.app_capture_loopback_module.take() {
            Self::run_pactl(&["unload-module", &module_id.to_string()]);
        }

        // Unload the null sink; PulseAudio will automatically migrate any
        // sink inputs that were connected to it back to the default sink.
        if let Some(module_id) = self.app_capture_null_sink_module.take() {
            Self::run_pactl(&["unload-module", &module_id.to_string()]);
        }

        self.app_capture_sink_input_index = None;

        debug!("App capture stopped");
    }
}

use async_channel::{Receiver, Sender};
use chrono::Local;
use egui::{ColorImage, Context, TextureHandle, ViewportBuilder};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::core::microphone_thread::microphone_thread;
use crate::core::preferences::PreferencesInterface;
use crate::core::processing_thread::processing_thread;
use crate::core::http_task::http_task;
use crate::core::thread_messages::{
    spawn_big_thread, DeviceListItem, GUIMessage, MicrophoneMessage, ProcessingMessage,
};
use crate::gui::song_history_interface::RecognitionHistoryInterface;
use crate::utils::csv_song_history::SongHistoryRecord;
use crate::utils::filesystem_operations::obtain_recognition_history_csv_path;

pub fn gui_main(
    log_object: crate::core::logging::Logging,
    recording: bool,
    input_file: Option<String>,
    _enable_mpris_cli: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("SongRec")
            .with_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "SongRec",
        native_options,
        Box::new(move |cc| Ok(Box::new(SongRecApp::new(cc, log_object, recording, input_file)))),
    )
    .map_err(|e| format!("eframe error: {e}").into())
}

struct SongRecApp {
    gui_rx: Receiver<GUIMessage>,
    microphone_tx: Sender<MicrophoneMessage>,
    processing_tx: Sender<ProcessingMessage>,

    audio_devices: Vec<DeviceListItem>,
    selected_device_idx: usize,
    microphone_active: bool,

    status_message: String,
    volume_percent: f32,
    network_ok: bool,
    rate_limited: bool,

    current_artist: String,
    current_song: String,
    current_album: String,
    cover_texture: Option<TextureHandle>,

    song_history: RecognitionHistoryInterface,

    log_text: String,
    show_about: bool,
    show_preferences: bool,
    preferences_interface: PreferencesInterface,
}

impl SongRecApp {
    fn new(
        _cc: &eframe::CreationContext<'_>,
        log_object: crate::core::logging::Logging,
        recording: bool,
        input_file: Option<String>,
    ) -> Self {
        let (gui_tx, gui_rx) = async_channel::unbounded::<GUIMessage>();
        let (microphone_tx, microphone_rx) = async_channel::unbounded::<MicrophoneMessage>();
        let (processing_tx, processing_rx) = async_channel::unbounded::<ProcessingMessage>();
        let (http_tx, http_rx) = async_channel::unbounded();

        log_object.connect_to_gui_logger(gui_tx.clone());

        let preferences_interface = PreferencesInterface::new();
        let prefs_arc = Arc::new(Mutex::new(preferences_interface.clone()));

        let gui_tx2 = gui_tx.clone();
        let gui_tx3 = gui_tx.clone();
        let microphone_tx2 = microphone_tx.clone();
        let microphone_tx3 = microphone_tx.clone();
        let processing_tx2 = processing_tx.clone();

        spawn_big_thread(move || {
            microphone_thread(microphone_rx, microphone_tx2, processing_tx2, gui_tx2, prefs_arc);
        });

        spawn_big_thread(move || {
            processing_thread(processing_rx, http_tx, gui_tx3);
        });

        let gui_tx4 = gui_tx.clone();
        spawn_big_thread(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime for HTTP task");
            rt.block_on(http_task(http_rx, gui_tx4, microphone_tx3));
        });

        let song_history =
            RecognitionHistoryInterface::new(obtain_recognition_history_csv_path).unwrap_or_else(
                |_| RecognitionHistoryInterface::new(|| Ok(String::new())).unwrap(),
            );

        // If an input file was supplied schedule it immediately
        if let Some(ref file) = input_file {
            processing_tx
                .try_send(ProcessingMessage::ProcessAudioFile(file.clone()))
                .ok();
        }

        SongRecApp {
            gui_rx,
            microphone_tx,
            processing_tx,
            audio_devices: Vec::new(),
            selected_device_idx: 0,
            microphone_active: recording,
            status_message: String::from("Idle"),
            volume_percent: 0.0,
            network_ok: true,
            rate_limited: false,
            current_artist: String::new(),
            current_song: String::new(),
            current_album: String::new(),
            cover_texture: None,
            song_history,
            log_text: String::new(),
            show_about: false,
            show_preferences: false,
            preferences_interface,
        }
    }

    fn start_microphone(&self) {
        if let Some(dev) = self.audio_devices.get(self.selected_device_idx) {
            self.microphone_tx
                .try_send(MicrophoneMessage::MicrophoneRecordStart(
                    dev.inner_name.clone(),
                ))
                .ok();
        }
    }

    fn stop_microphone(&self) {
        self.microphone_tx
            .try_send(MicrophoneMessage::MicrophoneRecordStop)
            .ok();
    }

    fn open_in_browser(artist: &str, song: &str) {
        use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
        let query = format!(
            "{}+{}",
            utf8_percent_encode(artist, NON_ALPHANUMERIC),
            utf8_percent_encode(song, NON_ALPHANUMERIC)
        );
        let url = format!("https://www.youtube.com/results?search_query={query}");
        webbrowser::open(&url).ok();
    }

    fn process_messages(&mut self, ctx: &Context) {
        while let Ok(msg) = self.gui_rx.try_recv() {
            match msg {
                GUIMessage::DevicesList(devices) => {
                    self.audio_devices = *devices;
                    if self.microphone_active && !self.audio_devices.is_empty() {
                        self.start_microphone();
                    }
                }
                GUIMessage::MicrophoneRecording => {
                    self.status_message = "Recording…".to_string();
                }
                GUIMessage::MicrophoneVolumePercent(v) => {
                    self.volume_percent = v;
                }
                GUIMessage::NetworkStatus(ok) => {
                    self.network_ok = ok;
                    if !ok {
                        self.status_message = "Network unreachable".to_string();
                    }
                }
                GUIMessage::RateLimitState(limited) => {
                    self.rate_limited = limited;
                    if limited {
                        self.status_message = "Rate limited — waiting…".to_string();
                    }
                }
                GUIMessage::ErrorMessage(e) => {
                    self.status_message = e;
                }
                GUIMessage::SongRecognized(msg) => {
                    self.current_artist = msg.artist_name.clone();
                    self.current_song = msg.song_name.clone();
                    self.current_album = msg.album_name.clone().unwrap_or_default();
                    self.status_message = format!("{} — {}", msg.artist_name, msg.song_name);

                    if let Some(ref bytes) = msg.cover_image {
                        if let Ok(img) = image::load_from_memory(bytes) {
                            let img = img.to_rgba8();
                            let (w, h) = img.dimensions();
                            let pixels = img.into_raw();
                            let color_image = ColorImage::from_rgba_unmultiplied(
                                [w as usize, h as usize],
                                &pixels,
                            );
                            self.cover_texture =
                                Some(ctx.load_texture("cover", color_image, Default::default()));
                        }
                    }

                    let record = SongHistoryRecord {
                        song_name: format!("{} — {}", msg.artist_name, msg.song_name),
                        album: msg.album_name.clone(),
                        track_key: Some(msg.track_key.clone()),
                        release_year: msg.release_year.clone(),
                        genre: msg.genre.clone(),
                        recognition_date: Local::now().format("%c").to_string(),
                    };

                    // Always suppress consecutive identical recognitions: only add the
                    // song to history when it differs from the most-recent entry.
                    let already_present = self
                        .song_history
                        .records
                        .first()
                        .map(|r| r.track_key == Some(msg.track_key.clone()))
                        .unwrap_or(false);

                    if !already_present {
                        self.song_history.add_row_and_save(record);
                    }
                }
                GUIMessage::AppendToLog(text) => {
                    self.log_text.push_str(&text);
                    if self.log_text.len() > 100_000 {
                        self.log_text =
                            self.log_text[self.log_text.len() - 80_000..].to_string();
                    }
                }
                GUIMessage::ShowWindow | GUIMessage::QuitApplication => {}
            }
        }
    }
}

impl eframe::App for SongRecApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.process_messages(ctx);
        ctx.request_repaint_after(Duration::from_millis(100));

        // ── About dialog ──────────────────────────────────────────────────
        if self.show_about {
            egui::Window::new("About SongRec")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.heading("SongRec");
                    ui.label(
                        "An open-source Shazam client for Linux, written in Rust.",
                    );
                    ui.hyperlink("https://github.com/marin-m/SongRec");
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        self.show_about = false;
                    }
                });
        }

        // ── Preferences dialog ────────────────────────────────────────────
        if self.show_preferences {
            let mut show = true;
            egui::Window::new("Preferences")
                .open(&mut show)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    let prefs = &mut self.preferences_interface.preferences;

                    let mut notifications = prefs.enable_notifications.unwrap_or(true);
                    if ui
                        .checkbox(&mut notifications, "Enable notifications")
                        .changed()
                    {
                        prefs.enable_notifications = Some(notifications);
                    }

                    let mut buf_size = prefs.buffer_size_secs.unwrap_or(12) as f64;
                    ui.add(
                        egui::Slider::new(&mut buf_size, 1.0..=60.0)
                            .text("Buffer size (s)")
                            .integer(),
                    );
                    prefs.buffer_size_secs = Some(buf_size as u64);

                    let mut interval = prefs.request_interval_secs_v3.unwrap_or(8) as f64;
                    ui.add(
                        egui::Slider::new(&mut interval, 1.0..=60.0)
                            .text("Request interval (s)")
                            .integer(),
                    );
                    prefs.request_interval_secs_v3 = Some(interval as u64);

                    ui.add_space(8.0);
                    if ui.button("Save").clicked() {
                        let updated = prefs.clone();
                        self.preferences_interface.update(updated);
                        self.show_preferences = false;
                    }
                });
            if !show {
                self.show_preferences = false;
            }
        }

        // ── Top toolbar ───────────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("🎵 SongRec");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("About").clicked() {
                        self.show_about = true;
                    }
                    if ui.button("⚙ Preferences").clicked() {
                        self.show_preferences = true;
                    }
                    if self.rate_limited {
                        ui.colored_label(egui::Color32::YELLOW, "⚠ Rate limited");
                    } else if !self.network_ok {
                        ui.colored_label(egui::Color32::RED, "✗ No network");
                    } else {
                        ui.colored_label(egui::Color32::GREEN, "✓ Online");
                    }
                });
            });
        });

        // ── Left panel ────────────────────────────────────────────────────
        egui::SidePanel::left("controls")
            .min_width(260.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);

                ui.label("Audio device:");
                egui::ComboBox::from_id_salt("device_combo")
                    .selected_text(
                        self.audio_devices
                            .get(self.selected_device_idx)
                            .map(|d| d.display_name.as_str())
                            .unwrap_or("(no devices)"),
                    )
                    .show_ui(ui, |ui| {
                        for (i, dev) in self.audio_devices.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.selected_device_idx,
                                i,
                                &dev.display_name,
                            );
                        }
                    });

                ui.add_space(6.0);

                let rec_label = if self.microphone_active {
                    "⏹ Stop"
                } else {
                    "▶ Start"
                };
                if ui.button(rec_label).clicked() {
                    if self.microphone_active {
                        self.microphone_active = false;
                        self.stop_microphone();
                    } else {
                        self.microphone_active = true;
                        self.start_microphone();
                    }
                }

                if ui.button("📂 Recognise from file…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter(
                            "Audio",
                            &["wav", "mp3", "flac", "ogg", "m4a", "aac"],
                        )
                        .pick_file()
                    {
                        let path_str = path.to_string_lossy().to_string();
                        self.processing_tx
                            .try_send(ProcessingMessage::ProcessAudioFile(path_str))
                            .ok();
                    }
                }

                ui.add_space(8.0);
                ui.separator();

                ui.label(egui::RichText::new("Status:").small());
                ui.label(&self.status_message);

                ui.add_space(4.0);
                ui.label(egui::RichText::new("Volume:").small());
                ui.add(
                    egui::ProgressBar::new(self.volume_percent / 100.0)
                        .desired_width(220.0)
                        .animate(self.microphone_active),
                );

                ui.add_space(8.0);
                ui.separator();

                if !self.current_song.is_empty() {
                    if let Some(ref tex) = self.cover_texture {
                        let size = egui::vec2(160.0, 160.0);
                        ui.image(egui::load::SizedTexture::new(tex.id(), size));
                    }
                    ui.label(egui::RichText::new(&self.current_song).strong().size(15.0));
                    ui.label(egui::RichText::new(&self.current_artist).size(13.0));
                    if !self.current_album.is_empty() {
                        ui.label(
                            egui::RichText::new(&self.current_album)
                                .italics()
                                .size(12.0),
                        );
                    }
                    let artist = self.current_artist.clone();
                    let song = self.current_song.clone();
                    if ui.button("▶ Open in YouTube").clicked() {
                        Self::open_in_browser(&artist, &song);
                    }
                } else {
                    ui.label("No song recognised yet.");
                }

                ui.add_space(8.0);
                if ui.button("🗑 Clear history").clicked() {
                    self.song_history.wipe_and_save();
                }
            });

        // ── Central panel: song history ───────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Recognition history");
            ui.add_space(4.0);

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("history_grid")
                    .num_columns(3)
                    .striped(true)
                    .min_col_width(100.0)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Date").strong());
                        ui.label(egui::RichText::new("Song").strong());
                        ui.label(egui::RichText::new("Album").strong());
                        ui.end_row();

                        let records_snapshot: Vec<SongHistoryRecord> =
                            self.song_history.records.clone();

                        for record in &records_snapshot {
                            let date_label = ui.label(&record.recognition_date);
                            let song_label = ui.label(&record.song_name);
                            let album_text = record.album.as_deref().unwrap_or("");
                            let album_label = ui.label(album_text);

                            let row_response = date_label | song_label | album_label;
                            row_response.context_menu(|ui| {
                                let parts: Vec<&str> =
                                    record.song_name.splitn(2, " — ").collect();
                                let (artist, song_title) = if parts.len() == 2 {
                                    (parts[0], parts[1])
                                } else {
                                    ("", record.song_name.as_str())
                                };

                                if ui.button("▶ Open in YouTube").clicked() {
                                    Self::open_in_browser(artist, song_title);
                                    ui.close_menu();
                                }
                                if ui.button("🗑 Remove entry").clicked() {
                                    self.song_history.remove(record);
                                    ui.close_menu();
                                }
                            });

                            ui.end_row();
                        }
                    });
            });
        });
    }
}

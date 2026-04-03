#[cfg(feature = "gui")]
use crate::core::thread_messages::GUIMessage;
use std::boxed::Box;
use std::io::Write;
use std::sync::{Arc, Mutex};

#[cfg(feature = "gui")]
#[derive(Clone)]
struct GUIDispatcher {
    #[cfg(feature = "gui")]
    gui_tx: Arc<Mutex<Option<async_channel::Sender<GUIMessage>>>>,
}

#[cfg(feature = "gui")]
impl GUIDispatcher {
    fn new() -> Self {
        Self {
            gui_tx: Arc::new(Mutex::new(None)),
        }
    }

    fn connect_to_gui_logger(&self, gui_tx: async_channel::Sender<GUIMessage>) {
        *self.gui_tx.lock().unwrap() = Some(gui_tx);
    }
}

#[cfg(feature = "gui")]
impl Write for GUIDispatcher {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
        #[cfg(feature = "gui")]
        if let Some(ref gui_tx) = *self.gui_tx.lock().unwrap() {
            gui_tx
                .try_send(GUIMessage::AppendToLog(
                    String::from_utf8_lossy(buf).into_owned(),
                ))
                .unwrap();
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), std::io::Error> {
        Ok(())
    }
}

#[cfg(feature = "gui")]
unsafe impl std::marker::Send for GUIDispatcher {}

pub struct Logging {
    #[cfg(feature = "gui")]
    gui_dispatcher: GUIDispatcher,
}

impl Logging {
    pub fn setup_logging(glib_level: log::LevelFilter, songrec_level: log::LevelFilter) -> Self {
        // TODO: Improve the format?

        let mut main_dispatch = fern::Dispatch::new().format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {} {}:{}] {}",
                humantime::format_rfc3339_seconds(std::time::SystemTime::now()),
                record.level(),
                record.target(),
                // record.module_path().unwrap_or("??"), <- Usually same as target
                record.file().unwrap_or("??"),
                match record.line() {
                    Some(line) => line.to_string(),
                    None => "??".to_string(),
                },
                message
            ))
        });

        let stderr_dispatch = fern::Dispatch::new()
            .level(glib_level)
            .level_for("songrec", songrec_level)
            .chain(std::io::stderr());

        main_dispatch = main_dispatch.chain(stderr_dispatch);

        #[cfg(feature = "gui")]
        {
            let gui_dispatcher = GUIDispatcher::new();
            let gui_dispatcher_copy: Box<dyn Write + Send> = Box::new(gui_dispatcher.clone());

            let gui_dispatch = fern::Dispatch::new()
                .level(log::LevelFilter::Debug)
                .level_for("songrec", log::LevelFilter::Debug)
                .chain(gui_dispatcher_copy);

            main_dispatch = main_dispatch.chain(gui_dispatch);
            main_dispatch.apply().unwrap();

            Self { gui_dispatcher }
        }

        #[cfg(not(feature = "gui"))]
        {
            main_dispatch.apply().unwrap();
            Self {}
        }
    }

    #[cfg(feature = "gui")]
    pub fn connect_to_gui_logger(self, gui_tx: async_channel::Sender<GUIMessage>) {
        self.gui_dispatcher.connect_to_gui_logger(gui_tx);
    }

    pub fn bind_glib_logging() {
        // No-op: glib logging integration removed in favour of reqwest/tokio stack
    }
}

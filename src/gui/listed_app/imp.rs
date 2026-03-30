// GObject backing type for an audio-playing application entry in the
// per-application capture combo row.
//
// See listed_device/imp.rs for the same pattern applied to audio devices.

use std::cell::{Cell, RefCell};

use glib::object::ObjectExt;
use glib::subclass::object::DerivedObjectProperties;
use glib::subclass::prelude::ObjectImpl;
use glib::subclass::prelude::ObjectSubclass;
use glib::Properties;

#[derive(Properties, Default)]
#[properties(wrapper_type = super::ListedApp)]
pub struct ListedApp {
    /// Human-readable application name shown in the combo row.
    #[property(construct_only, get)]
    display_name: RefCell<String>,
    /// PulseAudio sink-input index (u32::MAX == "All applications").
    #[property(construct_only, get)]
    app_index: Cell<u32>,
}

#[glib::object_subclass]
impl ObjectSubclass for ListedApp {
    const NAME: &'static str = "ListedApp";
    type Type = super::ListedApp;
    type ParentType = glib::Object;
}

#[glib::derived_properties]
impl ObjectImpl for ListedApp {}

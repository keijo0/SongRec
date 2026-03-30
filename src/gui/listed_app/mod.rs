mod imp;

glib::wrapper! {
    pub struct ListedApp(ObjectSubclass<imp::ListedApp>);
}

impl ListedApp {
    /// Create a new `ListedApp` item.
    ///
    /// `app_index == u32::MAX` is the sentinel value that means "All applications".
    pub fn new(display_name: String, app_index: u32) -> Self {
        glib::Object::builder()
            .property("display_name", display_name)
            .property("app_index", app_index)
            .build()
    }
}

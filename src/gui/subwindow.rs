use eframe::egui::{Context, Ui};

pub enum SubwindowResult {
    Keep(Box<dyn Subwindow>),
    Spawn((Box<dyn Subwindow>, Vec<Box<dyn Subwindow>>)),
    Replace(Box<dyn Subwindow>),
    Close,
}

pub trait Subwindow {
    /// String to be shown as the header
    fn name(&self) -> String;

    /// This function is called for visible subwindows
    fn ui(self: Box<Self>, ui: &mut Ui) -> SubwindowResult;

    /// Whether to show the close button
    fn is_closeable(&self) -> bool {
        true
    }

    /// This function is analogous to ui(), however with the caveat that it's only called
    /// for background subwindows, i.e. where only the context is available.
    fn not_ui(self: Box<Self>, ctx: &Context) -> SubwindowResult;

    /// Called when the subwindow is closed by the close button if allowed by is_closeable.
    fn on_close(&mut self) {}
}

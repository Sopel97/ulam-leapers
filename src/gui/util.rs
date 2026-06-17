use eframe::egui;
use eframe::egui::{Context, Ui};
use std::ops::RangeInclusive;

pub enum ContextOrUi<'a> {
    Context(&'a Context),
    Ui(&'a mut Ui),
}

impl<'a> ContextOrUi<'a> {
    pub fn ctx(&self) -> &Context {
        match self {
            ContextOrUi::Context(ctx) => ctx,
            ContextOrUi::Ui(ui) => ui.ctx(),
        }
    }

    pub fn ui(&mut self) -> Option<&mut Ui> {
        match self {
            ContextOrUi::Context(_ctx) => None,
            ContextOrUi::Ui(ui) => Some(ui),
        }
    }
}

pub fn scroll_delta_in_ui(ui: &Ui) -> i32 {
    ui.input(|i| {
        let mut zoom_delta = 0;
        for event in &i.events {
            if let egui::Event::MouseWheel { delta, .. } = event {
                zoom_delta += delta.y as i32;
            }
        }
        zoom_delta
    })
}

pub fn format_zoom_slider_text(n: f64, _: RangeInclusive<usize>) -> String {
    let n = n.round() as i32;
    if n >= 0 {
        format!("{}x", 1 << n)
    } else {
        format!("1/{}x", 1 << -n)
    }
}

use std::ops::RangeInclusive;
use eframe::egui;
use eframe::egui::Ui;

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
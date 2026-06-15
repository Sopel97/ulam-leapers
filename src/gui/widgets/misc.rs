use eframe::egui::color_picker::Alpha;
use eframe::egui::{color_picker, vec2, Color32, Response, Ui, Vec2};

pub fn ui_layout_2d<F>(ui: &mut Ui, width: usize, height: usize, mut func: F)
where
    F: FnMut(&mut Ui, usize, usize),
{
    ui.spacing_mut().item_spacing = Vec2::ZERO;
    ui.vertical(|ui| {
        for y in 0..height {
            ui.horizontal(|ui| {
                for x in 0..width {
                    func(ui, x, y);
                }
            });
        }
    });
}

pub fn srgb_color_button(ui: &mut Ui, color: &mut Color32, allow_change: bool) -> Response {
    if allow_change {
        color_picker::color_edit_button_srgba(ui, color, Alpha::Opaque)
    } else {
        color_picker::show_color(ui, *color, vec2(16.0, 16.0))
    }
}

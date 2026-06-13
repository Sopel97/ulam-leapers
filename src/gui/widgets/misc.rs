use eframe::egui::{Checkbox, Sense, Ui, Vec2};
use ulam_leapers::collections::array2d::Array2D;

pub fn ui_layout_2d<F>(ui: &mut Ui, width: usize, height: usize, mut func: F) 
where
    F: FnMut(&mut Ui, usize, usize)
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
use eframe::egui::{Color32, Response, Ui};
use crate::gui::widgets::misc::srgb_color_button;

/// Returns `true` if any color has changed. Returns `false` otherwise.
pub fn show_player_colors_ui(ui: &mut Ui, player_colors: &mut [Color32], allow_change: bool) -> bool {
    let mut any_change = false;

    // TODO: Columns for some reason take more space than necessary.
    //       This `set_max_width` is a hack to make it about as much as it should.
    ui.set_max_width(200.0);
    ui.columns(2, |columns| {
        for (player_id, color) in player_colors.iter_mut().enumerate() {
            let column = &mut columns[player_id % 2];
            column.horizontal_wrapped(|ui| {
                if srgb_color_button(ui, color, allow_change)
                    .changed()
                {
                    any_change = true;
                }

                if player_id == 0 {
                    ui.label("Empty");
                } else {
                    ui.label(format!("Player {}", player_id));
                }
            });
        }
    });
    
    any_change
}
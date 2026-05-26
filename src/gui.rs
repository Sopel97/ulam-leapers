use eframe::egui;

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();

    let mut name = "Arthur".to_owned();
    let mut age = 42;
    let mut t: f32 = 0.0;

    eframe::run_ui_native("Ulam Leapers Explorer", options, move |ui, _frame| {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let rect = ui.clip_rect(); // Full canvas
            let painter = ui.painter_at(rect);

            // background
            painter.rect_filled(rect, 0.0, egui::Color32::BLACK);

            let center = rect.center();

            let radius = rect.width().min(rect.height()) * 0.4;

            let angle = t;
            t += 0.01;

            let end = egui::pos2(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            );

            painter.line_segment(
                [center, end],
                egui::Stroke::new(3.0, egui::Color32::WHITE),
            );

            // Render some controls on top.
            ui.heading("My egui Application");
            ui.horizontal(|ui| {
                let name_label = ui.label("Your name: ");
                ui.text_edit_singleline(&mut name)
                    .labelled_by(name_label.id);
            });
            ui.add(egui::Slider::new(&mut age, 0..=120).text("age"));
            if ui.button("Increment").clicked() {
                age += 1;
            }
            ui.label(format!("Hello '{name}', age {age}"));

            // IMPORTANT: keeps animation running
            ui.ctx().request_repaint();
        });
    })
}
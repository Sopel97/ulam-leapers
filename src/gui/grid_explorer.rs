use crate::gui::grid_render::Zoom::{Magnification, Minification};
use crate::gui::grid_render::{default_player_colors, GridRender, GridRenderParameters};
use crate::gui::SubwindowResult::Keep;
use crate::gui::{Subwindow, SubwindowResult};
use eframe::egui;
use eframe::egui::{Rect, Response, Sense, Ui};
use eframe::emath::pos2;
use eframe::epaint::Color32;
use std::io::BufWriter;
use std::path::PathBuf;
use ulam_leapers::grid::{GridPoint, GridRect};
use ulam_leapers::io::{ReadFrom, WriteTo};
use ulam_leapers::simulation::{FinalizedSimulation, Game};
use ulam_leapers::util::pow2::{floor_div, floor_to_multiple, Pow2};

pub enum SaveState {
    NotSaved,
    Saved,
    Errored(std::io::Error),
}

pub struct GridExplorer {
    grid_view_controls: GridViewControls,
    finalized_sim: FinalizedSimulation,
    grid_render: GridRender,
    save_state: SaveState,
}

impl Subwindow for GridExplorer {
    fn name(&self) -> String {
        if matches!(self.save_state, SaveState::Saved) {
            "Explorer".to_owned()
        } else {
            "*Explorer".to_owned()
        }
    }

    fn ui(mut self: Box<Self>, ui: &mut Ui) -> SubwindowResult {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::Window::new("Controls")
                .scroll(false)
                .resizable([false, false]) // resizable so we can shrink if the text edit grows
                .constrain_to(ui.available_rect_before_wrap())
                .show(ui, |ui| {
                    self.grid_view_controls
                        .ui(ui, &self.finalized_sim, &mut self.save_state)
                });

            let rect = ui.clip_rect(); // Full canvas

            self.grid_view_controls.update_from_canvas_events(ui, &rect);

            let painter = ui.painter_at(rect);

            // background
            painter.rect_filled(rect, 0.0, Color32::WHITE);

            let _updated = self.grid_render.maybe_update(
                ui.ctx(),
                self.finalized_sim.grid(),
                self.grid_view_controls
                    .to_render_params(rect.width() as usize, rect.height() as usize),
            );

            if let Some(handle) = self.grid_render.handle() {
                // y-flip via uv
                painter.image(
                    handle.id(),
                    rect,
                    Rect::from_min_max(pos2(0.0, 1.0), pos2(1.0, 0.0)),
                    Color32::WHITE,
                );
            }
        });

        Keep(self)
    }
}

impl GridExplorer {
    pub fn new(finalized_simulation: FinalizedSimulation) -> Self {
        let player_count = finalized_simulation.player_count();
        let grid_view_controls = GridViewControls {
            min_zoom_pow2: -3,
            max_zoom_pow2: 3,
            turns: finalized_simulation.simulated_turns(),
            memory_usage: finalized_simulation.memory_usage(),
            complete_shells: finalized_simulation.complete_shells(),
            player_count,
            player_colors: default_player_colors()[..=player_count].to_vec(),
            ..Default::default()
        };

        let grid_render = GridRender::default();

        Self {
            grid_render,
            finalized_sim: finalized_simulation,
            grid_view_controls,
            save_state: SaveState::NotSaved,
        }
    }

    pub fn load_from_file(path: PathBuf) -> Result<GridExplorer, std::io::Error> {
        let file = std::fs::File::open(path)?;
        let mut reader = std::io::BufReader::new(file);
        let simulation = FinalizedSimulation::read_from(&mut reader)?;
        let mut explorer = GridExplorer::new(simulation);
        explorer.save_state = SaveState::Saved;
        Ok(explorer)
    }
}

pub struct GridViewControls {
    min_zoom_pow2: i32,
    max_zoom_pow2: i32,
    turns: usize,
    memory_usage: usize,
    complete_shells: i32,
    player_count: usize,
    player_colors: Vec<Color32>,
    last_pointed_coords: GridPoint,

    zoom_pow2: i32,
    origin_x: f32,
    origin_y: f32,
}

impl Default for GridViewControls {
    fn default() -> Self {
        GridViewControls {
            min_zoom_pow2: 0,
            max_zoom_pow2: 0,
            turns: 0,
            memory_usage: 0,
            complete_shells: 0,
            player_count: 0,
            player_colors: default_player_colors()[..1].to_vec(),
            last_pointed_coords: GridPoint::new(0, 0),

            zoom_pow2: 0,
            origin_x: 0f32,
            origin_y: 0f32,
        }
    }
}

impl GridViewControls {
    pub fn to_render_params(
        &self,
        viewport_width: usize,
        viewport_height: usize,
    ) -> GridRenderParameters {
        let zoom = match self.zoom_pow2 {
            e @ 0.. => Magnification(Pow2::from_exponent(e as usize)),
            e @ ..0 => Minification(Pow2::from_exponent((-e) as usize)),
        };

        let bounds = match zoom {
            Magnification(factor) => {
                let origin_x = self.origin_x as i32;
                let origin_y = self.origin_y as i32;

                GridRect::with_size(
                    GridPoint::new(
                        origin_x - floor_div(viewport_width / 2, factor) as i32,
                        origin_y - floor_div(viewport_height / 2, factor) as i32,
                    ),
                    floor_div(viewport_width as i32, factor),
                    floor_div(viewport_height as i32, factor),
                )
            }
            Minification(factor) => {
                // We have to ensure proper alignment for the sampling.
                let origin_x = floor_to_multiple(self.origin_x as i32, factor);
                let origin_y = floor_to_multiple(self.origin_y as i32, factor);
                let factor_i32: i32 = factor.into();

                GridRect::with_size(
                    GridPoint::new(
                        floor_to_multiple(origin_x, factor)
                            - viewport_width as i32 / 2 * factor_i32,
                        floor_to_multiple(origin_y, factor)
                            - viewport_height as i32 / 2 * factor_i32,
                    ),
                    viewport_width as i32 * factor_i32,
                    viewport_height as i32 * factor_i32,
                )
            }
        };

        GridRenderParameters::new(bounds, self.player_colors.clone(), zoom)
    }

    fn update_from_canvas_events(&mut self, ui: &mut Ui, rect: &Rect) {
        let response = ui.allocate_rect(*rect, Sense::drag() | Sense::hover() | Sense::click());

        let get_mouse_pos_in_grid_space = |response: &Response| {
            let middle_pos = (rect.max - rect.min.to_vec2()) * 0.5f32;
            let mouse = response
                .hover_pos()
                .map(|pos| {
                    // Invert y to match world coordinates.
                    pos2(pos.x - rect.min.x, rect.height() - (pos.y - rect.min.y))
                })
                .unwrap_or(middle_pos);

            let mouse_relative_to_center = mouse - middle_pos;
            (mouse, mouse_relative_to_center)
        };

        if response.hovered() {
            let mut new_zoom_pow2 = self.zoom_pow2;
            let (mouse, mouse_relative_to_center) = get_mouse_pos_in_grid_space(&response);

            ui.input(|i| {
                for event in &i.events {
                    if let egui::Event::MouseWheel { delta, .. } = event {
                        new_zoom_pow2 += delta.y as i32;
                    }
                }
            });

            new_zoom_pow2 = new_zoom_pow2.clamp(self.min_zoom_pow2, self.max_zoom_pow2);

            {
                // Last pointed coords needs to be more precise.
                // Use the actual bounds from rendering.
                // Interpolate within the viewport.
                let render_params =
                    self.to_render_params(rect.width() as usize, rect.height() as usize);
                let tx = mouse.x / rect.width();
                let ty = mouse.y / rect.height();
                let mouse_world_x = render_params.bounds().start.x as f32 * (1.0 - tx)
                    + render_params.bounds().end.x as f32 * tx;
                let mouse_world_y = render_params.bounds().start.y as f32 * (1.0 - ty)
                    + render_params.bounds().end.y as f32 * ty;
                self.last_pointed_coords =
                    GridPoint::new(mouse_world_x.floor() as i32, mouse_world_y.floor() as i32);
            }

            // Reproject with respect to the origin
            if new_zoom_pow2 != self.zoom_pow2 {
                let old_scale = (self.zoom_pow2 as f32).exp2();
                let new_scale = (new_zoom_pow2 as f32).exp2();

                let mouse_world_x = self.origin_x + mouse_relative_to_center.x / old_scale;
                let mouse_world_y = self.origin_y + mouse_relative_to_center.y / old_scale;

                self.origin_x = mouse_world_x - mouse_relative_to_center.x / new_scale;
                self.origin_y = mouse_world_y - mouse_relative_to_center.y / new_scale;

                self.zoom_pow2 = new_zoom_pow2;
            }
        }

        // Drag keeping origin at the pointer.
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            let zoom_scale = 0.5f32.powf(self.zoom_pow2 as f32);
            self.origin_x -= zoom_scale * delta.x;
            self.origin_y += zoom_scale * delta.y;
        }

        // Set origin to current pointer placement scaled to the size of the whole grid.
        // Allows going to any region on the grid, useful for large grids.
        if response.clicked_by(egui::PointerButton::Secondary) || response.dragged_by(egui::PointerButton::Secondary) {
            let (_mouse, mouse_relative_to_center) = get_mouse_pos_in_grid_space(&response);

            let tx = mouse_relative_to_center.x / rect.width() * 2.0;
            let ty = mouse_relative_to_center.y / rect.height() * 2.0;

            self.origin_x = tx * self.complete_shells as f32;
            self.origin_y = ty * self.complete_shells as f32;
        }

        let bounds = self.complete_shells as f32;
        self.origin_x = self.origin_x.clamp(-bounds, bounds);
        self.origin_y = self.origin_y.clamp(-bounds, bounds);
    }

    fn ui(
        &mut self,
        ui: &mut Ui,
        finalized_simulation: &FinalizedSimulation,
        save_state: &mut SaveState,
    ) {
        ui.heading("Info");
        ui.label(format!("Turns: {}M", self.turns / 1000 / 1000));
        ui.label(format!("Complete shells: {}", self.complete_shells));
        ui.label(format!(
            "Size in memory: {}MiB",
            self.memory_usage / 1024 / 1024
        ));
        ui.label(format!(
            "Pointer: {}, {}",
            self.last_pointed_coords.x, self.last_pointed_coords.y
        ));
        match save_state {
            SaveState::NotSaved => {
                ui.label("Simulation is not saved!");
            }
            SaveState::Errored(err) => {
                ui.label(format!("Error while saving simulation: {}", err));
            }
            SaveState::Saved => {
                ui.label("Simulation is saved!");
            }
        };
        if ui.button("Save simulation").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .set_file_name("simulation.uls")
                .save_file()
        {
            let mut writer = BufWriter::new(std::fs::File::create(path).unwrap());
            if let Err(e) = finalized_simulation.write_to(&mut writer) {
                eprintln!("Failed to save simulation: {}", e);
                *save_state = SaveState::Errored(e);
            } else {
                *save_state = SaveState::Saved;
            }
        }

        ui.heading("Controls");
        ui.add(
            egui::Slider::new(&mut self.zoom_pow2, self.min_zoom_pow2..=self.max_zoom_pow2)
                .text("Zoom")
                .custom_formatter(|n, _| {
                    let n = n as i32;
                    if n >= 0 {
                        format!("{}x", 1 << n)
                    } else {
                        format!("1/{}x", 1 << -n)
                    }
                }),
        );

        // How many per pixel.
        let coord_drag_speed = (self.complete_shells / 200) as f64;
        ui.add(
            egui::Slider::new(
                &mut self.origin_x,
                -self.complete_shells as f32..=self.complete_shells as f32,
            )
            .text("X")
            .drag_value_speed(coord_drag_speed),
        );
        ui.add(
            egui::Slider::new(
                &mut self.origin_y,
                -self.complete_shells as f32..=self.complete_shells as f32,
            )
            .text("Y")
            .drag_value_speed(coord_drag_speed),
        );

        for player_id in 0..=self.player_count {
            ui.horizontal_wrapped(|ui| {
                ui.color_edit_button_srgba(&mut self.player_colors[player_id]);
                if player_id == 0 {
                    ui.label("Empty");
                } else {
                    ui.label(format!("Player {}", player_id));
                }
            });
            ui.end_row();
        }
    }
}

use crate::gui::SubwindowResult::Keep;
use crate::gui::grid_render::Zoom::{Magnification, Minification};
use crate::gui::grid_render::{
    GridRender, GridRenderMipMaps, GridRenderParameters, default_player_colors,
};
use crate::gui::{Subwindow, SubwindowResult};
use eframe::egui;
use eframe::egui::{Button, Checkbox, Context, Rect, Response, Sense, Ui};
use eframe::emath::pos2;
use eframe::epaint::Color32;
use std::fs::File;
use std::io::BufWriter;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use ulam_leapers::grid::{GridPoint, GridRect};
use ulam_leapers::io::{ReadFrom, WriteTo};
use ulam_leapers::simulation::{FinalizedSimulation, Game};
use ulam_leapers::util::pow2::{Pow2, floor_div, floor_to_multiple};

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
                    self.grid_view_controls.ui(
                        ui,
                        &self.finalized_sim,
                        &mut self.save_state,
                        &mut self.grid_render,
                    );
                });

            let rect = ui.clip_rect(); // Full canvas

            self.grid_view_controls
                .update_from_canvas_events(ui, &rect, &self.grid_render);

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

    fn not_ui(self: Box<Self>, _ctx: &Context) -> SubwindowResult {
        Keep(self)
    }
}

const MIN_ZOOM_POW2: i32 = -5;
const MIN_ZOOM_POW2_MIPS: i32 = -12;
const DEFAULT_ZOOM_POW2: i32 = 0;
const MAX_ZOOM_POW2: i32 = 3;

// NOTE: Currently restricted by minimum chunk alignment due to the sampling method...
const MIN_ZOOM_POW2_PNG: i32 = -6;
const DEFAULT_ZOOM_POW2_PNG: i32 = 0;
const MAX_ZOOM_POW2_PNG: i32 = 3;

const MIN_PNG_EXTENT: i32 = 256;
const DEFAULT_PNG_EXTENT: i32 = 2048;
const MAX_PNG_EXTENT: i32 = 8192;

impl GridExplorer {
    pub fn new(finalized_simulation: FinalizedSimulation) -> Self {
        let player_count = finalized_simulation.player_count();
        let grid_view_controls = GridViewControls {
            min_zoom_pow2: MIN_ZOOM_POW2,
            max_zoom_pow2: MAX_ZOOM_POW2,
            min_zoom_pow2_png: MIN_ZOOM_POW2_PNG,
            max_zoom_pow2_png: MAX_ZOOM_POW2_PNG,
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
    min_zoom_pow2_png: i32,
    max_zoom_pow2_png: i32,
    turns: usize,
    memory_usage: usize,
    complete_shells: i32,
    player_count: usize,
    player_colors: Vec<Color32>,
    last_pointed_coords: GridPoint,

    zoom_pow2: i32,
    zoom_pow2_png: i32,
    png_extent: i32,
    origin_x: f32,
    origin_y: f32,
}

impl Default for GridViewControls {
    fn default() -> Self {
        GridViewControls {
            min_zoom_pow2: 0,
            max_zoom_pow2: 0,
            min_zoom_pow2_png: 0,
            max_zoom_pow2_png: 0,
            turns: 0,
            memory_usage: 0,
            complete_shells: 0,
            player_count: 0,
            player_colors: default_player_colors()[..1].to_vec(),
            last_pointed_coords: GridPoint::new(0, 0),

            zoom_pow2: DEFAULT_ZOOM_POW2,
            zoom_pow2_png: DEFAULT_ZOOM_POW2_PNG,
            png_extent: DEFAULT_PNG_EXTENT,
            origin_x: 0f32,
            origin_y: 0f32,
        }
    }
}

fn format_zoom_slider_text(n: f64, _: RangeInclusive<usize>) -> String {
    let n = n as i32;
    if n >= 0 {
        format!("{}x", 1 << n)
    } else {
        format!("1/{}x", 1 << -n)
    }
}

impl GridViewControls {
    pub fn zoom_range(&mut self, grid_render: &GridRender) -> RangeInclusive<i32> {
        if let Some(factor) = grid_render.highest_mipmap_minification_factor() {
            (-(factor.exponent() as i32))..=self.max_zoom_pow2
        } else {
            self.min_zoom_pow2..=self.max_zoom_pow2
        }
    }

    fn render_params(
        zoom_pow2: i32,
        origin_x: f32,
        origin_y: f32,
        viewport_width: i32,
        viewport_height: i32,
        colors: Vec<Color32>,
    ) -> GridRenderParameters {
        let zoom = match zoom_pow2 {
            e @ 0.. => Magnification(Pow2::from_exponent(e as usize)),
            e @ ..0 => Minification(Pow2::from_exponent((-e) as usize)),
        };

        let bounds = match zoom {
            Magnification(factor) => {
                let origin_x = origin_x as i32;
                let origin_y = origin_y as i32;

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
                let origin_x = floor_to_multiple(origin_x as i32, factor);
                let origin_y = floor_to_multiple(origin_y as i32, factor);
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

        GridRenderParameters::new(bounds, colors, zoom)
    }

    pub fn to_render_params(
        &self,
        viewport_width: usize,
        viewport_height: usize,
    ) -> GridRenderParameters {
        Self::render_params(
            self.zoom_pow2,
            self.origin_x,
            self.origin_y,
            viewport_width as i32,
            viewport_height as i32,
            self.player_colors.clone(),
        )
    }

    fn update_from_canvas_events(&mut self, ui: &mut Ui, rect: &Rect, grid_render: &GridRender) {
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

            let zoom_range = self.zoom_range(&grid_render);
            new_zoom_pow2 = new_zoom_pow2.clamp(*zoom_range.start(), *zoom_range.end());

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
        if response.clicked_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Secondary)
        {
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
        grid_render: &mut GridRender,
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
        {
            let lowest_minification = Pow2::from_exponent((-MIN_ZOOM_POW2 + 1) as usize);
            let highest_minification = Pow2::from_exponent((-MIN_ZOOM_POW2_MIPS) as usize);
            let estimated_mipmap_memory_requirement =
                GridRenderMipMaps::estimate_memory_requirement(
                    finalized_simulation.grid(),
                    lowest_minification,
                    highest_minification,
                );
            let mip_ram_mib = estimated_mipmap_memory_requirement / 1024 / 1024;
            if ui
                .button("Generate mipmaps")
                .on_hover_text(format!(
                    "WARNING: While this will enable up to 4096x minification \
                    it does require roughly {}MiB of RAM",
                    mip_ram_mib
                ))
                .clicked()
            {
                let timer = std::time::Instant::now();
                grid_render.generate_mipmaps(
                    finalized_simulation.grid(),
                    self.player_colors.clone(),
                    lowest_minification,
                    highest_minification,
                );
                let elapsed = timer.elapsed().as_secs_f32();
                let mipmap_bounds = grid_render.mipmap_bounds().unwrap();
                println!(
                    "Mipmaps of area {}x{} generated in {:?}",
                    mipmap_bounds.width(),
                    mipmap_bounds.height(),
                    elapsed
                );
            }
        }
        let zoom_range = self.zoom_range(&grid_render);
        ui.add(
            egui::Slider::new(&mut self.zoom_pow2, zoom_range)
                .text("Zoom")
                .custom_formatter(format_zoom_slider_text),
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

        ui.heading("Screenshots ❓")
            .on_hover_text("Currently it only provides a way to save small PNG images.\n\
            Chunked [big]TIFF support for large images, separately configurable, is a future feature.");

        ui.add(
            egui::Slider::new(
                &mut self.zoom_pow2_png,
                self.min_zoom_pow2_png..=self.max_zoom_pow2_png,
            )
            .text("Zoom")
            .custom_formatter(format_zoom_slider_text),
        );

        ui.add(
            egui::Slider::new(&mut self.png_extent, MIN_PNG_EXTENT..=MAX_PNG_EXTENT)
                .text("Size")
                .logarithmic(true)
                .custom_formatter(|n, _| {
                    let s = n as i32;
                    format!("{}x{}", s, s)
                }),
        );

        if ui.button("Screenshot").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("PNG", &["png"])
                .set_file_name("image.png")
                .save_file()
        {
            let s = self.png_extent;
            let render_params = Self::render_params(
                self.zoom_pow2_png,
                self.origin_x,
                self.origin_y,
                s,
                s,
                self.player_colors.clone(),
            );
            let image = GridRender::render_to_rgba_image(
                &render_params,
                finalized_simulation.grid(),
                &None,
            );

            let file = File::create(path).unwrap();
            let w = BufWriter::new(file);
            let mut encoder = png::Encoder::new(w, s as u32, s as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);

            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(image.as_raw()).unwrap();
        }
    }
}

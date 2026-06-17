use crate::gui::conv::{egui_to_grid_point, grid_rect_to_egui};
use crate::gui::grid_render::canvas::{GridCamera, GridCanvas};
use crate::gui::grid_render::render::{
    default_player_colors, GridRender, GridRenderer, MipmapGenerationProgress,
};
use crate::gui::subwindow::SubwindowResult::Keep;
use crate::gui::subwindow::{Subwindow, SubwindowResult};
use crate::gui::util::{format_zoom_slider_text, scroll_delta_in_ui};
use crate::gui::widgets::misc::srgb_color_button;
use eframe::egui;
use eframe::egui::{
    vec2, Align2, Button, Context, Key, KeyboardShortcut, Modifiers, Rect, Sense, Stroke, StrokeKind,
    Ui,
};
use eframe::emath::pos2;
use eframe::epaint::Color32;
use std::fs::File;
use std::io::BufWriter;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use ulam_leapers::game::chunk::BoundedChunk;
use ulam_leapers::game::persist::uls::{UlsError, UlsSimulation};
use ulam_leapers::game::simulation::{FinalizedSimulation, Game};
use ulam_leapers::math::coords::{GridPoint, Point2D};
use ulam_leapers::math::pow2::Pow2;
use ulam_leapers::math::rect::{GridRect, Rect2D};
use ulam_leapers::util::memory::MemSize;

const MIN_ZOOM_POW2: i32 = -5;
const MIN_ZOOM_POW2_MIPS: i32 = -12;
const DEFAULT_ZOOM_POW2: i32 = 0;
const MAX_ZOOM_POW2: i32 = 5;

const MIP_LOWEST_MINIFICATION: Pow2 = Pow2::from_exponent((-MIN_ZOOM_POW2 + 1) as u8);
const MIP_HIGHEST_MINIFICATION: Pow2 = Pow2::from_exponent((-MIN_ZOOM_POW2_MIPS) as u8);

const MIN_PNG_EXTENT: i32 = 256;
const DEFAULT_PNG_EXTENT: i32 = 2048;
const MAX_PNG_EXTENT: i32 = 8192;

const MIN_MIPMAP_MEMORY_REQUIREMENT_TO_SHOW_WARNING: MemSize = MemSize::mb(128);

const SAVE_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::S);
const DEBUG_UI_TOGGLE_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F3);

#[derive(Debug)]
pub enum SaveState {
    NotSaved,
    Saved,
    Incompatible(UlsError),
    Errored(std::io::Error),
}

pub struct GridExplorer {
    finalized_simulation: FinalizedSimulation,
    grid_renderer: GridRenderer,
    grid_render: Option<GridRender>,
    camera: GridCamera,

    mipmap_generation_progress: Option<MipmapGenerationProgress>,

    player_colors: Vec<Color32>,
    save_state: SaveState,

    zoom_pow2_png: i32,
    png_extent: i32,

    is_debug_ui_enabled: bool,
}

impl Subwindow for GridExplorer {
    fn name(&self) -> String {
        if self.is_saved() {
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
                    self.show_controls_window_ui(ui);
                });

            if ui.input_mut(|i| i.consume_shortcut(&DEBUG_UI_TOGGLE_SHORTCUT)) {
                self.is_debug_ui_enabled = !self.is_debug_ui_enabled;
            }

            // The projection will give us a more restricted viewport.
            let mut canvas = GridCanvas::in_ui(ui, self.camera);

            self.update_canvas_from_events(ui, &mut canvas);

            self.maybe_update_canvas_texture(ui, &canvas);
            self.draw_canvas_texture(ui, &canvas);

            if self.is_debug_ui_enabled {
                self.show_debug_ui(ui, &canvas);
            }
        });

        Keep(self)
    }

    fn not_ui(self: Box<Self>, _ctx: &Context) -> SubwindowResult {
        Keep(self)
    }
}

impl GridExplorer {
    pub fn new(finalized_simulation: FinalizedSimulation) -> Self {
        let max_id = finalized_simulation.highest_player_id();

        let grid_renderer = GridRenderer::new(
            &finalized_simulation,
            default_player_colors(max_id).as_slice(),
        );

        Self {
            grid_renderer,
            finalized_simulation,
            grid_render: None,
            camera: GridCamera::new(DEFAULT_ZOOM_POW2, Point2D::new(0.0, 0.0)),

            mipmap_generation_progress: None,

            player_colors: default_player_colors(max_id).to_vec(),
            save_state: SaveState::NotSaved,

            zoom_pow2_png: DEFAULT_ZOOM_POW2,
            png_extent: DEFAULT_PNG_EXTENT,

            is_debug_ui_enabled: false,
        }
    }

    pub fn load_from_file(path: PathBuf) -> Result<GridExplorer, std::io::Error> {
        let file = File::open(path)?;
        let mut reader = std::io::BufReader::new(file);
        let uls_sim = UlsSimulation::read_from(&mut reader)?;
        let simulation = FinalizedSimulation::from(uls_sim);
        let mut explorer = GridExplorer::new(simulation);
        explorer.assume_saved();
        Ok(explorer)
    }

    fn is_saved(&self) -> bool {
        matches!(self.save_state, SaveState::Saved)
    }

    fn assume_saved(&mut self) {
        self.save_state = SaveState::Saved;
    }

    fn zoom_range(&self) -> RangeInclusive<i32> {
        if let Some(factor) = self.grid_renderer.highest_mipmap_minification_factor() {
            (-(factor.exponent() as i32))..=MAX_ZOOM_POW2
        } else {
            MIN_ZOOM_POW2..=MAX_ZOOM_POW2
        }
    }

    fn draw_canvas_texture(&mut self, ui: &mut Ui, canvas: &GridCanvas) {
        if canvas.is_zero_area() {
            return;
        }

        let painter = canvas.make_painter(ui);
        let rect = painter.clip_rect();

        // background
        painter.rect_filled(rect, 0.0, self.player_colors[0]);

        if let Some(render) = &self.grid_render {
            // y-flip via uv
            painter.image(
                render.texture().id(),
                rect,
                Rect::from_min_max(pos2(0.0, 1.0), pos2(1.0, 0.0)),
                Color32::WHITE,
            );
        }
    }

    fn maybe_update_canvas_texture(&mut self, ui: &mut Ui, canvas: &GridCanvas) {
        // For the caching to be effective there needs to space for at least a few
        // framebuffers worth of data.
        const CACHE_FRAMEBUFFERS_WORTH: usize = 16;

        if canvas.is_zero_area() {
            return;
        }

        let world_bounds = canvas.world_rect();
        let zoom = canvas.zoom();

        if self
            .grid_render
            .as_ref()
            .is_none_or(|v| v.is_outdated(&self.grid_renderer, world_bounds, zoom))
        {
            let framebuffer_size =
                canvas.width() as usize * canvas.height() as usize * size_of::<Color32>();

            self.grid_renderer
                .set_cache_size(framebuffer_size * CACHE_FRAMEBUFFERS_WORTH);

            self.grid_render = Some(self.grid_renderer.render_texture(
                ui.ctx(),
                world_bounds,
                zoom,
            ));
        }
    }

    fn show_pointed_chunk_overlay(&mut self, ui: &mut Ui, canvas: &GridCanvas) {
        if canvas.is_zero_area() {
            return;
        }

        if let Some(egui_mouse_pos) = ui.pointer_latest_pos() {
            let mouse_pos = egui_to_grid_point(egui_mouse_pos);
            let pointed_coords = canvas.screen_to_world(mouse_pos);
            let chunk = self
                .finalized_simulation
                .get_chunk_containing(&pointed_coords);
            if let Some(chunk) = chunk {
                let chunk_bounds = chunk.bounds();
                let chunk_bounds_screen_space = canvas.world_to_screen_rect(*chunk_bounds);
                let painter = canvas.make_painter(ui);
                painter.rect(
                    grid_rect_to_egui(chunk_bounds_screen_space),
                    0,
                    Color32::TRANSPARENT,
                    Stroke::new(2.0, Color32::GREEN),
                    StrokeKind::Outside,
                );

                let coords_bounds = GridRect::with_size(pointed_coords, 1, 1);
                let coords_bounds_screen_space = canvas.world_to_screen_rect(coords_bounds);
                painter.rect(
                    grid_rect_to_egui(coords_bounds_screen_space),
                    0,
                    Color32::TRANSPARENT,
                    Stroke::new(1.0, Color32::BLUE),
                    StrokeKind::Outside,
                );

                let text = format!(
                    "Cursor: ({}, {})\n\
                    Bounds: ({}, {}), ({}, {})\n\
                    Memsize: {}",
                    pointed_coords.x,
                    pointed_coords.y,
                    chunk_bounds.start.x,
                    chunk_bounds.start.y,
                    chunk_bounds.end.x,
                    chunk_bounds.end.y,
                    chunk.memory_usage().display().si()
                );

                // Offsets slightly to prevent occlusion by the cursor.
                let offset = vec2(16.0, 24.0);
                painter.debug_text(
                    egui_mouse_pos + offset,
                    Align2::LEFT_TOP,
                    Color32::BLACK,
                    text,
                );
            }
        }
    }

    fn show_debug_ui(&mut self, ui: &mut Ui, canvas: &GridCanvas) {
        self.show_pointed_chunk_overlay(ui, canvas);
    }

    fn make_camera_position_bounds(&self) -> Rect2D<f32> {
        let complete_shells = self.finalized_simulation.complete_shells();
        let complete_shells_f32 = complete_shells as f32;
        Rect2D::with_start_end(
            Point2D::new(-complete_shells_f32, -complete_shells_f32),
            Point2D::new(complete_shells_f32, complete_shells_f32),
        )
    }

    fn update_canvas_from_events(&mut self, ui: &mut Ui, canvas: &mut GridCanvas) {
        if canvas.is_zero_area() {
            return;
        }

        let response = canvas.make_sense(ui, Sense::drag() | Sense::hover() | Sense::click());

        let mouse_pos = egui_to_grid_point(
            response
                .hover_pos()
                .unwrap_or_else(|| response.rect.min - response.rect.size() * 0.5),
        );

        let zoom_range = self.zoom_range();
        let camera_position_bounds = self.make_camera_position_bounds();

        let mut new_camera = self.camera.restricted(zoom_range, camera_position_bounds);

        let zoom_delta = if response.hovered() {
            scroll_delta_in_ui(ui)
        } else {
            0
        };

        new_camera.add_zoom_with_invariant_point(canvas, zoom_delta, mouse_pos);

        // Drag keeping origin at the pointer.
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            // Normally both would be negated but we flip y.
            new_camera.drag(-delta.x, delta.y);
        }

        // Set origin to current pointer placement scaled to the size of the whole grid.
        // Allows going to any region on the grid, useful for large grids.
        if response.clicked_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Secondary)
        {
            let tx = mouse_pos.x as f32 / canvas.width() as f32;
            let ty = 1.0 - mouse_pos.y as f32 / canvas.height() as f32;

            new_camera.set_position_proportional_within_bounds(tx, ty);
        }

        // Update canvas if anything changed.
        let new_camera = new_camera.to_camera();
        if new_camera != self.camera {
            self.camera = new_camera;
            *canvas = canvas.with_camera(self.camera);
        }
    }

    fn try_save(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("simulation.uls")
            .save_file()
        {
            let mut writer = BufWriter::new(File::create(path).unwrap());
            match UlsSimulation::try_from(&self.finalized_simulation) {
                Err(err) => self.save_state = SaveState::Incompatible(err),
                Ok(uls_sim) => {
                    if let Err(e) = uls_sim.write_to(&mut writer) {
                        eprintln!("Failed to save simulation: {}", e);
                        self.save_state = SaveState::Errored(e);
                    } else {
                        self.save_state = SaveState::Saved;
                    }
                }
            }
        }
    }

    fn show_save_ui(&mut self, ui: &mut Ui) {
        match &self.save_state {
            SaveState::NotSaved => {
                ui.label("Simulation is not saved!");
            }
            SaveState::Incompatible(err) => {
                ui.label(format!("Simulation incompatible with ULS format: {}", err));
            }
            SaveState::Errored(err) => {
                ui.label(format!("Error while saving simulation: {}", err));
            }
            SaveState::Saved => {
                ui.label("Simulation is saved!");
            }
        };

        let button =
            Button::new("Save simulation").shortcut_text(ui.ctx().format_shortcut(&SAVE_SHORTCUT));
        let save = ui.add(button).clicked() || ui.input_mut(|i| i.consume_shortcut(&SAVE_SHORTCUT));
        if save {
            self.try_save();
        }
    }

    fn show_mipmaps_ui(&mut self, ui: &mut Ui) {
        // Handle various stages of mipmap generation.
        // We rely directly on the state reported by the grid renderer instead of the
        // progress from the callback.
        if self.grid_renderer.has_mipmaps() {
            ui.label("Mipmaps are generated.");
        } else if self.grid_renderer.can_generate_mipmaps() {
            let estimated_mipmaps_memory_requirement =
                self.grid_renderer.estimate_mipmaps_memory_requirement(
                    MIP_LOWEST_MINIFICATION,
                    MIP_HIGHEST_MINIFICATION,
                );
            let on_hover_text = if estimated_mipmaps_memory_requirement
                >= MIN_MIPMAP_MEMORY_REQUIREMENT_TO_SHOW_WARNING
            {
                format!(
                    "WARNING: While this will enable up to {}x minification \
                it does require roughly {} of RAM and may take a long time.\
                This process is asynchronous.",
                    MIP_HIGHEST_MINIFICATION,
                    estimated_mipmaps_memory_requirement.display().si(),
                )
            } else {
                format!(
                    "This will enable up to {}x minification.",
                    MIP_LOWEST_MINIFICATION
                )
            };

            if ui
                .button("Generate mipmaps")
                .on_hover_text(on_hover_text)
                .clicked()
            {
                self.mipmap_generation_progress = Some(
                    self.grid_renderer
                        .generate_mipmaps_async(MIP_LOWEST_MINIFICATION, MIP_HIGHEST_MINIFICATION),
                );
            }
        } else
        /* if mipmap generation in progress */
        {
            if ui.button("Cancel mipmap generation.").clicked() {
                self.grid_renderer.cancel_mipmap_generation();
            } else if let Some(progress) = &self.mipmap_generation_progress {
                let progress = progress.get();
                let progress_pct = (progress.0 * 100).checked_div(progress.1).unwrap_or(0);
                ui.label(format!(
                    "{} / {} chunks ({}%)",
                    progress.0, progress.1, progress_pct
                ));
                // Maybe some better notification in the future, but chunks get processed fast
                // enough that this shouldn't be doing any redundant work.
                ui.ctx().request_repaint();
            } else {
                // We should never reach this state, but the progress reporting is
                // inherently asynchronous and imprecise, so we may still end up here
                // in extreme cases. It's not an error.
            }
        }
    }

    fn show_camera_ui(&mut self, ui: &mut Ui) {
        let complete_shells_f32 = self.finalized_simulation.complete_shells() as f32;
        let zoom_range = self.zoom_range();

        ui.add(
            egui::Slider::new(&mut self.camera.zoom_pow2, zoom_range.clone())
                .text("Zoom")
                .custom_formatter(format_zoom_slider_text),
        );

        ui.add(
            egui::Slider::new(
                &mut self.camera.position.x,
                -complete_shells_f32..=complete_shells_f32,
            )
            .text("X"),
        );

        ui.add(
            egui::Slider::new(
                &mut self.camera.position.y,
                -complete_shells_f32..=complete_shells_f32,
            )
            .text("Y"),
        );
    }

    fn show_player_colors_ui(&mut self, ui: &mut Ui) {
        let player_count = self.finalized_simulation.player_count();
        let allow_color_change = self.grid_renderer.can_set_colors();

        // TODO: Columns for some reason take more space than necessary.
        //       This `set_max_width` is a hack to make it about as much as it should.
        ui.set_max_width(200.0);
        ui.columns(2, |columns| {
            for player_id in 0..=player_count {
                let column = &mut columns[player_id % 2];
                column.horizontal_wrapped(|ui| {
                    if srgb_color_button(ui, &mut self.player_colors[player_id], allow_color_change)
                        .changed()
                    {
                        self.grid_renderer.set_colors(self.player_colors.as_slice())
                    }

                    if player_id == 0 {
                        ui.label("Empty");
                    } else {
                        ui.label(format!("Player {}", player_id));
                    }
                });
            }
        })
    }

    fn show_info_ui(&mut self, ui: &mut Ui) {
        let turns = self.finalized_simulation.complete_turns();
        let complete_shells = self.finalized_simulation.complete_shells();
        let side_cells = complete_shells.max(1) as usize * 2 - 1;
        let cells = side_cells * side_cells;
        let chunks = self.finalized_simulation.chunk_count();
        let memory_usage = self.finalized_simulation.memory_usage();

        ui.label(format!("Turns: {}M", turns / 1000 / 1000));
        ui.label(format!("Complete shells: {}", complete_shells));
        ui.label(format!("Number of cells: {}M", cells / 1000 / 1000));
        ui.label(format!("Number of chunks: {}", chunks));
        ui.label(format!("Size in memory: {}", memory_usage.display().si()));
    }

    fn show_screenshots_ui(&mut self, ui: &mut Ui) {
        let zoom_range = self.zoom_range();

        ui.add(
            egui::Slider::new(&mut self.zoom_pow2_png, zoom_range)
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
            let canvas = GridCanvas::new(
                self.camera.with_zoom(self.zoom_pow2_png),
                GridRect::with_size(GridPoint::zero(), s, s),
            );
            let image = self
                .grid_renderer
                .render_to_rgba_image(canvas.world_rect(), canvas.zoom());

            let file = File::create(path).unwrap();
            let w = BufWriter::new(file);
            let mut encoder = png::Encoder::new(w, image.width() as u32, image.height() as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);

            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(image.as_raw()).unwrap();
        }
    }

    fn show_controls_window_ui(&mut self, ui: &mut Ui) {
        ui.heading("Info");

        self.show_info_ui(ui);
        self.show_save_ui(ui);

        ui.heading("Controls");

        self.show_mipmaps_ui(ui);
        self.show_camera_ui(ui);
        self.show_player_colors_ui(ui);

        ui.heading("Screenshots ❓")
            .on_hover_text("Currently it only provides a way to save small PNG images.\n\
            Chunked [big]TIFF support for large images, separately configurable, is a future feature.");

        self.show_screenshots_ui(ui);
    }
}

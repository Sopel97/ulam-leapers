use crate::gui::conv::{egui_to_grid_point, grid_rect_to_egui};
use crate::gui::grid_render::canvas::{GridCamera, GridCanvas};
use crate::gui::grid_render::render::{
    default_player_colors, GridRender, GridRenderer, MipmapGenerationProgress,
};
use crate::gui::subwindow::SubwindowResult::Keep;
use crate::gui::subwindow::{Subwindow, SubwindowResult};
use crate::gui::util::{format_zoom_slider_text, make_player_name, scroll_delta_in_ui};
use crate::gui::widgets::leaper_attacks::LeaperAttacksView;
use crate::gui::widgets::misc::srgb_color_button;
use crate::gui::widgets::player_colors::show_player_colors_ui;
use crate::gui::widgets::player_relations::PlayerRelationsView;
use crate::gui::widgets::simulation_info::show_finalized_simulation_info_ui;
use crate::gui::widgets::widget::StatefulWidget;
use eframe::egui;
use eframe::egui::{
    vec2, Align2, Button, Context, Key, KeyboardShortcut, Modifiers, Painter, Rect, Sense,
    Stroke, StrokeKind, Ui,
};
use eframe::emath::pos2;
use eframe::epaint::Color32;
use std::cmp;
use std::fs::File;
use std::io::BufWriter;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use ulam_leapers::game::chunk::BoundedChunk;
use ulam_leapers::game::persist::uls::{UlsError, UlsSimulation};
use ulam_leapers::game::sampler::FrozenGridCellAccessor;
use ulam_leapers::game::simulation::{FinalizedSimulation, Game, PlayerId};
use ulam_leapers::math::coords::{GridPoint, Point2D};
use ulam_leapers::math::pow2::Pow2;
use ulam_leapers::math::rect::{GridRect, Rect2D};
use ulam_leapers::math::zoom::Zoom;
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
const OVERLAYS_UI_TOGGLE_SHORTCUT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::NONE, Key::F3);

const MAX_CONTROLS_WINDOW_WIDTH: f32 = 200.0;

const DEFAULT_SIMULATION_FILE_NAME: &str = "simulation.uls";

#[derive(Debug)]
pub enum SaveState {
    NotSaved,
    Saved(PathBuf),
    Incompatible(UlsError),
    Errored(std::io::Error),
}

pub struct GridExplorer {
    finalized_simulation: FinalizedSimulation,
    grid_renderer: GridRenderer,
    grid_render: Option<GridRender>,
    camera: GridCamera,

    grid_cell_accessor: FrozenGridCellAccessor<PlayerId>,

    mipmap_generation_progress: Option<Arc<Mutex<MipmapGenerationProgress>>>,

    player_colors: Vec<Color32>,
    save_state: SaveState,

    zoom_pow2_png: i32,
    png_extent: i32,

    are_overlays_enabled: bool,
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

            egui::Window::new("Players")
                .scroll(true)
                .resizable(true)
                .constrain_to(ui.available_rect_before_wrap())
                .anchor(Align2::LEFT_TOP, vec2(0.0, 0.0))
                .default_open(false)
                .show(ui, |ui| {
                    self.show_players_ui(ui);
                });

            if ui.input_mut(|i| i.consume_shortcut(&OVERLAYS_UI_TOGGLE_SHORTCUT)) {
                self.are_overlays_enabled = !self.are_overlays_enabled;
            }

            // The projection will give us a more restricted viewport.
            let mut canvas = GridCanvas::in_ui(ui, self.camera);

            self.update_canvas_from_events(ui, &mut canvas);

            self.maybe_update_canvas_texture(ui, &canvas);
            self.draw_canvas_texture(ui, &canvas);

            if self.are_overlays_enabled {
                self.show_overlays_ui(ui, &canvas);
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

        let grid_ref = Arc::clone(&finalized_simulation.grid());

        Self {
            grid_renderer,
            finalized_simulation,
            grid_render: None,
            camera: GridCamera::new(DEFAULT_ZOOM_POW2, Point2D::new(0.0, 0.0)),

            grid_cell_accessor: FrozenGridCellAccessor::new(grid_ref, 4),

            mipmap_generation_progress: None,

            player_colors: default_player_colors(max_id).to_vec(),
            save_state: SaveState::NotSaved,

            zoom_pow2_png: DEFAULT_ZOOM_POW2,
            png_extent: DEFAULT_PNG_EXTENT,

            are_overlays_enabled: false,
        }
    }

    pub fn load_from_file(path: PathBuf) -> Result<GridExplorer, std::io::Error> {
        let file = File::open(path.clone())?;
        let mut reader = std::io::BufReader::new(file);
        let uls_sim = UlsSimulation::read_from(&mut reader)?;
        let simulation = FinalizedSimulation::from(uls_sim);
        let mut explorer = GridExplorer::new(simulation);
        explorer.assume_saved(path);
        Ok(explorer)
    }

    fn is_saved(&self) -> bool {
        matches!(self.save_state, SaveState::Saved(_))
    }

    fn assume_saved(&mut self, path: PathBuf) {
        self.save_state = SaveState::Saved(path);
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

    fn draw_overlay_rect(
        canvas: &GridCanvas,
        painter: &Painter,
        rect: &GridRect,
        stroke: Stroke,
        stroke_kind: StrokeKind,
    ) {
        if let Some(rect_screen_space) = canvas
            .world_to_screen_rect(*rect)
            // This intersection is required because egui cannot properly render
            // rects that extend far beyond the visible area.
            // We expand the clipping area slightly to account for the stroke.
            .intersection(&canvas.screen_rect().expanded(stroke.width as i32 + 1))
        {
            painter.rect(
                grid_rect_to_egui(rect_screen_space),
                0,
                Color32::TRANSPARENT,
                stroke,
                stroke_kind,
            );
        }
    }

    fn draw_overlay_point(
        canvas: &GridCanvas,
        painter: &Painter,
        point: GridPoint,
        stroke: Stroke,
        stroke_kind: StrokeKind,
    ) {
        let rect = GridRect::with_size(point, 1, 1);
        Self::draw_overlay_rect(canvas, painter, &rect, stroke, stroke_kind);
    }

    fn draw_overlay_shell(canvas: &GridCanvas, painter: &Painter, shell: u32, stroke: Stroke) {
        let shell = shell as i32;
        let inner_shell_bounds = GridRect::with_size(
            GridPoint::new(-shell + 1, -shell + 1),
            shell * 2 - 1,
            shell * 2 - 1,
        );
        let outer_shell_bounds =
            GridRect::with_size(GridPoint::new(-shell, -shell), shell * 2 + 1, shell * 2 + 1);
        match canvas.zoom() {
            Zoom::Magnification(_) => {
                if shell > 0 {
                    Self::draw_overlay_rect(
                        canvas,
                        painter,
                        &inner_shell_bounds,
                        stroke,
                        StrokeKind::Inside,
                    );
                }
                Self::draw_overlay_rect(
                    canvas,
                    painter,
                    &outer_shell_bounds,
                    stroke,
                    StrokeKind::Outside,
                );
            }
            Zoom::Minification(_) => {
                Self::draw_overlay_rect(
                    canvas,
                    painter,
                    &outer_shell_bounds,
                    stroke,
                    StrokeKind::Middle,
                );
            }
        }
    }

    fn show_pointer_overlays(&mut self, ui: &mut Ui, canvas: &GridCanvas) {
        if canvas.is_zero_area() {
            return;
        }

        if let Some(egui_mouse_pos) = ui.pointer_latest_pos() {
            let painter = canvas.make_painter(ui);
            let mouse_pos = egui_to_grid_point(egui_mouse_pos);
            let pointed_coords = canvas.screen_to_world(mouse_pos);

            if matches!(self.camera.zoom(), Zoom::Magnification(_))
                && let Some(pid) = self.grid_cell_accessor.get(pointed_coords)
                && let Some(player) = self.finalized_simulation.player(pid)
            {
                for attacked_coords in player.attacks().get_attacks_from(&pointed_coords) {
                    if let Some(attacked_pid) = self.grid_cell_accessor.get(attacked_coords) {
                        let is_attacked_pid_enemy = player.enemies().is_set(attacked_pid);
                        let color = match is_attacked_pid_enemy {
                            true => Color32::DARK_RED,
                            false => Color32::DARK_GRAY,
                        };
                        Self::draw_overlay_point(
                            canvas,
                            &painter,
                            attacked_coords,
                            Stroke::new(2.0, color),
                            StrokeKind::Inside,
                        );
                    }
                }
            }

            Self::draw_overlay_point(
                canvas,
                &painter,
                pointed_coords,
                Stroke::new(1.0, Color32::LIGHT_BLUE),
                StrokeKind::Outside,
            );

            let shell = cmp::max(
                pointed_coords.x.unsigned_abs(),
                pointed_coords.y.unsigned_abs(),
            );
            Self::draw_overlay_shell(canvas, &painter, shell, Stroke::new(1.0, Color32::GOLD));

            let chunk = self
                .finalized_simulation
                .get_chunk_containing(&pointed_coords);

            if let Some(chunk) = chunk {
                let chunk_bounds = chunk.bounds();
                Self::draw_overlay_rect(
                    canvas,
                    &painter,
                    chunk_bounds,
                    Stroke::new(2.0, Color32::LIGHT_GREEN),
                    StrokeKind::Outside,
                );

                let cursor_line = match self.grid_cell_accessor.get(pointed_coords) {
                    Some(pid) => {
                        let name = make_player_name(pid);
                        format!(
                            "Cursor: {} at ({}, {})",
                            name, pointed_coords.x, pointed_coords.y,
                        )
                    }
                    None => {
                        format!("Cursor: ({}, {})", pointed_coords.x, pointed_coords.y,)
                    }
                };

                let text = format!(
                    "{cursor_line}\n\
                    Shell: {}\n\
                    Bounds: ({}, {}), ({}, {})\n\
                    Memsize: {}",
                    shell,
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

    fn show_overlays_ui(&mut self, ui: &mut Ui, canvas: &GridCanvas) {
        self.show_pointer_overlays(ui, canvas);
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
        let suggested_name = match &self.save_state {
            SaveState::Saved(path) => path
                .file_name()
                .expect("If saved it should be a file.")
                .display()
                .to_string(),
            _ => DEFAULT_SIMULATION_FILE_NAME.parse().unwrap(),
        };

        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(suggested_name)
            .save_file()
        {
            let mut writer = BufWriter::new(File::create(path.clone()).unwrap());
            match UlsSimulation::try_from(&self.finalized_simulation) {
                Err(err) => self.save_state = SaveState::Incompatible(err),
                Ok(uls_sim) => {
                    if let Err(e) = uls_sim.write_to(&mut writer) {
                        eprintln!("Failed to save simulation: {}", e);
                        self.save_state = SaveState::Errored(e);
                    } else {
                        self.save_state = SaveState::Saved(path);
                    }
                }
            }
        }
    }

    fn show_save_ui(&mut self, ui: &mut Ui) {
        ui.scope(|ui| {
            ui.set_max_width(MAX_CONTROLS_WINDOW_WIDTH);

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
                SaveState::Saved(path) => {
                    let path_display = path.display();
                    ui.label(format!("Saved at {path_display}"));
                }
            };
        });

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
                let progress = *progress.lock().unwrap();
                match progress {
                    MipmapGenerationProgress::LargestMipmap {
                        chunks_done,
                        chunks_total,
                    } => {
                        let progress_pct =
                            (chunks_done * 100).checked_div(chunks_total).unwrap_or(0);
                        ui.label(format!(
                            "{} / {} chunks ({}%)",
                            chunks_done, chunks_total, progress_pct
                        ));
                    }
                    MipmapGenerationProgress::SmallerMipmap { zoom } => {
                        ui.label(format!("Processing zoom {}", zoom));
                    }
                }
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
        let allow_color_change = self.grid_renderer.can_set_colors();

        if show_player_colors_ui(ui, &mut self.player_colors, allow_color_change) {
            self.grid_renderer.set_colors(self.player_colors.as_slice());
        }
    }

    fn show_info_ui(&mut self, ui: &mut Ui) {
        show_finalized_simulation_info_ui(&self.finalized_simulation, ui);

        ui.checkbox(&mut self.are_overlays_enabled, "Overlays");
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

    fn show_players_ui(&mut self, ui: &mut Ui) {
        let players = self.finalized_simulation.players();

        ui.horizontal_top(|ui| {
            ui.group(|ui| {
                ui.vertical(|ui| {
                    for (i, player) in players.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let pid = PlayerId::new((i + 1) as u8);
                            srgb_color_button(ui, &mut self.player_colors[pid.index()], false);
                            let name = make_player_name(pid);
                            ui.label(format!("{name} attacks"));
                        });
                        let mut widget = LeaperAttacksView::new(player.attacks());
                        widget.ui(ui);
                    }
                });
            });

            let mut relations_widget = PlayerRelationsView::new(players);
            relations_widget.ui(ui);
        });
    }
}

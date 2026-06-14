use crate::gui::render::grid_render::{
    default_player_colors, GridRenderer, MipmapGenerationProgress,
};
use crate::gui::subwindow::SubwindowResult::Keep;
use crate::gui::subwindow::{Subwindow, SubwindowResult};
use eframe::egui;
use eframe::egui::color_picker::Alpha;
use eframe::egui::{color_picker, vec2, Context, Key, KeyboardShortcut, Modifiers, Rect, Response, Sense, Stroke, StrokeKind, TextureHandle, Ui};
use eframe::emath::pos2;
use eframe::epaint::Color32;
use std::fs::File;
use std::io::BufWriter;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use ulam_leapers::game::chunk::BoundedChunk;
use ulam_leapers::game::simulation::{FinalizedSimulation, Game};
use ulam_leapers::io::{ReadFrom, WriteTo};
use ulam_leapers::math::coords::GridPoint;
use ulam_leapers::math::pow2::{floor_to_multiple, mod_floor, Pow2};
use ulam_leapers::math::projection::{FlipAxis, ScreenWorldDiscrete2D};
use ulam_leapers::math::rect::GridRect;
use ulam_leapers::math::zoom::Zoom;
use ulam_leapers::util::memory::MemSize;
use crate::gui::conv::{egui_to_grid_rect, grid_rect_to_egui};

#[derive(Debug)]
pub enum SaveState {
    NotSaved,
    Saved,
    Errored(std::io::Error),
}

pub struct GridExplorer {
    grid_view_controls: GridViewControls,
    finalized_simulation: Arc<FinalizedSimulation>,
    grid_renderer: Arc<Mutex<GridRenderer>>,
    grid_render_texture: Option<TextureHandle>,
    last_grid_render_params: GridRenderParameters,
    is_debug_ui_enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GridRenderParameters {
    bounds: GridRect,
    zoom: Zoom<Pow2>,
}

impl GridRenderParameters {
    pub fn new(bounds: GridRect, zoom: Zoom<Pow2>) -> Self {
        Self { bounds, zoom }
    }

    pub fn bounds(&self) -> GridRect {
        self.bounds
    }
}

impl Default for GridRenderParameters {
    fn default() -> Self {
        GridRenderParameters {
            bounds: GridRect::with_size(GridPoint::new(0, 0), 0, 0),
            zoom: Zoom::Magnification(Pow2::try_from(1).unwrap()),
        }
    }
}

impl Subwindow for GridExplorer {
    fn name(&self) -> String {
        if self.grid_view_controls.is_saved() {
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
                    self.grid_view_controls.ui(ui);
                });

            // The projection will give us a more restricted viewport.
            let proj = GridViewControls::make_projection(
                self.grid_view_controls.zoom_pow2,
                GridPoint::new(self.grid_view_controls.origin_x as i32, self.grid_view_controls.origin_y as i32),
                egui_to_grid_rect(ui.clip_rect()),
            );
            let rect = proj.screen_rect();

            if rect.width() >= MIN_CANVAS_WIDTH && rect.height() >= MIN_CANVAS_HEIGHT {
                self.maybe_update_canvas_texture(ui, rect);
                self.draw_canvas_texture(ui, rect);
            }

            if ui.input_mut(|i| i.consume_shortcut(&DEBUG_UI_TOGGLE_SHORTCUT)) {
                self.is_debug_ui_enabled = !self.is_debug_ui_enabled;
            }
            if self.is_debug_ui_enabled {
                self.show_debug_ui(ui, rect);
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

const MIN_PNG_EXTENT: i32 = 256;
const DEFAULT_PNG_EXTENT: i32 = 2048;
const MAX_PNG_EXTENT: i32 = 8192;

const MIN_MIPMAP_MEMORY_REQUIREMENT_TO_SHOW_WARNING: MemSize = MemSize::mb(128);

const SAVE_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::S);
const DEBUG_UI_TOGGLE_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F3);

const SCREEN_TO_WORLD_AXIS_FLIP: FlipAxis = FlipAxis::Y;

const MIN_CANVAS_WIDTH: i32 = 1;
const MIN_CANVAS_HEIGHT: i32 = 1;

impl GridExplorer {
    pub fn new(finalized_simulation: FinalizedSimulation) -> Self {
        let finalized_simulation = Arc::new(finalized_simulation);
        let grid_renderer = Arc::new(Mutex::new(GridRenderer::new(
            &finalized_simulation,
            default_player_colors().as_slice(),
        )));

        let player_count = finalized_simulation.player_count();
        let grid_view_controls = GridViewControls {
            finalized_simulation: Arc::clone(&finalized_simulation),
            grid_renderer: Arc::clone(&grid_renderer),
            mipmap_generation_progress: None,

            min_zoom_pow2: MIN_ZOOM_POW2,
            max_zoom_pow2: MAX_ZOOM_POW2,
            player_colors: default_player_colors()[..=player_count].to_vec(),
            last_pointed_coords: GridPoint::new(0, 0),
            save_state: SaveState::NotSaved,

            zoom_pow2: DEFAULT_ZOOM_POW2,
            zoom_pow2_png: DEFAULT_ZOOM_POW2,
            png_extent: DEFAULT_PNG_EXTENT,
            origin_x: 0.0,
            origin_y: 0.0,
            have_colors_changed: false,
        };

        Self {
            grid_renderer,
            finalized_simulation,
            grid_view_controls,
            grid_render_texture: None,
            last_grid_render_params: Default::default(),
            is_debug_ui_enabled: false,
        }
    }

    pub fn load_from_file(path: PathBuf) -> Result<GridExplorer, std::io::Error> {
        let file = File::open(path)?;
        let mut reader = std::io::BufReader::new(file);
        let simulation = FinalizedSimulation::read_from(&mut reader)?;
        let mut explorer = GridExplorer::new(simulation);
        explorer.assume_saved();
        Ok(explorer)
    }

    fn assume_saved(&mut self) {
        self.grid_view_controls.assume_saved();
    }

    fn draw_canvas_texture(&mut self, ui: &mut Ui, rect: GridRect) {
        let egui_rect = grid_rect_to_egui(rect);
        let painter = ui.painter_at(egui_rect);

        // background
        painter.rect_filled(egui_rect, 0.0, self.grid_view_controls.player_colors[0]);

        if let Some(handle) = &self.grid_render_texture {
            // y-flip via uv
            painter.image(
                handle.id(),
                egui_rect,
                Rect::from_min_max(pos2(0.0, 1.0), pos2(1.0, 0.0)),
                Color32::WHITE,
            );
        }
    }

    fn maybe_update_canvas_texture(&mut self, ui: &mut Ui, rect: GridRect) {
        // For the caching to be effective there needs to space for at least a few
        // framebuffers worth of data.
        const CACHE_FRAMEBUFFERS_WORTH: usize = 16;

        self.grid_view_controls.update_from_canvas_events(ui, rect);

        let framebuffer_size =
            rect.width() as usize * rect.height() as usize * size_of::<Color32>();

        self.grid_renderer
            .lock()
            .unwrap()
            .set_cache_size(framebuffer_size * CACHE_FRAMEBUFFERS_WORTH);

        let curr_grid_render_params = self.grid_view_controls.to_render_params(rect);

        if self.last_grid_render_params != curr_grid_render_params
            || self.grid_view_controls.have_colors_changed
        {
            // Check for changed colors and notify the renderer.
            // NOTE: After generating mipmaps the renderer cannot change colors,it will panic.
            //       The control panel must ensure the controls are disabled.
            if self.grid_view_controls.have_colors_changed {
                self.grid_renderer
                    .lock()
                    .unwrap()
                    .set_colors(self.grid_view_controls.player_colors.as_slice());

                // Do not forget to reset the colors changed flag.
                self.grid_view_controls.have_colors_changed = false;
            }

            self.grid_render_texture = Some(self.grid_renderer.lock().unwrap().render_texture(
                ui.ctx(),
                &curr_grid_render_params.bounds,
                curr_grid_render_params.zoom,
            ));

            // Do not forget to update grid params.
            self.last_grid_render_params = curr_grid_render_params;
        }
    }

    fn show_pointed_chunk_overlay(&mut self, ui: &mut Ui, viewport: GridRect) {
        let pointed_coords = self.grid_view_controls.last_pointed_coords();
        let chunk = self
            .finalized_simulation
            .get_chunk_containing(&pointed_coords);
        if let Some(chunk) = chunk {
            let chunk_bounds = chunk.bounds();
            let proj = GridViewControls::make_projection(
                self.grid_view_controls.zoom_pow2,
                GridPoint::new(self.grid_view_controls.origin_x as i32, self.grid_view_controls.origin_y as i32),
                viewport,
            );
            let chunk_bounds_screen_space = proj.world_to_screen_rect(*chunk_bounds);
            let painter = ui.painter_at(grid_rect_to_egui(viewport));
            painter.rect(grid_rect_to_egui(chunk_bounds_screen_space), 0, Color32::TRANSPARENT, Stroke::new(1.0, Color32::GREEN), StrokeKind::Outside);
        }
    }

    fn show_debug_ui(&mut self, ui: &mut Ui, rect: GridRect) {
        self.show_pointed_chunk_overlay(ui, rect);
    }
}

pub struct GridViewControls {
    finalized_simulation: Arc<FinalizedSimulation>,
    grid_renderer: Arc<Mutex<GridRenderer>>,
    mipmap_generation_progress: Option<MipmapGenerationProgress>,

    min_zoom_pow2: i32,
    max_zoom_pow2: i32,
    player_colors: Vec<Color32>,
    last_pointed_coords: GridPoint,
    save_state: SaveState,

    zoom_pow2: i32,
    zoom_pow2_png: i32,
    png_extent: i32,

    // The origin must be a floating-point number because we require subpixel precision
    // for moving the grid while zoomed-in.
    origin_x: f32,
    origin_y: f32,
    have_colors_changed: bool,
}

fn format_zoom_slider_text(n: f64, _: RangeInclusive<usize>) -> String {
    let n = n.round() as i32;
    if n >= 0 {
        format!("{}x", 1 << n)
    } else {
        format!("1/{}x", 1 << -n)
    }
}

impl GridViewControls {
    pub fn last_pointed_coords(&self) -> GridPoint {
        self.last_pointed_coords
    }

    pub fn zoom_range(&self, grid_renderer: &GridRenderer) -> RangeInclusive<i32> {
        if let Some(factor) = grid_renderer.highest_mipmap_minification_factor() {
            (-(factor.exponent() as i32))..=self.max_zoom_pow2
        } else {
            self.min_zoom_pow2..=self.max_zoom_pow2
        }
    }

    pub fn make_projection(
        zoom_pow2: i32,
        origin_world: GridPoint,
        mut rect: GridRect,
    ) -> ScreenWorldDiscrete2D {
        if zoom_pow2 > 0 {
            // Restrict viewport to bounds compatible with the alignment required by the zoom.
            let factor = Pow2::from_exponent(zoom_pow2 as u8);
            let w = floor_to_multiple(rect.width(), factor);
            let h = floor_to_multiple(rect.height(), factor);
            let dx = mod_floor(rect.width(), factor) / 2;
            let dy = mod_floor(rect.height(), factor) / 2;
            let min = GridPoint::new(rect.start.x + dx, rect.start.y + dy);
            rect = GridRect::with_size(min, w, h);
        }

        ScreenWorldDiscrete2D::new(
            zoom_pow2,
            origin_world,
            rect,
            SCREEN_TO_WORLD_AXIS_FLIP,
        )
    }

    fn render_params(
        zoom_pow2: i32,
        origin_world: GridPoint,
        rect: GridRect,
    ) -> GridRenderParameters {
        let zoom = match zoom_pow2 {
            e @ 0.. => Zoom::Magnification(Pow2::from_exponent(e as u8)),
            e @ ..0 => Zoom::Minification(Pow2::from_exponent((-e) as u8)),
        };
        let proj = Self::make_projection(zoom_pow2, origin_world, rect);
        let bounds = proj.world_rect();
        GridRenderParameters::new(bounds, zoom)
    }

    pub fn to_render_params(&self, viewport: GridRect) -> GridRenderParameters {
        Self::render_params(
            self.zoom_pow2,
            GridPoint::new(self.origin_x as i32, self.origin_y as i32),
            viewport,
        )
    }

    fn update_from_canvas_events(&mut self, ui: &mut Ui, viewport: GridRect) {
        let response = ui.allocate_rect(grid_rect_to_egui(viewport), Sense::drag() | Sense::hover() | Sense::click());

        let get_mouse_pos_in_grid_space = |response: &Response| {
            let middle_pos = (response.rect.max - response.rect.min.to_vec2()) * 0.5f32;
            let mouse = response
                .hover_pos()
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

            let zoom_range = self.zoom_range(&self.grid_renderer.lock().unwrap());
            new_zoom_pow2 = new_zoom_pow2.clamp(*zoom_range.start(), *zoom_range.end());

            let proj = Self::make_projection(self.zoom_pow2, GridPoint::new(self.origin_x as i32, self.origin_y as i32), viewport);
            self.last_pointed_coords = proj.screen_to_world(GridPoint::new(mouse.x as i32, mouse.y as i32));

            if new_zoom_pow2 != self.zoom_pow2 {
                let proj_new = Self::make_projection(new_zoom_pow2, GridPoint::new(self.origin_x as i32, self.origin_y as i32), viewport);

                let mouse_world = proj.screen_to_world(GridPoint::new(mouse.x as i32, mouse.y as i32));
                let mouse_world_new = proj_new.screen_to_world(GridPoint::new(mouse.x as i32, mouse.y as i32));
                let diff = mouse_world - mouse_world_new;
                self.origin_x += diff.x as f32;
                self.origin_y += diff.y as f32;
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

        let complete_shells = self.finalized_simulation.complete_shells();

        // Set origin to current pointer placement scaled to the size of the whole grid.
        // Allows going to any region on the grid, useful for large grids.
        if response.clicked_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Secondary)
        {
            let (_mouse, mouse_relative_to_center) = get_mouse_pos_in_grid_space(&response);

            let tx = mouse_relative_to_center.x / viewport.width() as f32 * 2.0;
            let ty = mouse_relative_to_center.y / viewport.height() as f32 * 2.0;

            self.origin_x = tx * complete_shells as f32;
            self.origin_y = ty * complete_shells as f32;
        }

        let bounds = complete_shells as f32;
        self.origin_x = self.origin_x.clamp(-bounds, bounds);
        self.origin_y = self.origin_y.clamp(-bounds, bounds);
    }

    pub fn is_saved(&self) -> bool {
        matches!(self.save_state, SaveState::Saved)
    }

    fn assume_saved(&mut self) {
        self.save_state = SaveState::Saved;
    }

    fn try_save(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("simulation.uls")
            .save_file()
        {
            let mut writer = BufWriter::new(File::create(path).unwrap());
            if let Err(e) = self.finalized_simulation.write_to(&mut writer) {
                eprintln!("Failed to save simulation: {}", e);
                self.save_state = SaveState::Errored(e);
            } else {
                self.save_state = SaveState::Saved;
            }
        }
    }

    fn show_save_ui(&mut self, ui: &mut Ui) {
        match &self.save_state {
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

        let save = ui.button("Save simulation").clicked()
            || ui.input_mut(|i| i.consume_shortcut(&SAVE_SHORTCUT));
        if save {
            self.try_save();
        }
    }

    fn show_mipmaps_ui(&mut self, ui: &mut Ui) {
        let mut grid_renderer_mutex_guard = self.grid_renderer.lock().unwrap();

        // Handle various stages of mipmap generation.
        // We rely directly on the state reported by the grid renderer instead of the
        // progress from the callback.
        if grid_renderer_mutex_guard.has_mipmaps() {
            ui.label("Mipmaps are generated.");
        } else if grid_renderer_mutex_guard.can_generate_mipmaps() {
            let lowest_minification = Pow2::from_exponent((-MIN_ZOOM_POW2 + 1) as u8);
            let highest_minification = Pow2::from_exponent((-MIN_ZOOM_POW2_MIPS) as u8);
            let estimated_mipmaps_memory_requirement = grid_renderer_mutex_guard
                .estimate_mipmaps_memory_requirement(lowest_minification, highest_minification);
            let on_hover_text = if estimated_mipmaps_memory_requirement
                >= MIN_MIPMAP_MEMORY_REQUIREMENT_TO_SHOW_WARNING
            {
                format!(
                    "WARNING: While this will enable up to {}x minification \
                it does require roughly {} of RAM and may take a long time.\
                This process is asynchronous.",
                    highest_minification,
                    estimated_mipmaps_memory_requirement.display().si(),
                )
            } else {
                format!(
                    "This will enable up to {}x minification.",
                    highest_minification
                )
            };

            if ui
                .button("Generate mipmaps")
                .on_hover_text(on_hover_text)
                .clicked()
            {
                self.mipmap_generation_progress = Some(
                    grid_renderer_mutex_guard
                        .generate_mipmaps_async(lowest_minification, highest_minification),
                );
            }
        } else
        /* if mipmap generation in progress */
        {
            if ui.button("Cancel mipmap generation.").clicked() {
                grid_renderer_mutex_guard.cancel_mipmap_generation();
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

    fn show_zoom_origin_ui(&mut self, ui: &mut Ui) {
        let grid_renderer_mutex_guard = self.grid_renderer.lock().unwrap();
        let complete_shells = self.finalized_simulation.complete_shells();
        let zoom_range = self.zoom_range(&grid_renderer_mutex_guard);

        ui.add(
            egui::Slider::new(&mut self.zoom_pow2, zoom_range.clone())
                .text("Zoom")
                .custom_formatter(format_zoom_slider_text),
        );

        // How many per pixel.
        let coord_drag_speed = (complete_shells / 200) as f64;
        ui.add(
            egui::Slider::new(
                &mut self.origin_x,
                -(complete_shells as f32)..=(complete_shells as f32),
            )
            .text("X")
            .drag_value_speed(coord_drag_speed),
        );
        ui.add(
            egui::Slider::new(
                &mut self.origin_y,
                -(complete_shells as f32)..=(complete_shells as f32),
            )
            .text("Y")
            .drag_value_speed(coord_drag_speed),
        );
    }

    fn show_player_colors_ui(&mut self, ui: &mut Ui) {
        let grid_renderer_mutex_guard = self.grid_renderer.lock().unwrap();
        let player_count = self.finalized_simulation.player_count();

        // TODO: Columns for some reason take more space than necessary.
        //       This `set_max_width` is a hack to make it about as much as it should.
        ui.set_max_width(200.0);
        ui.columns(2, |columns| {
            for player_id in 0..=player_count {
                let column = &mut columns[player_id % 2];
                column.horizontal_wrapped(|ui| {
                    // Disallow color picking after mipmaps have been generated
                    if !grid_renderer_mutex_guard.can_set_colors() {
                        color_picker::show_color(
                            ui,
                            self.player_colors[player_id],
                            vec2(16.0, 16.0),
                        );
                    } else {
                        if color_picker::color_edit_button_srgba(
                            ui,
                            &mut self.player_colors[player_id],
                            Alpha::Opaque,
                        )
                        .changed()
                        {
                            self.have_colors_changed = true;
                        }
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
        ui.label(format!(
            "Pointer: {}, {}",
            self.last_pointed_coords.x, self.last_pointed_coords.y
        ));
    }

    fn show_screenshots_ui(&mut self, ui: &mut Ui) {
        let grid_renderer_mutex_guard = self.grid_renderer.lock().unwrap();
        let zoom_range = self.zoom_range(&grid_renderer_mutex_guard);

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
            let render_params = Self::render_params(
                self.zoom_pow2_png,
                GridPoint::new(self.origin_x as i32, self.origin_y as i32),
                GridRect::with_size(GridPoint::zero(), s, s),
            );
            let image = grid_renderer_mutex_guard
                .render_to_rgba_image(&render_params.bounds, render_params.zoom);

            let file = File::create(path).unwrap();
            let w = BufWriter::new(file);
            let mut encoder = png::Encoder::new(w, s as u32, s as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);

            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(image.as_raw()).unwrap();
        }
    }

    fn ui(&mut self, ui: &mut Ui) {
        ui.heading("Info");

        self.show_info_ui(ui);
        self.show_save_ui(ui);

        ui.heading("Controls");

        self.show_mipmaps_ui(ui);
        self.show_zoom_origin_ui(ui);
        self.show_player_colors_ui(ui);

        ui.heading("Screenshots ❓")
            .on_hover_text("Currently it only provides a way to save small PNG images.\n\
            Chunked [big]TIFF support for large images, separately configurable, is a future feature.");

        self.show_screenshots_ui(ui);
    }
}

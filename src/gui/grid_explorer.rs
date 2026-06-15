use crate::gui::conv::{egui_to_grid_point, egui_to_grid_rect, grid_rect_to_egui};
use crate::gui::render::grid_render::{
    default_player_colors, GridRenderer, MipmapGenerationProgress,
};
use crate::gui::subwindow::SubwindowResult::Keep;
use crate::gui::subwindow::{Subwindow, SubwindowResult};
use crate::gui::widgets::misc::srgb_color_button;
use eframe::egui;
use eframe::egui::{
    Context, Key, KeyboardShortcut, Modifiers, Painter, Rect, Sense, Stroke, StrokeKind,
    TextureHandle, Ui,
};
use eframe::emath::pos2;
use eframe::epaint::Color32;
use std::fs::File;
use std::io::BufWriter;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use ulam_leapers::game::chunk::BoundedChunk;
use ulam_leapers::game::simulation::{FinalizedSimulation, Game};
use ulam_leapers::io::{ReadFrom, WriteTo};
use ulam_leapers::math::coords::{GridPoint, Point2D};
use ulam_leapers::math::pow2::Pow2;
use ulam_leapers::math::projection::{FlipAxis, ScreenWorldDiscrete2D};
use ulam_leapers::math::rect::GridRect;
use ulam_leapers::math::zoom::Zoom;
use ulam_leapers::util::memory::MemSize;

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

#[derive(Debug)]
pub enum SaveState {
    NotSaved,
    Saved,
    Errored(std::io::Error),
}

#[derive(Debug, Clone, Copy)]
struct Camera {
    zoom_pow2: i32,
    position: Point2D<f32>,
}

impl Camera {
    pub fn new(zoom_pow2: i32, position: Point2D<f32>) -> Self {
        Self { zoom_pow2, position }
    }

    pub fn with_zoom(&self, zoom_pow2: i32) -> Self {
        Self { zoom_pow2, position: self.position }
    }
}

struct Canvas {
    camera: Camera,
    viewport: GridRect,
    projection: ScreenWorldDiscrete2D,
}

impl Canvas {
    pub fn new(camera: Camera, viewport: GridRect) -> Self {
        let rect = if camera.zoom_pow2 > 0 {
            // Restrict viewport to bounds compatible with the alignment required by the zoom.
            let factor = Pow2::from_exponent(camera.zoom_pow2 as u8);
            viewport.aligned_to_pow2_inside(factor)
        } else {
            viewport
        };

        Self {
            projection: ScreenWorldDiscrete2D::new(
                camera.zoom_pow2,
                GridPoint::new(camera.position.x as i32, camera.position.y as i32),
                rect,
                SCREEN_TO_WORLD_AXIS_FLIP,
            ),
            camera,
            viewport,
        }
    }

    pub fn make_painter(&self, ui: &mut Ui) -> Painter {
        ui.painter_at(grid_rect_to_egui(self.rect()))
    }

    pub fn with_camera(&self, camera: Camera) -> Self {
        Self::new(camera, self.viewport)
    }

    pub fn with_zoom(&self, zoom_pow2: i32) -> Self {
        Self::new(self.camera.with_zoom(zoom_pow2), self.viewport)
    }

    pub fn to_render_params(&self) -> GridRenderParameters {
        GridRenderParameters::new(self.world_rect(), self.zoom())
    }

    pub fn zoom(&self) -> Zoom<Pow2> {
        self.projection.zoom()
    }

    pub fn world_rect(&self) -> GridRect {
        self.projection.world_rect()
    }

    pub fn rect(&self) -> GridRect {
        self.projection.screen_rect()
    }

    pub fn width(&self) -> i32 {
        self.projection.screen_rect().width()
    }

    pub fn height(&self) -> i32 {
        self.projection.screen_rect().height()
    }

    pub fn screen_to_world(&self, screen_point: GridPoint) -> GridPoint {
        self.projection.screen_to_world(screen_point)
    }

    pub fn world_to_screen(&self, world_point: GridPoint) -> GridPoint {
        self.projection.world_to_screen(world_point)
    }

    pub fn world_to_screen_rect(&self, world_rect: GridRect) -> GridRect {
        self.projection.world_to_screen_rect(world_rect)
    }
}

pub struct GridExplorer {
    finalized_simulation: FinalizedSimulation,
    grid_renderer: GridRenderer,
    grid_render_texture: Option<TextureHandle>,
    last_grid_render_params: GridRenderParameters,
    is_debug_ui_enabled: bool,

    mipmap_generation_progress: Option<MipmapGenerationProgress>,

    min_zoom_pow2: i32,
    max_zoom_pow2: i32,
    player_colors: Vec<Color32>,
    last_pointed_coords: GridPoint,
    save_state: SaveState,

    zoom_pow2_png: i32,
    png_extent: i32,

    camera: Camera,
    have_colors_changed: bool,
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

            // The projection will give us a more restricted viewport.
            let mut canvas = self.make_canvas(egui_to_grid_rect(ui.clip_rect()));

            if canvas.width() >= MIN_CANVAS_WIDTH && canvas.height() >= MIN_CANVAS_HEIGHT {
                self.update_canvas_from_events(ui, &mut canvas);
                self.maybe_update_canvas_texture(ui, &canvas);
                self.draw_canvas_texture(ui, &canvas);
            }

            if ui.input_mut(|i| i.consume_shortcut(&DEBUG_UI_TOGGLE_SHORTCUT)) {
                self.is_debug_ui_enabled = !self.is_debug_ui_enabled;
            }
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
        let grid_renderer =
            GridRenderer::new(&finalized_simulation, default_player_colors().as_slice());

        let player_count = finalized_simulation.player_count();

        Self {
            grid_renderer,
            finalized_simulation,
            grid_render_texture: None,
            last_grid_render_params: Default::default(),
            is_debug_ui_enabled: false,

            mipmap_generation_progress: None,

            min_zoom_pow2: MIN_ZOOM_POW2,
            max_zoom_pow2: MAX_ZOOM_POW2,
            player_colors: default_player_colors()[..=player_count].to_vec(),
            last_pointed_coords: GridPoint::new(0, 0),
            save_state: SaveState::NotSaved,

            zoom_pow2_png: DEFAULT_ZOOM_POW2,
            png_extent: DEFAULT_PNG_EXTENT,

            camera: Camera::new(DEFAULT_ZOOM_POW2, Point2D::new(0.0, 0.0)),
            have_colors_changed: false,
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

    fn draw_canvas_texture(&mut self, ui: &mut Ui, canvas: &Canvas) {
        let painter = canvas.make_painter(ui);
        let rect = painter.clip_rect();

        // background
        painter.rect_filled(rect, 0.0, self.player_colors[0]);

        if let Some(handle) = &self.grid_render_texture {
            // y-flip via uv
            painter.image(
                handle.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 1.0), pos2(1.0, 0.0)),
                Color32::WHITE,
            );
        }
    }

    fn maybe_update_canvas_texture(&mut self, ui: &mut Ui, canvas: &Canvas) {
        // For the caching to be effective there needs to space for at least a few
        // framebuffers worth of data.
        const CACHE_FRAMEBUFFERS_WORTH: usize = 16;

        let framebuffer_size =
            canvas.width() as usize * canvas.height() as usize * size_of::<Color32>();

        self.grid_renderer
            .set_cache_size(framebuffer_size * CACHE_FRAMEBUFFERS_WORTH);

        let curr_grid_render_params = canvas.to_render_params();

        if self.last_grid_render_params != curr_grid_render_params || self.have_colors_changed {
            // Check for changed colors and notify the renderer.
            // NOTE: After generating mipmaps the renderer cannot change colors,it will panic.
            //       The control panel must ensure the controls are disabled.
            if self.have_colors_changed {
                self.grid_renderer.set_colors(self.player_colors.as_slice());

                // Do not forget to reset the colors changed flag.
                self.have_colors_changed = false;
            }

            self.grid_render_texture = Some(self.grid_renderer.render_texture(
                ui.ctx(),
                &curr_grid_render_params.bounds,
                curr_grid_render_params.zoom,
            ));

            // Do not forget to update grid params.
            self.last_grid_render_params = curr_grid_render_params;
        }
    }

    fn show_pointed_chunk_overlay(&mut self, ui: &mut Ui, canvas: &Canvas) {
        let pointed_coords = self.last_pointed_coords();
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
                Stroke::new(1.0, Color32::GREEN),
                StrokeKind::Outside,
            );
        }
    }

    fn show_debug_ui(&mut self, ui: &mut Ui, canvas: &Canvas) {
        self.show_pointed_chunk_overlay(ui, canvas);
    }
}

fn format_zoom_slider_text(n: f64, _: RangeInclusive<usize>) -> String {
    let n = n.round() as i32;
    if n >= 0 {
        format!("{}x", 1 << n)
    } else {
        format!("1/{}x", 1 << -n)
    }
}

impl GridExplorer {
    pub fn last_pointed_coords(&self) -> GridPoint {
        self.last_pointed_coords
    }

    pub fn zoom_range(&self) -> RangeInclusive<i32> {
        if let Some(factor) = self.grid_renderer.highest_mipmap_minification_factor() {
            (-(factor.exponent() as i32))..=self.max_zoom_pow2
        } else {
            self.min_zoom_pow2..=self.max_zoom_pow2
        }
    }

    fn make_canvas(&self, rect: GridRect) -> Canvas {
        Canvas::new(self.camera, rect)
    }

    fn update_canvas_from_events(&mut self, ui: &mut Ui, canvas: &mut Canvas) {
        let response = ui.allocate_rect(
            grid_rect_to_egui(canvas.rect()),
            Sense::drag() | Sense::hover() | Sense::click(),
        );

        let mouse_pos = egui_to_grid_point(
            response
                .hover_pos()
                .unwrap_or_else(|| (response.rect.max - response.rect.min.to_vec2()) * 0.5f32),
        );

        let mouse_world = canvas.screen_to_world(mouse_pos);
        self.last_pointed_coords = mouse_world;

        let mut new_origin_x = self.camera.position.x;
        let mut new_origin_y = self.camera.position.y;
        let mut new_zoom_pow2 = self.camera.zoom_pow2;

        if response.hovered() {
            ui.input(|i| {
                for event in &i.events {
                    if let egui::Event::MouseWheel { delta, .. } = event {
                        new_zoom_pow2 += delta.y as i32;
                    }
                }
            });

            let zoom_range = self.zoom_range();
            new_zoom_pow2 = new_zoom_pow2.clamp(*zoom_range.start(), *zoom_range.end());
        }

        // Drag keeping origin at the pointer.
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            let zoom_scale = 0.5f32.powf(new_zoom_pow2 as f32);
            new_origin_x -= zoom_scale * delta.x;
            new_origin_y += zoom_scale * delta.y;
        }

        let complete_shells = self.finalized_simulation.complete_shells();
        let complete_shells_f32 = complete_shells as f32;

        // Set origin to current pointer placement scaled to the size of the whole grid.
        // Allows going to any region on the grid, useful for large grids.
        if response.clicked_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Secondary)
        {
            let tx = mouse_pos.x as f32 / canvas.width() as f32;
            let ty = 1.0 - mouse_pos.y as f32 / canvas.height() as f32;

            new_origin_x = -complete_shells_f32 + tx * complete_shells_f32 * 2.0;
            new_origin_y = -complete_shells_f32 + ty * complete_shells_f32 * 2.0;
        }

        if new_zoom_pow2 != self.camera.zoom_pow2 {
            let canvas_new = canvas.with_zoom(new_zoom_pow2);

            let mouse_world_new = canvas_new.screen_to_world(mouse_pos);
            let diff = mouse_world - mouse_world_new;
            new_origin_x += diff.x as f32;
            new_origin_y += diff.y as f32;
        }

        new_origin_x = new_origin_x.clamp(-complete_shells_f32, complete_shells_f32);
        new_origin_y = new_origin_y.clamp(-complete_shells_f32, complete_shells_f32);

        // Update canvas if anything changed.
        if new_origin_x != self.camera.position.x
            || new_origin_y != self.camera.position.y
            || new_zoom_pow2 != self.camera.zoom_pow2
        {
            self.camera.position.x = new_origin_x;
            self.camera.position.y = new_origin_y;
            self.camera.zoom_pow2 = new_zoom_pow2;

            *canvas = canvas.with_camera(
                self.camera
            );
        }
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
        // Handle various stages of mipmap generation.
        // We rely directly on the state reported by the grid renderer instead of the
        // progress from the callback.
        if self.grid_renderer.has_mipmaps() {
            ui.label("Mipmaps are generated.");
        } else if self.grid_renderer.can_generate_mipmaps() {
            let lowest_minification = Pow2::from_exponent((-MIN_ZOOM_POW2 + 1) as u8);
            let highest_minification = Pow2::from_exponent((-MIN_ZOOM_POW2_MIPS) as u8);
            let estimated_mipmaps_memory_requirement = self
                .grid_renderer
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
                    self.grid_renderer
                        .generate_mipmaps_async(lowest_minification, highest_minification),
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

    fn show_zoom_origin_ui(&mut self, ui: &mut Ui) {
        let complete_shells = self.finalized_simulation.complete_shells();
        let zoom_range = self.zoom_range();

        ui.add(
            egui::Slider::new(&mut self.camera.zoom_pow2, zoom_range.clone())
                .text("Zoom")
                .custom_formatter(format_zoom_slider_text),
        );

        // How many per pixel.
        let coord_drag_speed = (complete_shells / 200) as f64;
        ui.add(
            egui::Slider::new(
                &mut self.camera.position.x,
                -(complete_shells as f32)..=(complete_shells as f32),
            )
            .text("X")
            .drag_value_speed(coord_drag_speed),
        );
        ui.add(
            egui::Slider::new(
                &mut self.camera.position.y,
                -(complete_shells as f32)..=(complete_shells as f32),
            )
            .text("Y")
            .drag_value_speed(coord_drag_speed),
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
                        self.have_colors_changed = true;
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
            let render_params = Canvas::new(
                self.camera.with_zoom(self.zoom_pow2_png),
                GridRect::with_size(GridPoint::zero(), s, s),
            )
            .to_render_params();
            let image = self
                .grid_renderer
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

    fn show_controls_window_ui(&mut self, ui: &mut Ui) {
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

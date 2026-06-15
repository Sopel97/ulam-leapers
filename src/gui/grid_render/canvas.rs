use crate::gui::conv::{egui_to_grid_rect, grid_rect_to_egui};
use crate::gui::grid_explorer::GridRenderParameters;
use eframe::egui::{Painter, Ui};
use std::ops::RangeInclusive;
use ulam_leapers::math::coords::{GridPoint, Point2D, Vector2D};
use ulam_leapers::math::pow2::Pow2;
use crate::gui::grid_render::projection::{FlipAxis, GridProjection};
use ulam_leapers::math::rect::{GridRect, Rect2D};
use ulam_leapers::math::zoom::Zoom;

const SCREEN_TO_WORLD_AXIS_FLIP: FlipAxis = FlipAxis::Y;

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct GridCamera {
    pub zoom_pow2: i32,
    pub position: Point2D<f32>,
}

impl GridCamera {
    pub fn new(zoom_pow2: i32, position: Point2D<f32>) -> Self {
        Self {
            zoom_pow2,
            position,
        }
    }

    pub fn with_zoom(&self, zoom_pow2: i32) -> Self {
        Self {
            zoom_pow2,
            position: self.position,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RestrictedGridCamera {
    camera: GridCamera,
    zoom_pow2_range: RangeInclusive<i32>,
    position_range: Rect2D<f32>,
}

impl RestrictedGridCamera {
    pub fn from_camera(
        camera: GridCamera,
        zoom_pow2_range: RangeInclusive<i32>,
        position_range: Rect2D<f32>,
    ) -> Self {
        Self {
            camera: GridCamera::new(
                Self::clamped_zoom(camera.zoom_pow2, zoom_pow2_range.clone()),
                Self::clamped_position(camera.position, position_range),
            ),
            zoom_pow2_range,
            position_range,
        }
    }

    pub fn to_camera(&self) -> GridCamera {
        self.camera
    }

    pub fn add_zoom_with_invariant_point(
        &mut self,
        canvas: &GridCanvas,
        zoom_delta: i32,
        invariant_point: GridPoint,
    ) {
        let new_zoom_pow2 = Self::clamped_zoom(
            self.camera.zoom_pow2 + zoom_delta,
            self.zoom_pow2_range.clone(),
        );
        let canvas_new = canvas.with_zoom(new_zoom_pow2);

        let invariant_world = canvas.screen_to_world(invariant_point);
        let invariant_world_new = canvas_new.screen_to_world(invariant_point);
        let diff = invariant_world - invariant_world_new;

        self.camera.position = Self::clamped_position(
            Point2D::new(
                self.camera.position.x + diff.x as f32,
                self.camera.position.y + diff.y as f32,
            ),
            self.position_range,
        );
        self.camera.zoom_pow2 = new_zoom_pow2;
    }

    pub fn drag(&mut self, dx: f32, dy: f32) {
        let zoom_scale = 0.5f32.powf(self.camera.zoom_pow2 as f32);
        self.camera.position = Self::clamped_position(
            self.camera.position + Vector2D::new(dx, dy) * zoom_scale,
            self.position_range,
        );
    }

    pub fn set_position_proportional_within_bounds(&mut self, tx: f32, ty: f32) {
        self.camera.position = Self::clamped_position(
            Point2D::new(
                self.position_range.start.x + tx * self.position_range.width(),
                self.position_range.start.y + ty * self.position_range.height(),
            ),
            self.position_range,
        );
    }

    fn clamped_zoom(zoom_pow2: i32, zoom_pow2_range: RangeInclusive<i32>) -> i32 {
        zoom_pow2.clamp(*zoom_pow2_range.start(), *zoom_pow2_range.end())
    }

    fn clamped_position(position: Point2D<f32>, position_range: Rect2D<f32>) -> Point2D<f32> {
        Point2D::new(
            position
                .x
                .clamp(position_range.start.x, position_range.end.x),
            position
                .y
                .clamp(position_range.start.y, position_range.end.y),
        )
    }
}

pub struct GridCanvas {
    camera: GridCamera,
    viewport: GridRect,
    projection: GridProjection,
}

impl GridCanvas {
    pub fn in_ui(ui: &Ui, camera: GridCamera) -> Self {
        let viewport = egui_to_grid_rect(ui.clip_rect());
        Self::new(camera, viewport)
    }

    pub fn new(camera: GridCamera, viewport: GridRect) -> Self {
        let rect = if camera.zoom_pow2 > 0 {
            // Restrict viewport to bounds compatible with the alignment required by the zoom.
            let factor = Pow2::from_exponent(camera.zoom_pow2 as u8);
            viewport.aligned_to_pow2_inside(factor)
        } else {
            viewport
        };

        Self {
            projection: GridProjection::new(
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

    pub fn with_camera(&self, camera: GridCamera) -> Self {
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

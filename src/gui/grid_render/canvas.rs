use crate::gui::conv::{egui_to_grid_rect, grid_rect_to_egui};
use crate::gui::grid_render::projection::{FlipAxis, GridProjection};
use eframe::egui::{Painter, Response, Sense, Ui};
use std::ops::RangeInclusive;
use ulam_leapers::math::coords::{GridPoint, Point2D, Vector2D};
use ulam_leapers::math::pow2::Pow2;
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

    pub fn zoom(&self) -> Zoom<Pow2> {
        Zoom::from_exponent(self.zoom_pow2)
    }

    pub fn restricted(
        &self,
        zoom_pow2_range: RangeInclusive<i32>,
        position_range: Rect2D<f32>,
    ) -> RestrictedGridCamera {
        RestrictedGridCamera::from_camera(*self, zoom_pow2_range, position_range)
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
        let rect = match camera.zoom() {
            Zoom::Magnification(factor) => viewport.aligned_to_pow2_inside(factor),
            Zoom::Minification(_) => viewport,
        };

        Self {
            projection: GridProjection::new(
                camera.zoom(),
                GridPoint::new(camera.position.x as i32, camera.position.y as i32),
                rect,
                SCREEN_TO_WORLD_AXIS_FLIP,
            ),
            camera,
            viewport,
        }
    }

    pub fn is_zero_area(&self) -> bool {
        self.width() == 0 || self.height() == 0
    }

    pub fn make_sense(&self, ui: &mut Ui, sense: Sense) -> Response {
        ui.allocate_rect(grid_rect_to_egui(self.rect()), sense)
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

#[cfg(test)]
mod grid_camera_tests {
    use super::*;
    use ulam_leapers::math::coords::Point2D;
    use ulam_leapers::math::zoom::Zoom;

    fn cam(zoom_pow2: i32) -> GridCamera {
        GridCamera::new(zoom_pow2, Point2D::new(0.0, 0.0))
    }

    fn cam_at(zoom_pow2: i32, x: f32, y: f32) -> GridCamera {
        GridCamera::new(zoom_pow2, Point2D::new(x, y))
    }

    #[test]
    fn new_stores_fields() {
        let c = cam_at(3, 10.0, -5.0);
        assert_eq!(c.zoom_pow2, 3);
        assert_eq!(c.position.x, 10.0);
        assert_eq!(c.position.y, -5.0);
    }

    #[test]
    fn with_zoom_changes_only_zoom() {
        let original = cam_at(2, 7.0, 3.0);
        let zoomed = original.with_zoom(5);
        assert_eq!(zoomed.zoom_pow2, 5);
        assert_eq!(zoomed.position.x, original.position.x);
        assert_eq!(zoomed.position.y, original.position.y);
    }

    #[test]
    fn with_zoom_zero() {
        let c = cam(4).with_zoom(0);
        assert_eq!(c.zoom_pow2, 0);
    }

    #[test]
    fn with_zoom_negative() {
        let c = cam(1).with_zoom(-3);
        assert_eq!(c.zoom_pow2, -3);
    }

    #[test]
    fn zoom_positive_exponent_is_magnification() {
        let c = cam(2);
        assert!(matches!(c.zoom(), Zoom::Magnification(_)));
    }

    #[test]
    fn zoom_negative_exponent_is_minification() {
        let c = cam(-2);
        assert!(matches!(c.zoom(), Zoom::Minification(_)));
    }

    #[test]
    fn zoom_zero_exponent() {
        let c = cam(0);
        assert!(matches!(c.zoom(), Zoom::Magnification(_)));
    }

    #[test]
    fn equality_same_values() {
        let a = cam_at(1, 2.0, 3.0);
        let b = cam_at(1, 2.0, 3.0);
        assert_eq!(a, b);
    }

    #[test]
    fn inequality_different_zoom() {
        assert_ne!(cam_at(1, 0.0, 0.0), cam_at(2, 0.0, 0.0));
    }

    #[test]
    fn inequality_different_position() {
        assert_ne!(cam_at(1, 0.0, 0.0), cam_at(1, 1.0, 0.0));
    }

    #[test]
    #[allow(unused)]
    fn copy_is_independent() {
        let original = cam_at(3, 5.0, 6.0);
        let mut copy = original;
        copy.zoom_pow2 = 99;
        assert_eq!(original.zoom_pow2, 3); // original unchanged
    }
}

#[cfg(test)]
mod restricted_grid_camera_tests {
    use super::*;
    use ulam_leapers::math::coords::Point2D;
    use ulam_leapers::math::rect::Rect2D;

    fn bounds() -> Rect2D<f32> {
        Rect2D {
            start: Point2D::new(-100.0, -100.0),
            end: Point2D::new(100.0, 100.0),
        }
    }

    fn small_bounds() -> Rect2D<f32> {
        Rect2D {
            start: Point2D::new(0.0, 0.0),
            end: Point2D::new(10.0, 10.0),
        }
    }

    fn restricted(zoom_pow2: i32, x: f32, y: f32) -> RestrictedGridCamera {
        GridCamera::new(zoom_pow2, Point2D::new(x, y)).restricted(-4..=4, bounds())
    }

    #[test]
    fn zoom_clamped_to_max() {
        let rc = restricted(10, 0.0, 0.0);
        assert_eq!(rc.to_camera().zoom_pow2, 4);
    }

    #[test]
    fn zoom_clamped_to_min() {
        let rc = restricted(-10, 0.0, 0.0);
        assert_eq!(rc.to_camera().zoom_pow2, -4);
    }

    #[test]
    fn zoom_within_range_unchanged() {
        let rc = restricted(2, 0.0, 0.0);
        assert_eq!(rc.to_camera().zoom_pow2, 2);
    }

    #[test]
    fn position_clamped_to_bounds() {
        let rc = restricted(0, 9999.0, -9999.0);
        let cam = rc.to_camera();
        assert_eq!(cam.position.x, 100.0);
        assert_eq!(cam.position.y, -100.0);
    }

    #[test]
    fn position_within_bounds_unchanged() {
        let rc = restricted(0, 50.0, -50.0);
        let cam = rc.to_camera();
        assert_eq!(cam.position.x, 50.0);
        assert_eq!(cam.position.y, -50.0);
    }

    #[test]
    fn drag_moves_camera() {
        let mut rc = restricted(0, 0.0, 0.0);
        let before = rc.to_camera().position;
        rc.drag(10.0, 5.0);
        let after = rc.to_camera().position;
        // At zoom_pow2 == 0, zoom_scale == 1.0 so delta is applied directly.
        assert!((after.x - before.x - 10.0).abs() < 1e-4);
        assert!((after.y - before.y - 5.0).abs() < 1e-4);
    }

    #[test]
    fn drag_is_scaled_by_zoom() {
        // At zoom_pow2 == 2, scale == 0.5^2 == 0.25.
        let mut rc = restricted(2, 0.0, 0.0);
        rc.drag(100.0, 0.0);
        let pos = rc.to_camera().position;
        assert!((pos.x - 25.0).abs() < 1e-4);
    }

    #[test]
    fn drag_clamps_to_position_bounds() {
        let mut rc = restricted(0, 90.0, 0.0);
        rc.drag(1000.0, 0.0);
        assert_eq!(rc.to_camera().position.x, 100.0);
    }

    #[test]
    fn drag_negative_direction() {
        let mut rc = restricted(0, 0.0, 0.0);
        rc.drag(-30.0, -20.0);
        let pos = rc.to_camera().position;
        assert!((pos.x + 30.0).abs() < 1e-4);
        assert!((pos.y + 20.0).abs() < 1e-4);
    }

    #[test]
    fn proportional_position_zero_zero_is_start() {
        let mut rc = GridCamera::new(0, Point2D::new(0.0, 0.0)).restricted(-4..=4, small_bounds());
        rc.set_position_proportional_within_bounds(0.0, 0.0);
        let pos = rc.to_camera().position;
        assert!((pos.x - 0.0).abs() < 1e-5);
        assert!((pos.y - 0.0).abs() < 1e-5);
    }

    #[test]
    fn proportional_position_one_one_is_end() {
        let mut rc = GridCamera::new(0, Point2D::new(0.0, 0.0)).restricted(-4..=4, small_bounds());
        rc.set_position_proportional_within_bounds(1.0, 1.0);
        let pos = rc.to_camera().position;
        assert!((pos.x - 10.0).abs() < 1e-5);
        assert!((pos.y - 10.0).abs() < 1e-5);
    }

    #[test]
    fn proportional_position_half_is_midpoint() {
        let mut rc = GridCamera::new(0, Point2D::new(0.0, 0.0)).restricted(-4..=4, small_bounds());
        rc.set_position_proportional_within_bounds(0.5, 0.5);
        let pos = rc.to_camera().position;
        assert!((pos.x - 5.0).abs() < 1e-5);
        assert!((pos.y - 5.0).abs() < 1e-5);
    }

    #[test]
    fn proportional_position_out_of_range_is_clamped() {
        let mut rc = GridCamera::new(0, Point2D::new(0.0, 0.0)).restricted(-4..=4, small_bounds());
        // t > 1.0 should clamp to the end of the range.
        rc.set_position_proportional_within_bounds(2.0, 2.0);
        let pos = rc.to_camera().position;
        assert_eq!(pos.x, 10.0);
        assert_eq!(pos.y, 10.0);
    }

    #[test]
    fn to_camera_reflects_current_state() {
        let mut rc = restricted(0, 0.0, 0.0);
        rc.drag(5.0, -3.0);
        let cam = rc.to_camera();
        assert!((cam.position.x - 5.0).abs() < 1e-4);
        assert!((cam.position.y + 3.0).abs() < 1e-4);
    }
}

#[cfg(test)]
mod grid_canvas_tests {
    use super::*;
    use ulam_leapers::math::coords::{GridPoint, Point2D};
    use ulam_leapers::math::rect::GridRect;

    fn viewport_400x300() -> GridRect {
        GridRect::with_start_end(GridPoint::new(0, 0), GridPoint::new(400, 300))
    }

    fn canvas(zoom_pow2: i32) -> GridCanvas {
        let camera = GridCamera::new(zoom_pow2, Point2D::new(0.0, 0.0));
        GridCanvas::new(camera, viewport_400x300())
    }

    #[test]
    fn canvas_is_not_zero_area_for_normal_viewport() {
        assert!(!canvas(0).is_zero_area());
    }

    #[test]
    fn canvas_with_zero_width_is_zero_area() {
        let camera = GridCamera::new(0, Point2D::new(0.0, 0.0));
        let zero_viewport = GridRect::with_start_end(GridPoint::new(0, 0), GridPoint::new(0, 300));
        assert!(GridCanvas::new(camera, zero_viewport).is_zero_area());
    }

    #[test]
    fn canvas_with_zero_height_is_zero_area() {
        let camera = GridCamera::new(0, Point2D::new(0.0, 0.0));
        let zero_viewport = GridRect::with_start_end(GridPoint::new(0, 0), GridPoint::new(400, 0));
        assert!(GridCanvas::new(camera, zero_viewport).is_zero_area());
    }

    #[test]
    fn with_zoom_changes_zoom_level() {
        let c = canvas(-3).with_zoom(3);
        assert_eq!(c.zoom(), Zoom::Magnification(Pow2::from_exponent(3)));
    }

    #[test]
    fn with_camera_preserves_viewport_dimensions() {
        let original = canvas(0);
        let new_cam = GridCamera::new(2, Point2D::new(10.0, 10.0));
        let updated = original.with_camera(new_cam);
        // Viewport shouldn't have changed.
        assert_eq!(original.viewport, updated.viewport);

        // The screen rect may be smaller now but shouldn't be zero.
        assert!(!updated.is_zero_area());
        assert!(original.rect().contains(&updated.rect()));
    }

    #[test]
    fn screen_to_world_and_back_at_zoom0() {
        let c = canvas(0);
        let screen_pt = GridPoint::new(50, 80);
        let world_pt = c.screen_to_world(screen_pt);
        let back = c.world_to_screen(world_pt);
        assert_eq!(back, screen_pt);
    }

    #[test]
    fn world_rect_to_screen_rect_round_trip_at_zoom0() {
        let c = canvas(0);
        // Pick a world rect we know maps cleanly.
        let world_rect = c.world_rect();
        let screen_rect = c.world_to_screen_rect(world_rect);
        // The screen rect should cover the canvas screen rect.
        assert_eq!(screen_rect, c.rect());
    }

    #[test]
    fn rect_area_matches_viewport_at_no_zoom() {
        let c = canvas(0);
        assert_eq!(c.rect(), c.viewport);
    }
}

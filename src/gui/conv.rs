use eframe::egui::{Pos2, Rect, Vec2};
use ulam_leapers::math::coords::{GridPoint, GridVector};
use ulam_leapers::math::rect::GridRect;

pub fn grid_point_to_egui(point: GridPoint) -> Pos2 {
    Pos2::new(point.x as f32, point.y as f32)
}

pub fn grid_vector_to_egui(vector: GridVector) -> Vec2 {
    Vec2::new(vector.x as f32, vector.y as f32)
}

pub fn grid_rect_to_egui(rect: GridRect) -> Rect {
    Rect::from_min_max(grid_point_to_egui(rect.start), grid_point_to_egui(rect.end))
}

pub fn egui_to_grid_point(point: Pos2) -> GridPoint {
    GridPoint::new(point.x as i32, point.y as i32)
}

pub fn egui_to_grid_vector(vector: Vec2) -> GridVector {
    GridVector::new(vector.x as i32, vector.y as i32)
}

pub fn egui_to_grid_rect(rect: Rect) -> GridRect {
    GridRect::with_start_end(egui_to_grid_point(rect.min), egui_to_grid_point(rect.max))
}

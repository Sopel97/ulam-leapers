use std::cmp;
use crate::math::coords::GridPoint;
use crate::math::pow2::{div_floor, floor_to_multiple, Pow2};
use crate::math::rect::GridRect;

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum FlipAxis {
    None,
    X,
    Y,
    XY,
}

impl FlipAxis {
    pub fn as_bools(&self) -> (bool, bool) {
        let flip_x = matches!(self, FlipAxis::X) || matches!(self, FlipAxis::XY);
        let flip_y = matches!(self, FlipAxis::Y) || matches!(self, FlipAxis::XY);
        (flip_x, flip_y)
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct ScreenWorldDiscrete2D {
    zoom_pow2: i32,
    screen_rect: GridRect,
    world_rect: GridRect,
    flip_x: bool,
    flip_y: bool,
}

impl ScreenWorldDiscrete2D {
    pub fn new(
        zoom_pow2: i32,
        origin_world: GridPoint,
        screen_rect: GridRect,
        flip_axis: FlipAxis,
    ) -> ScreenWorldDiscrete2D {
        let world_rect = match zoom_pow2 {
            e @ 0.. => {
                let factor = Pow2::from_exponent(e as u8);

                GridRect::with_size(
                    GridPoint::new(
                        origin_world.x - div_floor(screen_rect.width() / 2, factor),
                        origin_world.y - div_floor(screen_rect.height() / 2, factor),
                    ),
                    div_floor(screen_rect.width(), factor),
                    div_floor(screen_rect.height(), factor),
                )
            }
            e @ ..0 => {
                let factor = Pow2::from_exponent((-e) as u8);
                // We have to ensure proper alignment for the sampling.
                let factor_i32: i32 = factor.as_u64() as i32;

                GridRect::with_size(
                    GridPoint::new(
                        floor_to_multiple(origin_world.x, factor) - screen_rect.width() / 2 * factor_i32,
                        floor_to_multiple(origin_world.y, factor) - screen_rect.height() / 2 * factor_i32,
                    ),
                    screen_rect.width() * factor_i32,
                    screen_rect.height() * factor_i32,
                )
            }
        };

        let (flip_x, flip_y) = flip_axis.as_bools();

        ScreenWorldDiscrete2D {
            screen_rect,
            zoom_pow2,
            world_rect,
            flip_x,
            flip_y,
        }
    }

    pub fn world_rect(&self) -> GridRect {
        self.world_rect
    }

    pub fn screen_rect(&self) -> GridRect {
        self.screen_rect
    }

    pub fn screen_to_world(&self, screen_point: GridPoint) -> GridPoint {
        let mut dx;
        let mut dy;

        match self.zoom_pow2 {
            e @ 0.. => {
                let factor = Pow2::from_exponent(e as u8);
                dx = div_floor(screen_point.x - self.screen_rect.start.x, factor);
                dy = div_floor(screen_point.y - self.screen_rect.start.y, factor);
            }
            e @ ..0 => {
                let factor = Pow2::from_exponent((-e) as u8);
                let factor_i32: i32 = factor.as_u64() as i32;
                dx = (screen_point.x - self.screen_rect.start.x) * factor_i32;
                dy = (screen_point.y - self.screen_rect.start.y) * factor_i32;
            }
        }

        if self.flip_x {
            dx = self.world_rect.width() - dx - 1;
        }
        if self.flip_y {
            dy = self.world_rect.height() - dy - 1;
        }
        let x = self.world_rect.start.x + dx;
        let y = self.world_rect.start.y + dy;
        GridPoint::new(x, y)
    }

    pub fn world_to_screen(&self, world_point: GridPoint) -> GridPoint {
        let mut dx;
        let mut dy;

        match self.zoom_pow2 {
            e @ 0.. => {
                let factor = Pow2::from_exponent(e as u8);
                let factor_i32: i32 = factor.as_u64() as i32;
                dx = (world_point.x - self.world_rect.start.x) * factor_i32;
                dy = (world_point.y - self.world_rect.start.y) * factor_i32;
            }
            e @ ..0 => {
                let factor = Pow2::from_exponent((-e) as u8);
                dx = div_floor(world_point.x - self.world_rect.start.x, factor);
                dy = div_floor(world_point.y - self.world_rect.start.y, factor);
            }
        }

        if self.flip_x {
            dx = self.screen_rect.width() - dx - 1;
        }
        if self.flip_y {
            dy = self.screen_rect.height() - dy - 1;
        }
        let x = self.screen_rect.start.x + dx;
        let y = self.screen_rect.start.y + dy;
        GridPoint::new(x, y)
    }

    pub fn world_to_screen_rect(&self, world_rect: GridRect) -> GridRect {
        let p0 = self.world_to_screen(world_rect.start);
        let p1 = self.world_to_screen(world_rect.end);
        GridRect::with_start_end(
            GridPoint::new(cmp::min(p0.x, p1.x), cmp::min(p0.y, p1.y)),
            GridPoint::new(cmp::max(p0.x, p1.x), cmp::max(p0.y, p1.y)),
        )
    }
}

use std::cmp;
use ulam_leapers::math::coords::{GridPoint, GridVector};
use ulam_leapers::math::pow2::{div_floor, floor_to_multiple, Pow2};
use ulam_leapers::math::rect::GridRect;
use ulam_leapers::math::zoom::Zoom;

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

/// Represents a screen to world projection, where the world is a discrete grid.
/// Supports power-of-two zoom factors - both magnification and minification.
/// When magnifying, the world grid is kept uniformly sized, that is, every
/// world grid point has the same size in screen space.
/// Since this is a discrete projection the coordinates are always aligned
/// to the world grid. This means that multiple different `camera_position` values
/// may correspond to the same world position depending on the `zoom`.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct GridProjection {
    zoom: Zoom<Pow2>,
    screen_rect: GridRect,
    world_rect: GridRect,
    flip_x: bool,
    flip_y: bool,
}

impl GridProjection {
    /// Creates a new projection based on camera parameters and viewport.
    /// `camera_position` is the world point to be centered in the viewport.
    pub fn new(
        zoom: Zoom<Pow2>,
        camera_position: GridPoint,
        screen_rect: GridRect,
        flip_axis: FlipAxis,
    ) -> GridProjection {
        let world_rect = match zoom {
            Zoom::Magnification(factor) => GridRect::with_size(
                GridPoint::new(
                    camera_position.x - div_floor(screen_rect.width() / 2, factor),
                    camera_position.y - div_floor(screen_rect.height() / 2, factor),
                ),
                div_floor(screen_rect.width(), factor),
                div_floor(screen_rect.height(), factor),
            ),
            Zoom::Minification(factor) => {
                // We have to ensure proper alignment for the sampling.
                let factor_i32: i32 = factor.as_u64() as i32;

                GridRect::with_size(
                    GridPoint::new(
                        floor_to_multiple(camera_position.x, factor)
                            - screen_rect.width() / 2 * factor_i32,
                        floor_to_multiple(camera_position.y, factor)
                            - screen_rect.height() / 2 * factor_i32,
                    ),
                    screen_rect.width() * factor_i32,
                    screen_rect.height() * factor_i32,
                )
            }
        };

        let (flip_x, flip_y) = flip_axis.as_bools();

        GridProjection {
            screen_rect,
            zoom,
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

    pub fn zoom(&self) -> Zoom<Pow2> {
        self.zoom
    }

    pub fn screen_to_world(&self, screen_point: GridPoint) -> GridPoint {
        let mut dx;
        let mut dy;

        match self.zoom {
            Zoom::Magnification(factor) => {
                dx = div_floor(screen_point.x - self.screen_rect.start.x, factor);
                dy = div_floor(screen_point.y - self.screen_rect.start.y, factor);
            }
            Zoom::Minification(factor) => {
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

        match self.zoom {
            Zoom::Magnification(factor) => {
                let factor_i32: i32 = factor.as_u64() as i32;
                dx = (world_point.x - self.world_rect.start.x) * factor_i32;
                dy = (world_point.y - self.world_rect.start.y) * factor_i32;
            }
            Zoom::Minification(factor) => {
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

    /// Projects the given `world_rect` into screen coordinates.
    /// Note, that if flipping of any axis is present this is not equivalent
    /// to simply mapping `world_rect.start` and `world_rect.end` separately. 
    pub fn world_to_screen_rect(&self, world_rect: GridRect) -> GridRect {
        // Due to optional flipping of axes we have to be very careful, because
        // if we provide `rect.end`, which is outside the rectangle's area, it may get
        // flipped to be outside too but on the side we don't expect.

        let mut screen_rect = GridRect::with_start_end(
            self.world_to_screen(world_rect.start),
            self.world_to_screen(world_rect.end)
        );

        // We avoid this by checking if the resulting coordinates are in ascending order,
        // and correcting the range by swapping start with end in case they are not.
        // (end, start] -> (start, end] -> [start, end)
        if screen_rect.start.x > screen_rect.end.x {
            std::mem::swap(&mut screen_rect.start.x, &mut screen_rect.end.x);
            // One is added to each start and end to account for exclusivity of the range.
            // We want [start, end), not (start, end]
            screen_rect.start.x += 1;
            screen_rect.end.x += 1;
        }
        if screen_rect.start.y > screen_rect.end.y {
            std::mem::swap(&mut screen_rect.start.y, &mut screen_rect.end.y);
            screen_rect.start.y += 1;
            screen_rect.end.y += 1;
        }

        screen_rect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen_rect() -> GridRect {
        GridRect::with_size(GridPoint::new(0, 0), 100, 80)
    }

    #[test]
    fn flip_axis_as_bools() {
        assert_eq!(FlipAxis::None.as_bools(), (false, false));
        assert_eq!(FlipAxis::X.as_bools(), (true, false));
        assert_eq!(FlipAxis::Y.as_bools(), (false, true));
        assert_eq!(FlipAxis::XY.as_bools(), (true, true));
    }

    #[test]
    fn magnification_world_rect_is_centered_on_camera() {
        let zoom = Zoom::Magnification(Pow2::from_exponent(1));
        let camera = GridPoint::new(50, 40);

        let projection = GridProjection::new(zoom, camera, screen_rect(), FlipAxis::None);

        assert_eq!(
            projection.world_rect(),
            GridRect::with_size(GridPoint::new(25, 20), 50, 40,)
        );
    }

    #[test]
    fn magnification_world_to_screen_origin() {
        let zoom = Zoom::Magnification(Pow2::from_exponent(1));

        let projection =
            GridProjection::new(zoom, GridPoint::new(50, 40), screen_rect(), FlipAxis::None);

        let screen = projection.world_to_screen(GridPoint::new(25, 20));

        assert_eq!(screen, GridPoint::new(0, 0));
    }

    #[test]
    fn magnification_screen_to_world_origin() {
        let zoom = Zoom::Magnification(Pow2::from_exponent(1));

        let projection =
            GridProjection::new(zoom, GridPoint::new(50, 40), screen_rect(), FlipAxis::None);

        let world = projection.screen_to_world(GridPoint::new(0, 0));

        assert_eq!(world, GridPoint::new(25, 20));
    }

    #[test]
    fn magnification_correct_size_ratio() {
        let factor = Pow2::from_exponent(3);
        let zoom = Zoom::Magnification(factor);

        let screen_rect = screen_rect();
        let projection =
            GridProjection::new(zoom, GridPoint::new(50, 40), screen_rect, FlipAxis::None);
        let world_rect = projection.world_rect();

        assert_eq!(world_rect.width(), div_floor(screen_rect.width(), factor));
        assert_eq!(world_rect.height(), div_floor(screen_rect.height(), factor));
    }

    #[test]
    fn minification_correct_size_ratio() {
        let factor = Pow2::from_exponent(3);
        let zoom = Zoom::Minification(factor);

        let screen_rect = screen_rect();
        let projection =
            GridProjection::new(zoom, GridPoint::new(50, 40), screen_rect, FlipAxis::None);
        let world_rect = projection.world_rect();

        assert_eq!(
            world_rect.width(),
            screen_rect.width() * factor.as_u64() as i32
        );
        assert_eq!(
            world_rect.height(),
            screen_rect.height() * factor.as_u64() as i32
        );
    }

    #[test]
    fn magnification_world_to_screen_scales_by_factor() {
        let zoom = Zoom::Magnification(Pow2::from_exponent(1));

        let projection =
            GridProjection::new(zoom, GridPoint::new(50, 40), screen_rect(), FlipAxis::None);

        let screen = projection.world_to_screen(GridPoint::new(26, 21));

        assert_eq!(screen, GridPoint::new(2, 2));
    }

    #[test]
    fn flip_x_reverses_horizontal_mapping() {
        let zoom = Zoom::Magnification(Pow2::from_exponent(0));

        let projection =
            GridProjection::new(zoom, GridPoint::new(50, 40), screen_rect(), FlipAxis::X);

        let left_world = projection.world_rect().start;

        let screen = projection.world_to_screen(left_world);

        assert_eq!(screen.x, projection.screen_rect().width() - 1);
    }

    #[test]
    fn flip_y_reverses_vertical_mapping() {
        let zoom = Zoom::Magnification(Pow2::from_exponent(0));

        let projection =
            GridProjection::new(zoom, GridPoint::new(50, 40), screen_rect(), FlipAxis::Y);

        let top_world = projection.world_rect().start;

        let screen = projection.world_to_screen(top_world);

        assert_eq!(screen.y, projection.screen_rect().height() - 1);
    }

    #[test]
    fn flip_xy_reverses_both_axes() {
        let zoom = Zoom::Magnification(Pow2::from_exponent(0));

        let projection =
            GridProjection::new(zoom, GridPoint::new(50, 40), screen_rect(), FlipAxis::XY);

        let corner = projection.world_rect().start;

        let screen = projection.world_to_screen(corner);

        assert_eq!(screen.x, projection.screen_rect().width() - 1);
        assert_eq!(screen.y, projection.screen_rect().height() - 1);
    }

    #[test]
    fn minification_world_rect_is_aligned_to_factor() {
        let zoom = Zoom::Minification(Pow2::from_exponent(2));

        let projection = GridProjection::new(
            zoom,
            GridPoint::new(53, 41),
            GridRect::with_size(GridPoint::new(0, 0), 10, 8),
            FlipAxis::None,
        );

        let world_rect = projection.world_rect();

        assert_eq!(world_rect.start.x % 4, 0);
        assert_eq!(world_rect.start.y % 4, 0);
        assert_eq!(world_rect.width(), 40);
        assert_eq!(world_rect.height(), 32);
    }

    #[test]
    fn world_to_screen_rect_preserves_bounds() {
        let zoom = Zoom::Magnification(Pow2::from_exponent(0));

        let projection =
            GridProjection::new(zoom, GridPoint::new(50, 40), screen_rect(), FlipAxis::None);

        let rect = GridRect::with_start_end(GridPoint::new(10, 10), GridPoint::new(20, 20));

        let projected = projection.world_to_screen_rect(rect);

        assert!(projected.width() >= 0);
        assert!(projected.height() >= 0);
    }

    #[test]
    fn world_screen_world_round_trip_without_flips() {
        let zoom = Zoom::Magnification(Pow2::from_exponent(1));

        let projection =
            GridProjection::new(zoom, GridPoint::new(50, 40), screen_rect(), FlipAxis::None);

        let world = GridPoint::new(30, 25);

        let screen = projection.world_to_screen(world);
        let recovered = projection.screen_to_world(screen);

        assert_eq!(recovered, world);
    }
}

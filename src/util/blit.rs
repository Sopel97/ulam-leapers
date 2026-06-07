use crate::collections::array2d::Array2D;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blit2D {
    pub src_x: usize,
    pub src_y: usize,
    pub dst_x: usize,
    pub dst_y: usize,
    pub width: usize,
    pub height: usize,
}

pub fn blit_array2d<T>(src: &Array2D<T>, dst: &mut Array2D<T>, blit: &Blit2D)
where
    T: Clone + Copy
{
    for y in 0..blit.height {
        for x in 0..blit.width {
            dst[(blit.dst_x + x, blit.dst_y + y)] = src[(blit.src_x + x, blit.src_y + y)];
        }
    }
}

/// # Safety
///
/// Calling this function with a source blit region that is not contained
/// within the source array, or a destination blit region that is not contained
/// within the destination array, is *[undefined behavior]*
///
/// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
pub unsafe fn blit_array2d_unchecked<T>(src: &Array2D<T>, dst: &mut Array2D<T>, blit: &Blit2D)
where
    T: Clone + Copy
{
    for y in 0..blit.height {
        for x in 0..blit.width {
            unsafe {
                *dst.get_unchecked_mut(blit.dst_x + x, blit.dst_y + y) = *src
                    .get_unchecked(
                        blit.src_x + x,
                        blit.src_y + y,
                    );
            }
        }
    }
}
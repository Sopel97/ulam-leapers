use crate::collections::array2d::{MutSlice2D, Slice2D};
use crate::util::pow2;
use crate::util::pow2::Pow2;

// Effectively inserts a 0 bit before every bit of the source.
// Can be implemented faster with specific instructions, but we don't really care about speed.
fn carryless_square(mut n: u16) -> u32 {
    n &= 0x00ff;
    n = (n | (n << 4)) & 0x0F0F;
    n = (n | (n << 2)) & 0x3333;
    n = (n | (n << 1)) & 0x5555;
    n as u32
}

// "Squishes" the number by removing every even bit.
// Could be implemented with pext, but we don't really care about speed.
fn inv_carryless_square(mut n: u32) -> u16 {
    n &= 0x5555;
    n = (n ^ (n >> 1)) & 0x3333;
    n = (n ^ (n >> 2)) & 0x0F0F;
    n = (n ^ (n >> 4)) & 0x00FF;
    n as u16
}

fn d_to_xy(index: u32) -> (u16, u16) {
    let x = inv_carryless_square(index);
    let y = inv_carryless_square(index >> 1);
    (x, y)
}

fn xy_to_d(x: u16, y: u16) -> u32 {
    (carryless_square(y) << 1) | carryless_square(x)
}

pub struct MortonCurveTransform {
    size: Pow2,
    d_to_xy_table: Box<[(u16, u16)]>,
    xy_to_d_table: Box<[u32]>,
}

impl MortonCurveTransform {
    pub fn new(size: Pow2) -> MortonCurveTransform {
        let sz: usize = size.into();
        let table_size: usize = sz * sz;
        let mut d_to_xy_table: Box<[(u16, u16)]> = vec![Default::default(); table_size].into_boxed_slice();
        let mut xy_to_d_table: Box<[u32]> = vec![Default::default(); table_size].into_boxed_slice();
        for d in 0..table_size {
            let (x, y) = d_to_xy(d as u32);
            d_to_xy_table[d] = (x, y);
            xy_to_d_table[(y as usize) * sz + (x as usize)] = d as u32;
        }
        MortonCurveTransform {
            size,
            d_to_xy_table,
            xy_to_d_table,
        }
    }

    fn transform_chunk<T: Clone + Copy>(&self, input: Slice2D<T>, output: &mut [T]) {
        assert_eq!(input.width(), self.size.into());
        assert_eq!(input.height(), self.size.into());

        let side_size: usize = self.size.into();
        let chunk_size: usize = side_size * side_size;

        for d in 0..chunk_size {
            let (x, y) = self.d_to_xy_table[d];
            output[d] = input[(x as usize, y as usize)];
        }
    }

    pub fn transform<T: Clone + Copy>(&self, input: Slice2D<T>, output: &mut [T]) {
        if pow2::floor_mod(input.width(), self.size) != 0 || pow2::floor_mod(input.height(), self.size) != 0 {
            panic!("Cannot chunk input without remainder");
        }

        let wc = pow2::floor_div(input.width(), self.size);
        let hc = pow2::floor_div(input.height(), self.size);

        let side_size: usize = self.size.into();
        let chunk_size: usize = side_size * side_size;
        for cx in 0..wc {
            for cy in 0..hc {
                let x_start = cx * side_size;
                let x_end = x_start + side_size;
                let y_start = cy * side_size;
                let y_end = y_start + side_size;

                let d_start = (cy * wc + cx) * chunk_size;
                let d_end = d_start + chunk_size;

                let input_chunk = input.slice2d(x_start..x_end, y_start..y_end);
                let output_chunk = &mut output[d_start..d_end];

                self.transform_chunk(input_chunk, output_chunk);
            }
        }
    }

    fn inv_transform_chunk<T: Clone + Copy>(&self, input: &[T], mut output: MutSlice2D<T>) {
        assert_eq!(output.width(), self.size.into());
        assert_eq!(output.height(), self.size.into());

        let side_size: usize = self.size.into();
        let chunk_size: usize = side_size * side_size;

        for d in 0..chunk_size {
            let (x, y) = self.d_to_xy_table[d];
            output[(x as usize, y as usize)] = input[d];
        }
    }

    pub fn inv_transform<T: Clone + Copy>(&self, input: &[T], mut output: MutSlice2D<T>) {
        if pow2::floor_mod(output.width(), self.size) != 0 || pow2::floor_mod(output.height(), self.size) != 0 {
            panic!("Cannot chunk input without remainder");
        }

        let wc = pow2::floor_div(output.width(), self.size);
        let hc = pow2::floor_div(output.height(), self.size);

        let side_size: usize = self.size.into();
        let chunk_size: usize = side_size * side_size;
        for cx in 0..wc {
            for cy in 0..hc {
                let x_start = cx * side_size;
                let x_end = x_start + side_size;
                let y_start = cy * side_size;
                let y_end = y_start + side_size;

                let d_start = (cy * wc + cx) * chunk_size;
                let d_end = d_start + chunk_size;

                let input_chunk = &input[d_start..d_end];
                let output_chunk = output.mut_slice2d(x_start..x_end, y_start..y_end);

                self.inv_transform_chunk(input_chunk, output_chunk);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::algo::morton::MortonCurveTransform;
    use crate::collections::array2d::Array2D;
    use crate::util::align::MemoryAlignment;
    use crate::util::pow2::Pow2;

    #[test]
    fn single_chunk_transform() {
        let t = MortonCurveTransform::new(Pow2::new(2));
        let mut input = Array2D::<u8>::new(2, 2);
        input[(0, 0)] = 1;
        input[(0, 1)] = 2;
        input[(1, 0)] = 3;
        input[(1, 1)] = 4;

        let mut output = vec![0u8; input.width() * input.height()];
        t.transform(input.as_slice2d(), output.as_mut_slice());

        assert_eq!(output, vec![1, 3, 2, 4]);
    }

    #[test]
    fn multi_chunk_transform() {
        let t = MortonCurveTransform::new(Pow2::new(2));
        let mut input = Array2D::<u8>::new(4, 2);
        input[(0, 0)] = 1;
        input[(0, 1)] = 2;
        input[(1, 0)] = 3;
        input[(1, 1)] = 4;
        input[(2, 0)] = 5;
        input[(2, 1)] = 6;
        input[(3, 0)] = 7;
        input[(3, 1)] = 8;

        let mut output = vec![0u8; input.width() * input.height()];
        t.transform(input.as_slice2d(), output.as_mut_slice());

        assert_eq!(output, vec![1, 3, 2, 4, 5, 7, 6, 8]);
    }

    #[test]
    fn multi_chunk_transform_roundtrip() {
        let t = MortonCurveTransform::new(Pow2::new(2));
        let mut input = Array2D::<u8>::new(4, 2);
        input[(0, 0)] = 1;
        input[(0, 1)] = 2;
        input[(1, 0)] = 3;
        input[(1, 1)] = 4;
        input[(2, 0)] = 5;
        input[(2, 1)] = 6;
        input[(3, 0)] = 7;
        input[(3, 1)] = 8;

        let mut output = vec![0u8; input.width() * input.height()];
        t.transform(input.as_slice2d(), output.as_mut_slice());

        assert_eq!(output, vec![1, 3, 2, 4, 5, 7, 6, 8]);

        let mut output2 = Array2D::<u8>::new(4, 2);
        t.inv_transform(output.as_slice(), output2.as_mut_slice2d());

        assert_eq!(output2.as_flat_slice(), input.as_flat_slice());
    }
}
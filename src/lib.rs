pub mod coords;
pub mod grid;
pub mod collections {
    pub mod sliding_window;
    pub mod aligned_boxed_slice;
    pub mod array2d;
}
pub mod piece;
pub mod simulation;
pub mod algo {
    pub mod bit_transpose {
        pub mod sse2;
        pub mod avx2;
        pub mod avx512bw;
        pub mod dispatch;
    }
    pub mod morton;
}
pub mod util {
    pub mod align;
    pub mod pow2;
}
pub mod compression {
    pub mod rle;
}

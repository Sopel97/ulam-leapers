pub mod coords;
pub mod grid;
pub mod collections {
    pub mod sliding_window;
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
}

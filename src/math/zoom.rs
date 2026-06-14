#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zoom<T> 
where
    T: Copy
{
    Magnification(T),
    Minification(T),
}
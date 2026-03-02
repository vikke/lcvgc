#[derive(Debug, Clone, PartialEq)]
pub enum Tempo {
    Absolute(u16),
    Relative(i16),
}

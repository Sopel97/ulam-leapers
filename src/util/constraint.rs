use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::ops::RangeInclusive;

#[derive(Debug)]
pub struct ConstraintViolationError {
    message: String,
}

impl Display for ConstraintViolationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for ConstraintViolationError {}

impl ConstraintViolationError {
    pub fn new(message: String) -> Self {
        Self {
            message,
        }
    }

    pub fn take_message(self) -> String {
        self.message
    }
}

pub trait Constraint<T> {
    fn check_constraint(&self, val: &T, name: &str) -> Result<(), ConstraintViolationError>;
}

impl<T> Constraint<T> for RangeInclusive<T>
where
    T: Debug + Display + PartialOrd,
{
    fn check_constraint(&self, val: &T, name: &str) -> Result<(), ConstraintViolationError> {
        if !self.contains(val) {
            Err(ConstraintViolationError::new(format!(
                "{name} {val} outside allowed range {self:?}"
            )))
        } else {
            Ok(())
        }
    }
}

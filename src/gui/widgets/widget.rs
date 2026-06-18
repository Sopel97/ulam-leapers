use eframe::egui::{Response, Ui};
use serde_json::Value;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::ops::RangeInclusive;
use ulam_leapers::util::json::JsonError;

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum JsonWidgetError {
    JsonError(JsonError),
    WidgetError(WidgetError),
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum WidgetError {
    ConstraintViolation(String),
    InvalidState(String),
}

impl Display for WidgetError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            WidgetError::ConstraintViolation(e) => write!(f, "Constraint violation: {}", e),
            WidgetError::InvalidState(e) => write!(f, "Invalid state: {}", e),
        }
    }
}

impl Error for WidgetError {}

impl Display for JsonWidgetError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonWidgetError::JsonError(e) => Display::fmt(&e, f),
            JsonWidgetError::WidgetError(e) => Display::fmt(&e, f),
        }
    }
}

impl Error for JsonWidgetError {}

impl From<JsonError> for JsonWidgetError {
    fn from(e: JsonError) -> Self {
        JsonWidgetError::JsonError(e)
    }
}

impl From<WidgetError> for JsonWidgetError {
    fn from(e: WidgetError) -> Self {
        JsonWidgetError::WidgetError(e)
    }
}

pub trait JsonWidget
where
    Self: Sized,
{
    type ConstraintsType;

    fn to_json(&self) -> Value;
    fn try_from_json(
        json: &Value,
        constraints: Self::ConstraintsType,
    ) -> Result<Self, JsonWidgetError>;
}

pub trait StatefulWidget
where
    Self: Sized,
{
    fn ui(&mut self, ui: &mut Ui) -> Response;
}

pub trait WidgetConstraint<T> {
    fn check_constraint(&self, val: &T, name: &str) -> Result<(), WidgetError>;
}

impl<T> WidgetConstraint<T> for RangeInclusive<T>
where
    T: Debug + Display + PartialOrd,
{
    fn check_constraint(&self, val: &T, name: &str) -> Result<(), WidgetError> {
        if !self.contains(val) {
            Err(WidgetError::ConstraintViolation(format!(
                "{name} {val} outside allowed range {self:?}"
            )))
        } else {
            Ok(())
        }
    }
}

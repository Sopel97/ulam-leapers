use eframe::egui::{Response, Ui};
use serde_json::Value;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use ulam_leapers::util::constraint::ConstraintViolationError;
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

impl From<ConstraintViolationError> for WidgetError {
    fn from(e: ConstraintViolationError) -> Self {
        WidgetError::ConstraintViolation(e.take_message())
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

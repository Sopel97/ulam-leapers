use eframe::egui::{Response, Ui};
use serde_json::Value;

pub trait JsonWidget
where
    Self: Sized,
{
    type ConstraintsType;

    fn to_json(&self) -> Value;
    fn try_from_json(json: &Value, constraints: &Self::ConstraintsType) -> Option<Self>;
}

pub trait StatefulWidget {
    fn ui(&mut self, ui: &mut Ui) -> Response;
}
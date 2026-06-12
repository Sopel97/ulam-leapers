use serde_json::{Map, Value};
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum JsonError {
    MissingIndex(usize),
    MissingKey(String),
    TypeMismatchAtIndex(usize, &'static str),
    TypeMismatchAtKey(String, &'static str),
}

impl Display for JsonError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonError::MissingIndex(index) => write!(f, "Index {} is missing", index),
            JsonError::MissingKey(key) => write!(f, "missing key: {}", key),
            JsonError::TypeMismatchAtIndex(index, expected_type) => write!(
                f,
                "value under index {} is not of type {}",
                index, expected_type
            ),
            JsonError::TypeMismatchAtKey(key, expected_type) => write!(
                f,
                "value under key {} is not of type {}",
                key, expected_type
            ),
        }
    }
}

impl Error for JsonError {}

pub trait JsonIndex {
    fn index_into<'a>(&self, value: &'a Value) -> Result<&'a Value, JsonError>;
    fn missing_key(&self) -> JsonError;
    fn mismatched_type(&self, expected_type: &'static str) -> JsonError;
}

impl JsonIndex for usize {
    fn index_into<'a>(&self, value: &'a Value) -> Result<&'a Value, JsonError> {
        value.get(self).ok_or_else(|| self.missing_key())
    }

    fn missing_key(&self) -> JsonError {
        JsonError::MissingIndex(*self)
    }

    fn mismatched_type(&self, expected_type: &'static str) -> JsonError {
        JsonError::TypeMismatchAtIndex(*self, expected_type)
    }
}

impl JsonIndex for str {
    fn index_into<'a>(&self, value: &'a Value) -> Result<&'a Value, JsonError> {
        value.get(self).ok_or_else(|| self.missing_key())
    }

    fn missing_key(&self) -> JsonError {
        JsonError::MissingKey(self.to_string())
    }

    fn mismatched_type(&self, expected_type: &'static str) -> JsonError {
        JsonError::TypeMismatchAtKey(self.to_string(), expected_type)
    }
}

impl<T> JsonIndex for &T
where
    T: ?Sized + JsonIndex,
{
    fn index_into<'b>(&self, value: &'b Value) -> Result<&'b Value, JsonError> {
        (**self).index_into(value)
    }

    fn missing_key(&self) -> JsonError {
        (**self).missing_key()
    }

    fn mismatched_type(&self, expected_type: &'static str) -> JsonError {
        (**self).mismatched_type(expected_type)
    }
}

pub trait SerdeJsonValueExt {
    fn read_value(&self, index: impl JsonIndex) -> Result<&Value, JsonError>;
    fn read_u64(&self, index: impl JsonIndex) -> Result<u64, JsonError>;
    fn read_i64(&self, index: impl JsonIndex) -> Result<i64, JsonError>;
    fn read_f64(&self, index: impl JsonIndex) -> Result<f64, JsonError>;
    fn read_bool(&self, index: impl JsonIndex) -> Result<bool, JsonError>;
    fn read_null(&self, index: impl JsonIndex) -> Result<(), JsonError>;
    fn read_string(&self, index: impl JsonIndex) -> Result<&str, JsonError>;
    fn read_array(&self, index: impl JsonIndex) -> Result<&Vec<Value>, JsonError>;
    fn read_object(&self, index: impl JsonIndex) -> Result<&Map<String, Value>, JsonError>;
}

impl SerdeJsonValueExt for Value {
    fn read_value(&self, index: impl JsonIndex) -> Result<&Value, JsonError> {
        index.index_into(self)
    }

    fn read_u64(&self, index: impl JsonIndex) -> Result<u64, JsonError> {
        index
            .index_into(self)?
            .as_u64()
            .ok_or_else(|| index.mismatched_type("u64"))
    }

    fn read_i64(&self, index: impl JsonIndex) -> Result<i64, JsonError> {
        index
            .index_into(self)?
            .as_i64()
            .ok_or_else(|| index.mismatched_type("i64"))
    }

    fn read_f64(&self, index: impl JsonIndex) -> Result<f64, JsonError> {
        index
            .index_into(self)?
            .as_f64()
            .ok_or_else(|| index.mismatched_type("f64"))
    }

    fn read_bool(&self, index: impl JsonIndex) -> Result<bool, JsonError> {
        index
            .index_into(self)?
            .as_bool()
            .ok_or_else(|| index.mismatched_type("bool"))
    }

    fn read_null(&self, index: impl JsonIndex) -> Result<(), JsonError> {
        index
            .index_into(self)?
            .as_null()
            .ok_or_else(|| index.mismatched_type("null"))
    }

    fn read_string(&self, index: impl JsonIndex) -> Result<&str, JsonError> {
        index
            .index_into(self)?
            .as_str()
            .ok_or_else(|| index.mismatched_type("string"))
    }

    fn read_array(&self, index: impl JsonIndex) -> Result<&Vec<Value>, JsonError> {
        index
            .index_into(self)?
            .as_array()
            .ok_or_else(|| index.mismatched_type("array"))
    }

    fn read_object(&self, index: impl JsonIndex) -> Result<&Map<String, Value>, JsonError> {
        index
            .index_into(self)?
            .as_object()
            .ok_or_else(|| index.mismatched_type("object"))
    }
}

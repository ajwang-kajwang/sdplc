//! Runtime process image for SD-PLC.
//!
//! A process image is the shared memory model between compiled control
//! logic, plant simulation, diagnostics, and the OPC UA server. Keeping it
//! small and typed makes the final validation story straightforward:
//! ST variables become process variables; process variables become OPC UA
//! nodes; benchmark samples are taken from the same source of truth.

use std::collections::BTreeMap;
use std::fmt;

/// Runtime value supported by the validation and OPC UA bridge layers.
#[derive(Debug, Clone, PartialEq)]
pub enum PlcValue {
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    Text(String),
}

impl PlcValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PlcValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            PlcValue::F64(v) => Some(*v),
            PlcValue::I64(v) => Some(*v as f64),
            PlcValue::U64(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            PlcValue::Bool(_) => "BOOL",
            PlcValue::I64(_) => "LINT",
            PlcValue::U64(_) => "ULINT",
            PlcValue::F64(_) => "LREAL",
            PlcValue::Text(_) => "STRING",
        }
    }
}

impl fmt::Display for PlcValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlcValue::Bool(v) => write!(f, "{}", if *v { "TRUE" } else { "FALSE" }),
            PlcValue::I64(v) => write!(f, "{}", v),
            PlcValue::U64(v) => write!(f, "{}", v),
            PlcValue::F64(v) => write!(f, "{:.6}", v),
            PlcValue::Text(v) => write!(f, "{}", v),
        }
    }
}

/// A single named process variable.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessVariable {
    pub name: String,
    pub value: PlcValue,
    pub writable: bool,
    pub description: String,
}

impl ProcessVariable {
    pub fn new(name: impl Into<String>, value: PlcValue) -> Self {
        Self {
            name: name.into(),
            value,
            writable: true,
            description: String::new(),
        }
    }

    pub fn read_only(mut self) -> Self {
        self.writable = false;
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
}

/// Deterministic, sorted process image.
#[derive(Debug, Clone, Default)]
pub struct ProcessImage {
    variables: BTreeMap<String, ProcessVariable>,
}

impl ProcessImage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, variable: ProcessVariable) {
        self.variables.insert(variable.name.clone(), variable);
    }

    pub fn set(&mut self, name: &str, value: PlcValue) -> Result<(), ProcessImageError> {
        match self.variables.get_mut(name) {
            Some(var) if var.writable => {
                var.value = value;
                Ok(())
            }
            Some(_) => Err(ProcessImageError::ReadOnly(name.to_string())),
            None => Err(ProcessImageError::Missing(name.to_string())),
        }
    }

    pub fn get(&self, name: &str) -> Option<&PlcValue> {
        self.variables.get(name).map(|v| &v.value)
    }

    pub fn get_f64(&self, name: &str) -> Option<f64> {
        self.get(name).and_then(PlcValue::as_f64)
    }

    pub fn get_bool(&self, name: &str) -> Option<bool> {
        self.get(name).and_then(PlcValue::as_bool)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ProcessVariable> {
        self.variables.values()
    }

    pub fn len(&self) -> usize {
        self.variables.len()
    }

    pub fn is_empty(&self) -> bool {
        self.variables.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessImageError {
    Missing(String),
    ReadOnly(String),
}

impl fmt::Display for ProcessImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessImageError::Missing(name) => write!(f, "process variable '{}' does not exist", name),
            ProcessImageError::ReadOnly(name) => write!(f, "process variable '{}' is read-only", name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_values_round_trip() {
        let mut image = ProcessImage::new();
        image.insert(ProcessVariable::new("level", PlcValue::F64(42.5)));
        image.insert(ProcessVariable::new("running", PlcValue::Bool(true)));

        assert_eq!(image.get_f64("level"), Some(42.5));
        assert_eq!(image.get_bool("running"), Some(true));

        image.set("level", PlcValue::F64(50.0)).unwrap();
        assert_eq!(image.get_f64("level"), Some(50.0));
    }
}

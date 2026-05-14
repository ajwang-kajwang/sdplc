//! OPC UA bridge scaffold for SD-PLC.
//!
//! The immediate sprint target is to expose the runtime process image through
//! a browsable OPC UA address space. This module deliberately keeps the core
//! address-space mapping independent from any specific OPC UA crate so the
//! next sprint can bind it either to `open62541` through FFI or to a pure Rust
//! server crate without changing validation code.

use crate::process_image::{PlcValue, ProcessImage};

/// Logical OPC UA node generated from a process-image variable.
#[derive(Debug, Clone, PartialEq)]
pub struct OpcUaNodeSpec {
    pub node_id: String,
    pub browse_name: String,
    pub data_type: &'static str,
    pub writable: bool,
    pub value: PlcValue,
    pub description: String,
}

/// Maps process variables into a deterministic OPC UA namespace layout.
#[derive(Debug, Clone)]
pub struct OpcUaAddressSpace {
    namespace_uri: String,
    nodes: Vec<OpcUaNodeSpec>,
}

impl OpcUaAddressSpace {
    pub fn from_process_image(namespace_uri: impl Into<String>, image: &ProcessImage) -> Self {
        let namespace_uri = namespace_uri.into();
        let nodes = image
            .iter()
            .map(|var| OpcUaNodeSpec {
                node_id: format!("ns=2;s=SDPLC.{}", var.name),
                browse_name: var.name.clone(),
                data_type: opcua_type_name(&var.value),
                writable: var.writable,
                value: var.value.clone(),
                description: var.description.clone(),
            })
            .collect();

        Self {
            namespace_uri,
            nodes,
        }
    }

    pub fn namespace_uri(&self) -> &str {
        &self.namespace_uri
    }

    pub fn nodes(&self) -> &[OpcUaNodeSpec] {
        &self.nodes
    }

    pub fn csv_header() -> &'static str {
        "node_id,browse_name,data_type,writable,value,description"
    }

    pub fn csv_rows(&self) -> Vec<String> {
        self.nodes
            .iter()
            .map(|node| {
                format!(
                    "{},{},{},{},{},{}",
                    node.node_id,
                    node.browse_name,
                    node.data_type,
                    node.writable,
                    node.value,
                    node.description.replace(',', ";"),
                )
            })
            .collect()
    }
}

fn opcua_type_name(value: &PlcValue) -> &'static str {
    match value {
        PlcValue::Bool(_) => "Boolean",
        PlcValue::I64(_) => "Int64",
        PlcValue::U64(_) => "UInt64",
        PlcValue::F64(_) => "Double",
        PlcValue::Text(_) => "String",
    }
}

/// Minimal contract the concrete OPC UA server implementation must satisfy.
///
/// Sprint 2 should implement this trait for the selected server backend.
pub trait OpcUaServerBackend {
    type Error;

    fn load_address_space(&mut self, address_space: &OpcUaAddressSpace) -> Result<(), Self::Error>;
    fn serve_until_stopped(&mut self) -> Result<(), Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_image::{PlcValue, ProcessVariable};

    #[test]
    fn address_space_maps_process_variables() {
        let mut image = ProcessImage::new();
        image.insert(ProcessVariable::new("tank.level", PlcValue::F64(50.0)));
        image.insert(ProcessVariable::new("tank.running", PlcValue::Bool(true)));

        let address_space = OpcUaAddressSpace::from_process_image("urn:sdplc:test", &image);
        assert_eq!(address_space.nodes().len(), 2);
        assert!(
            address_space.nodes()[0]
                .node_id
                .starts_with("ns=2;s=SDPLC.")
        );
    }
}

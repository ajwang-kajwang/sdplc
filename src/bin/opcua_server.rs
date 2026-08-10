//! SD-PLC OPC UA server backend.
//!
//! This binary exposes the validation process image through a real OPC UA
//! endpoint using the pure Rust async-opcua server stack.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use opcua_client::{ClientBuilder, IdentityToken};
use opcua_server::address_space::{AccessLevel, NodeBase, ObjectBuilder, Variable};
use opcua_server::diagnostics::NamespaceMetadata;
use opcua_server::node_manager::memory::{SimpleNodeManager, simple_node_manager};
use opcua_server::{ANONYMOUS_USER_TOKEN_ID, ServerBuilder};
use opcua_types::{
    AttributeId, BrowseDescription, BrowseDescriptionResultMask, BrowseDirection, BuildInfo,
    DataValue, DateTime, EndpointDescription, MessageSecurityMode, NodeId, NumericRange, ObjectId,
    ReadValueId, ReferenceTypeId, StatusCode, TimestampsToReturn, UserTokenPolicy, Variant,
    WriteValue,
};
use sdplc::opcua_bridge::OpcUaAddressSpace;
use sdplc::process_image::{PlcValue, ProcessImage, ProcessVariable};
use sdplc::simulation::FlotationTankSim;
use sdplc::timing::ScanTiming;

const NAMESPACE_URI: &str = "urn:sdplc:opcua";

#[derive(Debug, Clone)]
struct Config {
    host: String,
    port: u16,
    scan_time_ms: u64,
    output_dir: PathBuf,
    duration_seconds: Option<u64>,
    self_test: bool,
    read_count: usize,
    write_count: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 4855,
            scan_time_ms: 10,
            output_dir: PathBuf::from("results"),
            duration_seconds: None,
            self_test: false,
            read_count: 1000,
            write_count: 100,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = parse_args();
    fs::create_dir_all(&config.output_dir)?;
    fs::create_dir_all("target/opcua/own")?;
    fs::create_dir_all("target/opcua/private")?;
    fs::create_dir_all("target/opcua/pki")?;

    let mut image = FlotationTankSim::default().seed_process_image();
    seed_runtime_nodes(&mut image);
    let image = Arc::new(Mutex::new(image));

    let endpoint = format!("opc.tcp://{}:{}/", config.host, config.port);
    let (server, handle) = ServerBuilder::new_anonymous("SD-PLC OPC UA")
        .host(config.host.clone())
        .port(config.port)
        .application_uri("urn:sdplc:opcua-server")
        .product_uri("urn:sdplc")
        .build_info(BuildInfo {
            product_uri: "urn:sdplc".into(),
            manufacturer_name: "SD-PLC".into(),
            product_name: "SD-PLC OPC UA Server".into(),
            software_version: env!("CARGO_PKG_VERSION").into(),
            build_number: "sprint3".into(),
            build_date: DateTime::now(),
        })
        .certificate_path("target/opcua/own/cert.der")
        .private_key_path("target/opcua/private/private.pem")
        .pki_dir("target/opcua/pki")
        .create_sample_keypair(true)
        .trust_client_certs(true)
        .with_node_manager(simple_node_manager(
            NamespaceMetadata {
                namespace_uri: NAMESPACE_URI.to_string(),
                ..Default::default()
            },
            "sdplc",
        ))
        .build()?;

    let manager = handle
        .node_managers()
        .get_by_name::<SimpleNodeManager>("sdplc")
        .ok_or("SD-PLC node manager was not registered")?;
    let namespace_index = handle
        .get_namespace_index(NAMESPACE_URI)
        .ok_or("SD-PLC namespace was not registered")?;

    install_address_space(&manager, namespace_index, image.clone())?;
    write_evidence(&config.output_dir, &endpoint, &image.lock().unwrap())?;

    start_simulation_loop(image.clone(), Duration::from_millis(config.scan_time_ms));

    if config.self_test {
        let handle = handle.clone();
        let endpoint = endpoint.clone();
        let output_dir = config.output_dir.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(750)).await;
            match run_wire_self_test(&endpoint, output_dir, config.read_count, config.write_count)
                .await
            {
                Ok(()) => println!("OPC UA wire self-test passed"),
                Err(err) => eprintln!("OPC UA wire self-test failed: {err}"),
            }
            handle.cancel();
        });
    } else if let Some(seconds) = config.duration_seconds {
        let handle = handle.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(seconds)).await;
            handle.cancel();
        });
    } else {
        let handle = handle.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                handle.cancel();
            }
        });
    }

    println!("SD-PLC OPC UA server listening at {endpoint}");
    println!("Namespace: {NAMESPACE_URI}");
    println!("Anonymous endpoint token: {ANONYMOUS_USER_TOKEN_ID}");
    println!("Wrote evidence files to {}", config.output_dir.display());

    server.run().await?;
    Ok(())
}

async fn run_wire_self_test(
    endpoint: &str,
    output_dir: PathBuf,
    read_count: usize,
    write_count: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    fs::create_dir_all("target/opcua_client/own")?;
    fs::create_dir_all("target/opcua_client/private")?;
    fs::create_dir_all("target/opcua_client/pki")?;

    let mut client = ClientBuilder::new()
        .application_name("SD-PLC OPC UA Smoke Client")
        .application_uri("urn:sdplc:opcua-smoke-client")
        .certificate_path("target/opcua_client/own/cert.der")
        .private_key_path("target/opcua_client/private/private.pem")
        .pki_dir("target/opcua_client/pki")
        .create_sample_keypair(true)
        .trust_server_certs(true)
        .session_retry_limit(3)
        .client()
        .map_err(|errors| errors.join("; "))?;

    let endpoint_description: EndpointDescription = (
        endpoint,
        "None",
        MessageSecurityMode::None,
        UserTokenPolicy::anonymous(),
    )
        .into();
    let (session, event_loop) = client
        .connect_to_matching_endpoint(endpoint_description, IdentityToken::Anonymous)
        .await
        .map_err(|err| format!("connect failed: {err}"))?;
    let event_loop_handle = event_loop.spawn();
    session.wait_for_connection().await;

    let browse = BrowseDescription {
        node_id: ObjectId::ObjectsFolder.into(),
        browse_direction: BrowseDirection::Forward,
        reference_type_id: ReferenceTypeId::HierarchicalReferences.into(),
        include_subtypes: true,
        node_class_mask: 0,
        result_mask: BrowseDescriptionResultMask::all().bits(),
    };
    let browse_results = session
        .browse(&[browse], 100, None)
        .await
        .map_err(|err| format!("browse failed: {err}"))?;
    let browse_count = browse_results
        .first()
        .and_then(|result| result.references.as_ref())
        .map(|references| references.len())
        .unwrap_or(0);

    let read_nodes = [
        "SDPLC.tank.level",
        "SDPLC.tank.air_flow",
        "SDPLC.tank.feed_flow",
        "SDPLC.tank.tailings_flow",
        "SDPLC.tank.concentrate_grade",
        "SDPLC.tank.emergency_stop",
        "SDPLC.tank.motor_running",
        "SDPLC.runtime.cycle",
        "SDPLC.runtime.avg_exec_us",
        "SDPLC.runtime.max_jitter_us",
    ];
    let read_ids: Vec<ReadValueId> = read_nodes
        .iter()
        .map(|name| NodeId::new(2, *name).into())
        .collect();
    let before_values = session
        .read(&read_ids, TimestampsToReturn::Both, 0.0)
        .await
        .map_err(|err| format!("read failed: {err}"))?;

    let write_node = NodeId::new(2, "SDPLC.tank.air_flow");
    let write_statuses = session
        .write(&[WriteValue {
            node_id: write_node.clone(),
            attribute_id: AttributeId::Value as u32,
            index_range: NumericRange::None,
            value: DataValue::value_only(45.5_f64),
        }])
        .await
        .map_err(|err| format!("write failed: {err}"))?;
    if !write_statuses.iter().all(StatusCode::is_good) {
        return Err(format!("write returned {:?}", write_statuses).into());
    }

    let after_values = session
        .read(
            &[ReadValueId::from(write_node)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .map_err(|err| format!("read-back failed: {err}"))?;

    let mut rows = vec!["operation,node_id,status,value".to_string()];
    rows.push(format!(
        "browse,ObjectsFolder,Good,{browse_count} references"
    ));
    for (node, value) in read_nodes.iter().zip(before_values.iter()) {
        rows.push(format!("read,ns=2;s={},{}", node, data_value_csv(value)));
    }
    for status in write_statuses {
        rows.push(format!(
            "write,ns=2;s=SDPLC.tank.air_flow,{},45.500000",
            status
        ));
    }
    if let Some(value) = after_values.first() {
        rows.push(format!(
            "read_back,ns=2;s=SDPLC.tank.air_flow,{}",
            data_value_csv(value)
        ));
    }
    fs::write(
        output_dir.join("opcua_client_smoke.csv"),
        rows.join("\n") + "\n",
    )?;

    write_latency_benchmarks(&session, &output_dir, read_count, write_count).await?;

    let _ = session.disconnect().await;
    let _ = event_loop_handle.await;
    Ok(())
}

async fn write_latency_benchmarks(
    session: &opcua_client::Session,
    output_dir: &PathBuf,
    read_count: usize,
    write_count: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let read_node = NodeId::new(2, "SDPLC.tank.level");
    let read_id = ReadValueId::from(read_node.clone());
    let mut read_rows = vec!["sample,operation,node_id,status,latency_us,value".to_string()];

    for sample in 0..read_count {
        let started = Instant::now();
        let values = session
            .read(&[read_id.clone()], TimestampsToReturn::Both, 0.0)
            .await
            .map_err(|err| format!("read benchmark failed: {err}"))?;
        let latency_us = started.elapsed().as_secs_f64() * 1_000_000.0;
        let value = values.first().cloned().unwrap_or_default();
        read_rows.push(format!(
            "{sample},read,ns=2;s=SDPLC.tank.level,{},{latency_us:.3}",
            data_value_csv(&value)
        ));
    }

    let write_node = NodeId::new(2, "SDPLC.tank.air_flow");
    let mut write_rows = vec!["sample,operation,node_id,status,latency_us,value".to_string()];

    for sample in 0..write_count {
        let value = 42.5_f64 + (sample % 20) as f64 * 0.1;
        let started = Instant::now();
        let statuses = session
            .write(&[WriteValue {
                node_id: write_node.clone(),
                attribute_id: AttributeId::Value as u32,
                index_range: NumericRange::None,
                value: DataValue::value_only(value),
            }])
            .await
            .map_err(|err| format!("write benchmark failed: {err}"))?;
        let latency_us = started.elapsed().as_secs_f64() * 1_000_000.0;
        let status = statuses
            .first()
            .cloned()
            .unwrap_or(StatusCode::BadUnexpectedError);
        write_rows.push(format!(
            "{sample},write,ns=2;s=SDPLC.tank.air_flow,{status},{latency_us:.3},{value:.6}"
        ));
    }

    fs::write(
        output_dir.join("opcua_read_latency.csv"),
        read_rows.join("\n") + "\n",
    )?;
    fs::write(
        output_dir.join("opcua_write_latency.csv"),
        write_rows.join("\n") + "\n",
    )?;

    Ok(())
}

fn seed_runtime_nodes(image: &mut ProcessImage) {
    image.insert(
        ProcessVariable::new("runtime.cycle", PlcValue::U64(0))
            .read_only()
            .with_description("Runtime scan cycle count"),
    );
    image.insert(
        ProcessVariable::new("runtime.avg_exec_us", PlcValue::F64(0.0))
            .read_only()
            .with_description("Average scan execution time in microseconds"),
    );
    image.insert(
        ProcessVariable::new("runtime.max_jitter_us", PlcValue::F64(0.0))
            .read_only()
            .with_description("Maximum scan jitter in microseconds"),
    );
}

fn install_address_space(
    manager: &SimpleNodeManager,
    namespace_index: u16,
    image: Arc<Mutex<ProcessImage>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let sdplc_folder = NodeId::new(namespace_index, "SDPLC");
    let tank_folder = NodeId::new(namespace_index, "SDPLC.tank");
    let runtime_folder = NodeId::new(namespace_index, "SDPLC.runtime");

    {
        let mut address_space = manager.address_space().write();
        ObjectBuilder::new(&sdplc_folder, "SDPLC", "SDPLC")
            .is_folder()
            .organized_by(ObjectId::ObjectsFolder)
            .insert(&mut *address_space);
        ObjectBuilder::new(&tank_folder, "tank", "tank")
            .is_folder()
            .organized_by(sdplc_folder.clone())
            .insert(&mut *address_space);
        ObjectBuilder::new(&runtime_folder, "runtime", "runtime")
            .is_folder()
            .organized_by(sdplc_folder.clone())
            .insert(&mut *address_space);

        for variable in image.lock().unwrap().iter() {
            let node_id = node_id(namespace_index, &variable.name);
            let browse_name = browse_name(&variable.name);
            let parent = if variable.name.starts_with("runtime.") {
                runtime_folder.clone()
            } else {
                tank_folder.clone()
            };

            let mut node = Variable::new(
                &node_id,
                browse_name,
                browse_name,
                plc_value_to_variant(&variable.value),
            );
            node.set_description(variable.description.clone().into());
            if variable.writable {
                node.set_writable(true);
                node.set_user_access_level(node.user_access_level() | AccessLevel::CURRENT_WRITE);
            }
            address_space.insert::<_, NodeId>(node, None);
            address_space.insert_reference(&parent, &node_id, ReferenceTypeId::HasComponent);
        }
    }

    for variable in image.lock().unwrap().iter() {
        let node_id = node_id(namespace_index, &variable.name);
        let name = variable.name.clone();
        let image_for_read = image.clone();
        manager
            .inner()
            .add_read_callback(node_id.clone(), move |_, _, _| {
                let image = image_for_read
                    .lock()
                    .map_err(|_| StatusCode::BadInternalError)?;
                let value = image.get(&name).ok_or(StatusCode::BadNodeIdUnknown)?;
                Ok(DataValue::new_now(plc_value_to_variant(value)))
            });

        if variable.writable {
            let name = variable.name.clone();
            let expected = variable.value.clone();
            let image_for_write = image.clone();
            manager
                .inner()
                .add_write_callback(node_id, move |value, _| {
                    let Some(variant) = value.value else {
                        return StatusCode::BadNothingToDo;
                    };
                    let Some(value) = variant_to_plc_value(&variant, &expected) else {
                        return StatusCode::BadTypeMismatch;
                    };
                    match image_for_write.lock() {
                        Ok(mut image) => image
                            .set(&name, value)
                            .map(|_| StatusCode::Good)
                            .unwrap_or(StatusCode::BadNotWritable),
                        Err(_) => StatusCode::BadInternalError,
                    }
                });
        }
    }

    Ok(())
}

fn start_simulation_loop(image: Arc<Mutex<ProcessImage>>, scan_duration: Duration) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(scan_duration);
        let mut sim = FlotationTankSim::default();
        let mut timing = ScanTiming::new(scan_duration);
        let mut cycle = 0_u64;

        loop {
            interval.tick().await;
            let cycle_start = std::time::Instant::now();
            if let Ok(mut image) = image.lock() {
                sim.load_from_image(&image);
                sim.step(scan_duration.as_secs_f64());
                sim.write_to_image(&mut image);
                let exec_time = cycle_start.elapsed();
                cycle += 1;
                timing.record_cycle(exec_time, cycle_start.elapsed());
                let _ = image.refresh("runtime.cycle", PlcValue::U64(cycle));
                let _ = image.refresh("runtime.avg_exec_us", PlcValue::F64(timing.avg_exec_us()));
                let _ = image.refresh(
                    "runtime.max_jitter_us",
                    PlcValue::F64(timing.max_jitter_us()),
                );
            }
        }
    });
}

fn write_evidence(
    output_dir: &PathBuf,
    endpoint: &str,
    image: &ProcessImage,
) -> Result<(), Box<dyn std::error::Error>> {
    let address_space = OpcUaAddressSpace::from_process_image(NAMESPACE_URI, image);
    let mut address_rows = vec![OpcUaAddressSpace::csv_header().to_string()];
    address_rows.extend(address_space.csv_rows());
    fs::write(
        output_dir.join("opcua_address_space.csv"),
        address_rows.join("\n") + "\n",
    )?;

    let mut read_rows = vec!["node_id,browse_name,data_type,writable,value".to_string()];
    read_rows.extend(address_space.nodes().iter().map(|node| {
        format!(
            "{},{},{},{},{}",
            node.node_id, node.browse_name, node.data_type, node.writable, node.value
        )
    }));
    fs::write(
        output_dir.join("opcua_read_values.csv"),
        read_rows.join("\n") + "\n",
    )?;

    let writable = address_space
        .nodes()
        .iter()
        .find(|node| node.writable)
        .map(|node| node.node_id.as_str())
        .unwrap_or("none");
    fs::write(
        output_dir.join("opcua_test_notes.md"),
        format!(
            "# SD-PLC OPC UA Sprint 3 Notes\n\n\
             Endpoint: `{endpoint}`\n\n\
             Namespace URI: `{NAMESPACE_URI}`\n\n\
             Backend: `async-opcua-server` pure Rust OPC UA server.\n\n\
             Browse root: `Objects/SDPLC`.\n\n\
             Minimum nodes are exposed under `SDPLC/tank` and `SDPLC/runtime`.\n\n\
             Writable validation node: `{writable}`. Write through an OPC UA client and read the same node back; server write callbacks update the shared `ProcessImage`.\n\n\
             Run `cargo run --bin opcua_server -- examples/flotation_tank.st --scan-time=10 --self-test` to create `results/opcua_client_smoke.csv` with wire-level browse/read/write evidence.\n"
        ),
    )?;

    Ok(())
}

fn data_value_csv(value: &DataValue) -> String {
    let status = value.status.unwrap_or(StatusCode::Good);
    let display = value
        .value
        .as_ref()
        .map(variant_display)
        .unwrap_or_else(|| "<empty>".to_string());
    format!("{status},{display}")
}

fn plc_value_to_variant(value: &PlcValue) -> Variant {
    match value {
        PlcValue::Bool(value) => Variant::from(*value),
        PlcValue::I64(value) => Variant::from(*value),
        PlcValue::U64(value) => Variant::from(*value),
        PlcValue::F64(value) => Variant::from(*value),
        PlcValue::Text(value) => Variant::from(value.clone()),
    }
}

fn variant_to_plc_value(variant: &Variant, expected: &PlcValue) -> Option<PlcValue> {
    match (variant, expected) {
        (Variant::Boolean(value), PlcValue::Bool(_)) => Some(PlcValue::Bool(*value)),
        (Variant::Double(value), PlcValue::F64(_)) => Some(PlcValue::F64(*value)),
        (Variant::Float(value), PlcValue::F64(_)) => Some(PlcValue::F64(*value as f64)),
        (Variant::Int64(value), PlcValue::I64(_)) => Some(PlcValue::I64(*value)),
        (Variant::Int32(value), PlcValue::I64(_)) => Some(PlcValue::I64(*value as i64)),
        (Variant::Int16(value), PlcValue::I64(_)) => Some(PlcValue::I64(*value as i64)),
        (Variant::SByte(value), PlcValue::I64(_)) => Some(PlcValue::I64(*value as i64)),
        (Variant::UInt64(value), PlcValue::U64(_)) => Some(PlcValue::U64(*value)),
        (Variant::UInt32(value), PlcValue::U64(_)) => Some(PlcValue::U64(*value as u64)),
        (Variant::UInt16(value), PlcValue::U64(_)) => Some(PlcValue::U64(*value as u64)),
        (Variant::Byte(value), PlcValue::U64(_)) => Some(PlcValue::U64(*value as u64)),
        (Variant::String(value), PlcValue::Text(_)) => {
            Some(PlcValue::Text(value.value().clone().unwrap_or_default()))
        }
        _ => None,
    }
}

fn variant_display(value: &Variant) -> String {
    match value {
        Variant::Boolean(value) => value.to_string(),
        Variant::SByte(value) => value.to_string(),
        Variant::Byte(value) => value.to_string(),
        Variant::Int16(value) => value.to_string(),
        Variant::UInt16(value) => value.to_string(),
        Variant::Int32(value) => value.to_string(),
        Variant::UInt32(value) => value.to_string(),
        Variant::Int64(value) => value.to_string(),
        Variant::UInt64(value) => value.to_string(),
        Variant::Float(value) => format!("{value:.6}"),
        Variant::Double(value) => format!("{value:.6}"),
        Variant::String(value) => value.value().clone().unwrap_or_default(),
        other => format!("{other:?}"),
    }
}

fn node_id(namespace_index: u16, process_name: &str) -> NodeId {
    NodeId::new(namespace_index, format!("SDPLC.{process_name}"))
}

fn browse_name(process_name: &str) -> &str {
    process_name.rsplit('.').next().unwrap_or(process_name)
}

fn parse_args() -> Config {
    let mut config = Config::default();

    for arg in env::args().skip(1) {
        if let Some(value) = arg.strip_prefix("--host=") {
            config.host = value.to_string();
        } else if let Some(value) = arg.strip_prefix("--port=") {
            config.port = value.parse().expect("--port must be a TCP port");
        } else if let Some(value) = arg.strip_prefix("--scan-time=") {
            config.scan_time_ms = value
                .parse()
                .expect("--scan-time must be a positive integer");
        } else if let Some(value) = arg.strip_prefix("--duration=") {
            config.duration_seconds = Some(value.parse().expect("--duration must be seconds"));
        } else if let Some(value) = arg.strip_prefix("--out=") {
            config.output_dir = PathBuf::from(value);
        } else if let Some(value) = arg.strip_prefix("--read-count=") {
            config.read_count = value.parse().expect("--read-count must be a count");
        } else if let Some(value) = arg.strip_prefix("--write-count=") {
            config.write_count = value.parse().expect("--write-count must be a count");
        } else if arg == "--self-test" {
            config.self_test = true;
        } else if arg == "--help" || arg == "-h" {
            print_help_and_exit();
        } else if !arg.starts_with('-') {
            // Reserved for the Sunday demo shape:
            // opcua_server examples/flotation_tank.st --scan-time=10
        } else {
            eprintln!("unknown argument: {arg}");
            print_help_and_exit();
        }
    }

    config
}

fn print_help_and_exit() -> ! {
    println!("SD-PLC OPC UA server\n");
    println!("USAGE:");
    println!("  cargo run --bin opcua_server -- examples/flotation_tank.st --scan-time=10");
    println!();
    println!("OPTIONS:");
    println!("  --host=ADDR       Bind host, default 127.0.0.1");
    println!("  --port=PORT       Bind port, default 4855");
    println!("  --scan-time=MS    Simulation scan period, default 10");
    println!("  --duration=SEC    Stop automatically after SEC seconds");
    println!("  --self-test       Run a local OPC UA browse/read/write smoke client");
    println!("  --read-count=N    OPC UA self-test read latency samples, default 1000");
    println!("  --write-count=N   OPC UA self-test write latency samples, default 100");
    println!("  --out=DIR         Output directory, default results");
    std::process::exit(0);
}

# SD-PLC OPC UA Notes

Endpoint: `opc.tcp://127.0.0.1:4855/`

Namespace URI: `urn:sdplc:opcua`

Backend: `async-opcua-server` pure Rust OPC UA server.

Browse root: `Objects/SDPLC`.

Minimum nodes are exposed under `SDPLC/tank` and `SDPLC/runtime`.

Writable validation node: `ns=2;s=SDPLC.tank.air_flow`. Write through an OPC UA client and read the same node back; server write callbacks update the shared `ProcessImage`.

Run `cargo run --bin opcua_server -- examples/flotation_tank.st --scan-time=10 --self-test` to create `results/opcua_client_smoke.csv` with wire-level browse/read/write evidence.

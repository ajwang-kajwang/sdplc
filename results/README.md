# Results Folder Index

This folder is ignored by Git and holds local evidence artefacts.

```text
compiler_ir/          LLVM `.ll` and `.bc` output grouped by source program
compiler_benchmark/   Compiler phase timing CSVs
runtime/              Runtime scan timing and final-value CSVs
simulation/           Flotation-tank simulation timing and telemetry
opcua/                OPC UA address-space, smoke-test, and latency evidence
validation/           Validation-pack Markdown summaries
```

Prefer run-specific or source-specific subfolders when creating new evidence.

/**
 * Public re-export of the Rust-owned run-event contract.
 *
 * `src/types/generated/*` is written by `cargo test export_bindings` from
 * `src-tauri/src/provider_event.rs` (ts-rs). Do not hand-edit the generated
 * files — CI regenerates them and fails on drift. This barrel is the only
 * hand-written file here so consumers have one import path.
 */
export type { Capability } from "./generated/Capability";
export type { CapabilitySet } from "./generated/CapabilitySet";
export type { ContentKind } from "./generated/ContentKind";
export type { DecisionKind } from "./generated/DecisionKind";
export type { DecisionState } from "./generated/DecisionState";
export type { Provider } from "./generated/Provider";
export type { RawProviderRef } from "./generated/RawProviderRef";
export type { RunEvent } from "./generated/RunEvent";
export type { RunEventEnvelope } from "./generated/RunEventEnvelope";
export type { RunEventPayload } from "./generated/RunEventPayload";
export type { TerminalOutcome } from "./generated/TerminalOutcome";

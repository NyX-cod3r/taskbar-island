pub mod audio;
pub mod call;
// Pure call-domain seams: state machine, providers, masking. The call UI
// wiring (A3) lands on top of this; until then the module is test-only.
#[allow(dead_code)]
pub mod call_state;
pub mod media;
pub mod shelf;
pub mod timer;

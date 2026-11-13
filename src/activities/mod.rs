pub mod audio;
pub mod call;
// Call sources feeding the one call state machine (spec §3): the module owns
// the source trait, the hub and every observer (toast, notifications, the
// Phone Link window, the notification database).
pub mod callsrc;
// Pure call-domain seams: state machine, providers, masking.
#[allow(dead_code)]
pub mod call_state;
pub mod media;
pub mod shelf;
pub mod timer;

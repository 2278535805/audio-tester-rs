#![allow(unused_imports)]

#[cfg(target_os = "android")]
mod inner {
    pub use sasa::backend::oboe::{
        OboeBackend as AudioBackend,
        OboeRecorderBackend as AudioRecorderBackend,
        OboeSettings as Settings,
    };
    pub fn make_output() -> AudioBackend { AudioBackend::new(Settings::default()) }
    pub fn make_input() -> AudioRecorderBackend { AudioRecorderBackend::new(Settings::default()) }
}

#[cfg(not(target_os = "android"))]
mod inner {
    pub use sasa::backend::cpal::{
        CpalBackend as AudioBackend,
        CpalRecorderBackend as AudioRecorderBackend,
        CpalSettings as Settings,
    };
    pub fn make_output() -> AudioBackend { AudioBackend::new(Settings::default()) }
    pub fn make_input() -> AudioRecorderBackend { AudioRecorderBackend::new(Settings::default()) }
}

pub use inner::*;

#![allow(unused_imports)]

#[cfg(target_os = "android")]
mod inner {
    pub use sasa::backend::oboe::{
        OboeBackend as AudioBackend,
        OboeRecorderBackend as AudioRecorderBackend,
        OboeSettings, Usage, SharingMode, AudioApi, PerformanceMode
    };
    pub fn make_output() -> AudioBackend { AudioBackend::new(OboeSettings {
        buffer_size: None,
        performance_mode: PerformanceMode::LowLatency,
        audio_api: AudioApi::Unspecified,
        sharing_mode: SharingMode::Exclusive,
        usage: Usage::Game,
    }) }
    pub fn make_input() -> AudioRecorderBackend { AudioRecorderBackend::new(OboeSettings {
        buffer_size: None,
        performance_mode: PerformanceMode::LowLatency,
        audio_api: AudioApi::Unspecified,
        sharing_mode: SharingMode::Exclusive,
        usage: Usage::Game,
    }) }
}

#[cfg(not(target_os = "android"))]
mod inner {
    pub use sasa::backend::cpal::{
        CpalBackend as AudioBackend,
        CpalRecorderBackend as AudioRecorderBackend,
        CpalSettings,
    };
    pub fn make_output() -> AudioBackend { AudioBackend::new(CpalSettings::default()) }
    pub fn make_input() -> AudioRecorderBackend { AudioRecorderBackend::new(CpalSettings::default()) }
}

pub use inner::*;

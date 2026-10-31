//! A host for the widgets' tests, which records what they ask of it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nice_plug::context::gui::GuiContextInner;
use nice_plug::params::internals::ParamPtr;
use nice_plug::prelude::{PluginApi, PluginState};

#[derive(Default)]
pub struct Host {
    /// Every normalized value it was asked to set.
    pub requests: Mutex<Vec<f32>>,
    /// Every gesture's begin and end, in order.
    pub gestures: Arc<Mutex<Vec<&'static str>>>,
}

impl GuiContextInner for Host {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }
    unsafe fn raw_begin_set_parameter(&self, _: ParamPtr) {
        self.gestures.lock().expect("gestures").push("begin");
    }
    unsafe fn raw_set_parameter_normalized(&self, _: ParamPtr, value: f32) {
        self.requests.lock().expect("requests").push(value);
    }
    unsafe fn raw_end_set_parameter(&self, _: ParamPtr) {
        self.gestures.lock().expect("gestures").push("end");
    }
    fn get_state(&self) -> PluginState {
        PluginState {
            version: String::new(),
            params: BTreeMap::new(),
            fields: BTreeMap::new(),
        }
    }
    fn set_state(&self, _: PluginState) {}
    fn request_restart(&self) {}
}

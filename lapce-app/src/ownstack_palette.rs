use floem::reactive::{RwSignal, create_rw_signal};
use floem::prelude::{SignalGet, SignalUpdate};
use lapce_rpc::ownstack::OwnStackRpc;

use crate::window_tab::CommonData;

#[derive(Clone)]
pub struct OwnStackPaletteData {
    pub input: RwSignal<String>,
    pub active: RwSignal<bool>,
    common: CommonData,
}

impl OwnStackPaletteData {
    pub fn new(common: CommonData) -> Self {
        Self {
            input: create_rw_signal(String::new()),
            active: create_rw_signal(false),
            common,
        }
    }

    pub fn show(&self) {
        self.active.set(true);
        self.input.set(String::new());
    }

    pub fn hide(&self) {
        self.active.set(false);
    }

    pub fn submit(&self) {
        let prompt = self.input.get_untracked();
        if prompt.is_empty() {
            return;
        }

        // Send AI prompt via RPC
        let message = OwnStackRpc::AiPrompt { prompt };
        
        // TODO: Send via proxy RPC when bridge is fully integrated
        tracing::info!("OwnStack AI Prompt: {:?}", message);
        
        self.hide();
    }
}

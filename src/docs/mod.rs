//! The Sensity tool window: natives browser, runtime API, built-in events,
//! manifest directives, CfxLua syntax and the bundled guides. The page
//! (`web/`) is a thin client: searching and HTML building happen here.

pub mod data;

use crate::feature::{CommandCx, Feature};
use openidea_plugin::Error;
use openidea_plugin::views::{View, ViewEvent, Viewport};

/// The view id declared in plugin.toml.
pub const VIEW: &str = "docs";

#[derive(Default)]
pub struct Docs;

impl Docs {
    pub fn render(&self, _cx: &CommandCx, _view: &str, _viewport: Option<Viewport>) -> Result<View, Error> {
        Ok(View::message("The documentation is shown by the plugin's page."))
    }

    pub fn handle(&self, _cx: &CommandCx, _view: &str, _event: ViewEvent) -> Result<(), Error> {
        Ok(())
    }
}

impl Feature for Docs {}

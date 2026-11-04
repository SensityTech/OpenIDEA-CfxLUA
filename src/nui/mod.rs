//! NUI bridge: callbacks and messages between Lua client scripts and the page.

use crate::feature::Feature;

#[derive(Default)]
pub struct NuiFeature;

impl Feature for NuiFeature {}

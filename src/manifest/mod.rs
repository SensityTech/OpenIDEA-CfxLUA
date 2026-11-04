//! fxmanifest.lua: completion of directives, values, paths and dependencies; navigation; checks and quick fixes.

use crate::feature::Feature;

#[derive(Default)]
pub struct ManifestFeature;

impl Feature for ManifestFeature {}

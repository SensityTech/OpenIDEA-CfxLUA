//! Performance linter: per-frame loops, expensive calls, distance checks.

use crate::feature::Feature;

#[derive(Default)]
pub struct PerfFeature;

impl Feature for PerfFeature {}

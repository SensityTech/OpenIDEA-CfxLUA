//! CfxLua syntax checks: what the Cfx.re runtime accepts or silently misreads.

use crate::feature::Feature;

#[derive(Default)]
pub struct SyntaxFeature;

impl Feature for SyntaxFeature {}

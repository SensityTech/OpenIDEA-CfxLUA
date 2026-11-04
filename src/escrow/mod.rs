//! Asset Escrow: which files are encrypted, checks, the pre-upload report.

use crate::feature::Feature;

#[derive(Default)]
pub struct EscrowFeature;

impl Feature for EscrowFeature {}

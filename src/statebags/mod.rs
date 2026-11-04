//! State bags: keys written anywhere in the project, completed, navigated and checked where they are read.

use crate::feature::Feature;

#[derive(Default)]
pub struct StateBagsFeature;

impl Feature for StateBagsFeature {}

//! Cfx runtime API (Citizen, events, exports, state bags, json, msgpack, vectors…): bundled stubs, completion and documentation.

use crate::feature::Feature;

#[derive(Default)]
pub struct RuntimeFeature;

impl Feature for RuntimeFeature {}

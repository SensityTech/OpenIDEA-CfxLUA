use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Box3 {
    min: [f64; 3],
    max: [f64; 3],
}

impl Box3 {
    fn area(&self) -> f64 {
        (self.max[0] - self.min[0]) * (self.max[1] - self.min[1])
    }

    fn contains_xy(&self, x: f64, y: f64) -> bool {
        (self.min[0]..=self.max[0]).contains(&x) && (self.min[1]..=self.max[1]).contains(&y)
    }

    fn contains_z(&self, z: f64) -> bool {
        (self.min[2]..=self.max[2]).contains(&z)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Zone {
    pub name: String,
    pub display: String,
}

pub struct ZoneMap {
    zones: Vec<Zone>,
    boxes: Vec<(Box3, usize)>,
}

impl ZoneMap {
    pub fn parse(text: &str) -> Option<Self> {
        let Value::Array(items) = serde_json::from_str::<Value>(text).ok()? else { return None };
        let mut zones = Vec::new();
        let mut boxes = Vec::new();
        for item in &items {
            let Some(obj) = item.as_object() else { continue };
            let Some(name) = member(obj, "name").and_then(Value::as_str) else { continue };
            let display =
                member(obj, "displayName").and_then(Value::as_str).filter(|d| !d.trim().is_empty()).unwrap_or(name);
            let Some(bounds) = member(obj, "bounds").and_then(Value::as_array) else { continue };
            let found: Vec<Box3> = bounds.iter().filter_map(bounds_box).collect();
            if found.is_empty() {
                continue;
            }
            boxes.extend(found.into_iter().map(|b| (b, zones.len())));
            zones.push(Zone { name: name.to_owned(), display: display.to_owned() });
        }
        if zones.is_empty() {
            return None;
        }
        boxes.sort_by(|a, b| a.0.area().total_cmp(&b.0.area()));
        Some(Self { zones, boxes })
    }

    pub fn find(&self, x: f64, y: f64, z: Option<f64>) -> Option<&Zone> {
        let mut around = self.boxes.iter().filter(|(b, _)| b.contains_xy(x, y)).peekable();
        let smallest = around.peek()?.1;
        let by_height = z.and_then(|z| around.find(|(b, _)| b.contains_z(z)).map(|(_, i)| *i));
        Some(&self.zones[by_height.unwrap_or(smallest)])
    }
}

fn member<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    obj.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
}

fn bounds_box(bound: &Value) -> Option<Box3> {
    let obj = bound.as_object()?;
    let min = member(obj, "minimum")?.as_object()?;
    let max = member(obj, "maximum")?.as_object()?;
    let n = |o: &Map<String, Value>, k: &str| member(o, k).and_then(Value::as_f64);
    Some(Box3 {
        min: [n(min, "x")?, n(min, "y")?, n(min, "z").unwrap_or(-1e9)],
        max: [n(max, "x")?, n(max, "y")?, n(max, "z").unwrap_or(1e9)],
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const ZONES: &str = r#"[
      {"Name": "DAVIS", "DisplayName": "Davis", "Bounds": [
        {"Minimum": {"X": 0.0, "Y": -1700.0, "Z": -100.0}, "Maximum": {"X": 300.0, "Y": -1400.0, "Z": 500.0}}
      ]},
      {"name": "STRAW", "displayName": "Strawberry", "bounds": [
        {"minimum": {"x": 0.0, "y": -1400.0, "z": -100.0}, "maximum": {"x": 300.0, "y": -1100.0, "z": 500.0}}
      ]},
      {"Name": "LOSSANTOS", "DisplayName": "Los Santos", "Bounds": [
        {"Minimum": {"X": -1000.0, "Y": -3000.0, "Z": -100.0}, "Maximum": {"X": 1000.0, "Y": 0.0, "Z": 1000.0}}
      ]}
    ]"#;

    #[test]
    fn zones() {
        let zones = ZoneMap::parse(ZONES).unwrap();
        let name = |x, y, z| zones.find(x, y, z).map(|z| z.display.clone());
        assert_eq!(name(100.0, -1600.0, Some(30.0)).as_deref(), Some("Davis"));
        assert_eq!(name(100.0, -1300.0, Some(30.0)).as_deref(), Some("Strawberry"));
        assert_eq!(name(900.0, -900.0, None).as_deref(), Some("Los Santos"));
        assert_eq!(name(100.0, -1600.0, Some(900.0)).as_deref(), Some("Los Santos"));
        assert_eq!(name(100.0, -1600.0, Some(5000.0)).as_deref(), Some("Davis"));
        assert_eq!(name(5000.0, 5000.0, Some(0.0)), None);
        assert!(ZoneMap::parse("not json").is_none());
        assert!(ZoneMap::parse("[{\"Name\": \"X\"}]").is_none());
    }
}

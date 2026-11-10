//! Mapbox Geobuf encoder (six decimal places, no reprojection).
//!
//! Wire layout and coordinate algorithm follow mapbox/geobuf (ISC; see
//! third_party/geobuf). A small wire writer is used because Mapbox's official
//! encoder supports null geometry by omitting a field marked `required` in its
//! proto2 schema. Rewriting that schema as proto3 would change its defaults.

use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
#[error("invalid GeoJSON: {0}")]
pub struct Error(String);
type Result<T> = std::result::Result<T, Error>;
fn bad(s: &str) -> Error {
    Error(s.into())
}
fn list(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().ok_or_else(|| bad("expected array"))
}
fn obj(v: &Value) -> Result<&Map<String, Value>> {
    v.as_object().ok_or_else(|| bad("expected object"))
}
fn kind(v: &Value) -> Result<&str> {
    v["type"].as_str().ok_or_else(|| bad("missing type"))
}

/// Encodes a GeoJSON object. Rejects invalid structures, mixed coordinate
/// dimensions and coordinates outside JavaScript's safe integer precision.
/// Properties and foreign members are retained, including nested JSON values.
pub fn encode(value: &Value) -> Result<Vec<u8>> {
    let mut encoder = Encoder {
        keys: BTreeMap::new(),
        dimensions: None,
    };
    encoder.analyze(value, 0)?;
    let mut output = Wire::default();
    // Key indices follow insertion order; BTreeMap is only used for lookup.
    let mut keys: Vec<_> = encoder.keys.iter().collect();
    keys.sort_unstable_by_key(|(_, index)| **index);
    for (key, _) in keys {
        output.string(1, key);
    }
    if encoder.dimensions.unwrap_or(2) != 2 {
        output.uint(2, encoder.dimensions.unwrap_or(2) as u64);
    }
    // precision=6 is the proto2 default and therefore omitted.
    let tag = match kind(value)? {
        "FeatureCollection" => 4,
        "Feature" => 5,
        _ => 6,
    };
    output.message(tag, encoder.node(value)?);
    Ok(output.0)
}

struct Encoder<'a> {
    keys: BTreeMap<&'a str, u32>,
    dimensions: Option<usize>,
}
impl<'a> Encoder<'a> {
    fn key(&mut self, key: &'a str) -> Result<()> {
        if !self.keys.contains_key(key) {
            let index =
                u32::try_from(self.keys.len()).map_err(|_| bad("too many property keys"))?;
            self.keys.insert(key, index);
        }
        Ok(())
    }
    fn analyze(&mut self, v: &'a Value, depth: usize) -> Result<()> {
        if depth > 64 {
            return Err(bad("geometry nesting exceeds 64"));
        }
        let members = obj(v)?;
        let t = kind(v)?;
        match t {
            "FeatureCollection" => {
                for feature in list(&v["features"])? {
                    if kind(feature)? != "Feature" {
                        return Err(bad("collection member must be Feature"));
                    }
                    self.analyze(feature, depth + 1)?;
                }
            }
            "Feature" => {
                if !members.contains_key("geometry") {
                    return Err(bad("missing geometry"));
                }
                if !v["geometry"].is_null() {
                    if matches!(kind(&v["geometry"])?, "Feature" | "FeatureCollection") {
                        return Err(bad("invalid feature geometry"));
                    }
                    self.analyze(&v["geometry"], depth + 1)?;
                }
                if !v["properties"].is_null() {
                    for key in obj(&v["properties"])?.keys() {
                        self.key(key)?;
                    }
                } else if members.contains_key("properties") {
                    // Preserve explicit null via a custom member, supported by the decoder.
                    self.key("properties")?;
                }
            }
            "GeometryCollection" => {
                for geometry in list(&v["geometries"])? {
                    if matches!(kind(geometry)?, "Feature" | "FeatureCollection") {
                        return Err(bad("invalid collection geometry"));
                    }
                    self.analyze(geometry, depth + 1)?;
                }
            }
            "Point" => self.point(&v["coordinates"])?,
            "MultiPoint" => self.line(&v["coordinates"], false, 1)?,
            "LineString" => self.line(&v["coordinates"], false, 2)?,
            "MultiLineString" | "Polygon" => {
                let lines = list(&v["coordinates"])?;
                if lines.is_empty() {
                    return Err(bad("empty coordinate structure"));
                }
                for line in lines {
                    self.line(line, t == "Polygon", if t == "Polygon" { 4 } else { 2 })?;
                }
            }
            "MultiPolygon" => {
                let polygons = list(&v["coordinates"])?;
                if polygons.is_empty() {
                    return Err(bad("empty multipolygon"));
                }
                for polygon in polygons {
                    let rings = list(polygon)?;
                    if rings.is_empty() {
                        return Err(bad("empty polygon"));
                    }
                    for ring in rings {
                        self.line(ring, true, 4)?;
                    }
                }
            }
            _ => return Err(bad("unsupported geometry type")),
        }
        for key in members.keys() {
            if !special(key, t) {
                self.key(key)?;
            }
        }
        Ok(())
    }
    fn point(&mut self, v: &Value) -> Result<()> {
        let point = list(v)?;
        if !(2..=16).contains(&point.len()) {
            return Err(bad("coordinate dimension must be 2..16"));
        }
        if self.dimensions.is_some_and(|d| d != point.len()) {
            return Err(bad("mixed coordinate dimensions"));
        }
        self.dimensions = Some(point.len());
        for n in point {
            quantize(n)?;
        }
        Ok(())
    }
    fn line(&mut self, v: &Value, closed: bool, minimum: usize) -> Result<()> {
        let line = list(v)?;
        if line.len() < minimum {
            return Err(bad("too few coordinates"));
        }
        if closed && line.first() != line.last() {
            return Err(bad("polygon ring must be closed"));
        }
        for point in line {
            self.point(point)?;
        }
        Ok(())
    }
    fn node(&self, v: &Value) -> Result<Wire> {
        let mut out = Wire::default();
        let t = kind(v)?;
        match t {
            "FeatureCollection" => {
                for feature in list(&v["features"])? {
                    out.message(1, self.node(feature)?);
                }
            }
            "Feature" => {
                if !v["geometry"].is_null() {
                    out.message(1, self.node(&v["geometry"])?);
                }
                if let Some(id) = v.get("id") {
                    if let Some(n) = id.as_i64() {
                        out.sint(12, n);
                    } else if let Some(s) = id.as_str() {
                        out.string(11, s);
                    } else {
                        return Err(bad("feature ID must be a string or signed 64-bit integer"));
                    }
                }
                if let Some(properties) = v["properties"].as_object() {
                    self.properties(
                        &mut out,
                        properties.iter().map(|(k, v)| (k.as_str(), v)),
                        14,
                    )?;
                } else if v.get("properties").is_some_and(Value::is_null) {
                    self.properties(&mut out, std::iter::once(("properties", &Value::Null)), 15)?;
                }
            }
            "GeometryCollection" => {
                out.uint(1, 6);
                for g in list(&v["geometries"])? {
                    out.message(4, self.node(g)?);
                }
            }
            _ => {
                let code = match t {
                    "Point" => 0,
                    "MultiPoint" => 1,
                    "LineString" => 2,
                    "MultiLineString" => 3,
                    "Polygon" => 4,
                    "MultiPolygon" => 5,
                    _ => return Err(bad("unsupported geometry")),
                };
                out.uint(1, code);
                let coords = list(&v["coordinates"])?;
                let mut values = Vec::new();
                let mut lengths = Vec::new();
                match t {
                    "Point" => {
                        for n in coords {
                            values.push(quantize(n)?);
                        }
                    }
                    "MultiPoint" | "LineString" => self.deltas(coords, false, &mut values)?,
                    "MultiLineString" | "Polygon" => {
                        let closed = t == "Polygon";
                        if coords.len() != 1 {
                            for line in coords {
                                lengths.push((list(line)?.len() - usize::from(closed)) as u64);
                            }
                        }
                        for line in coords {
                            self.deltas(list(line)?, closed, &mut values)?;
                        }
                    }
                    "MultiPolygon" => {
                        if coords.len() != 1 || list(&coords[0])?.len() != 1 {
                            lengths.push(coords.len() as u64);
                            for polygon in coords {
                                let rings = list(polygon)?;
                                lengths.push(rings.len() as u64);
                                for ring in rings {
                                    lengths.push((list(ring)?.len() - 1) as u64);
                                }
                            }
                        }
                        for polygon in coords {
                            for ring in list(polygon)? {
                                self.deltas(list(ring)?, true, &mut values)?;
                            }
                        }
                    }
                    _ => unreachable!(),
                }
                if !lengths.is_empty() {
                    out.packed(2, lengths);
                }
                out.packed(3, values.into_iter().map(zigzag));
            }
        }
        self.properties(
            &mut out,
            obj(v)?
                .iter()
                .filter(|(k, _)| !special(k, t))
                .map(|(k, v)| (k.as_str(), v)),
            15,
        )?;
        Ok(out)
    }
    fn deltas(&self, line: &[Value], closed: bool, values: &mut Vec<i64>) -> Result<()> {
        let mut previous = vec![0_i64; self.dimensions.unwrap_or(2)];
        for point in &line[..line.len() - usize::from(closed)] {
            for (n, prev) in list(point)?.iter().zip(previous.iter_mut()) {
                let current = quantize(n)?;
                values.push(
                    current
                        .checked_sub(*prev)
                        .ok_or_else(|| bad("coordinate delta overflow"))?,
                );
                *prev = current;
            }
        }
        Ok(())
    }
    fn properties<'b>(
        &self,
        out: &mut Wire,
        properties: impl Iterator<Item = (&'b str, &'b Value)>,
        tag: u32,
    ) -> Result<()> {
        let mut indexes = Vec::new();
        for (index, (key, v)) in properties.enumerate() {
            let key = *self
                .keys
                .get(key)
                .ok_or_else(|| bad("missing property key"))?;
            let mut value = Wire::default();
            match v {
                Value::Null => {}
                Value::String(s) => value.string(1, s),
                Value::Bool(b) => value.uint(5, u64::from(*b)),
                Value::Number(n) => {
                    if let Some(n) = n.as_u64() {
                        value.uint(3, n);
                    } else if let Some(n) = n.as_i64() {
                        value.uint(4, n.unsigned_abs());
                    } else {
                        let n = n.as_f64().ok_or_else(|| bad("invalid property number"))?;
                        // JSON spellings such as 0.0 and 1007.0 are integers
                        // to Mapbox's JS encoder too; use its compact varint arm.
                        if n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0 {
                            value.uint(if n >= 0.0 { 3 } else { 4 }, n.abs() as u64);
                        } else {
                            value.key(2, 1);
                            value.0.extend_from_slice(&n.to_le_bytes());
                        }
                    }
                }
                _ => value.string(6, &v.to_string()),
            }
            out.message(13, value);
            indexes.push(u64::from(key));
            indexes.push(index as u64);
        }
        // Empty custom properties must not clear the decoder's value dictionary
        // or overwrite explicit null properties from the preceding group.
        if !indexes.is_empty() || tag == 14 {
            out.packed(tag, indexes);
        }
        Ok(())
    }
}

fn special(key: &str, t: &str) -> bool {
    key == "type"
        || match t {
            "FeatureCollection" => key == "features",
            "Feature" => matches!(key, "geometry" | "properties" | "id"),
            "GeometryCollection" => key == "geometries",
            _ => key == "coordinates",
        }
}
fn quantize(v: &Value) -> Result<i64> {
    let n = v
        .as_f64()
        .ok_or_else(|| bad("coordinate must be a number"))?;
    let scaled = (n * 1_000_000.0 + 0.5).floor();
    if !scaled.is_finite() || scaled.abs() > 9_007_199_254_740_991.0 {
        return Err(bad("coordinate exceeds safe integer range"));
    }
    Ok(scaled as i64)
}
fn zigzag(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

#[derive(Default)]
struct Wire(Vec<u8>);
impl Wire {
    fn varint(&mut self, mut n: u64) {
        while n >= 128 {
            self.0.push((n as u8 & 127) | 128);
            n >>= 7;
        }
        self.0.push(n as u8);
    }
    fn key(&mut self, tag: u32, wire: u32) {
        self.varint(u64::from((tag << 3) | wire));
    }
    fn uint(&mut self, tag: u32, n: u64) {
        self.key(tag, 0);
        self.varint(n);
    }
    fn sint(&mut self, tag: u32, n: i64) {
        self.uint(tag, zigzag(n));
    }
    fn string(&mut self, tag: u32, text: &str) {
        self.bytes(tag, text.as_bytes());
    }
    fn bytes(&mut self, tag: u32, bytes: &[u8]) {
        self.key(tag, 2);
        self.varint(bytes.len() as u64);
        self.0.extend_from_slice(bytes);
    }
    fn message(&mut self, tag: u32, message: Self) {
        self.bytes(tag, &message.0);
    }
    fn packed(&mut self, tag: u32, values: impl IntoIterator<Item = u64>) {
        let mut packed = Self::default();
        for n in values {
            packed.varint(n);
        }
        self.message(tag, packed);
    }
}

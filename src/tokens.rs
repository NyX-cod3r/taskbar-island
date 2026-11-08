//! Machine-readable v0.8 design tokens.
//!
//! The JSON file is intentionally checked into the application repository so
//! that builds, tests, and offline installer builds all use the same design
//! contract. `design` exposes the hot-path Rust constants; this module keeps
//! the complete token document available to validation and tooling.

use crate::json::{self, Json};

pub const SOURCE: &str = include_str!("../design-tokens.json");

#[derive(Debug, Clone, PartialEq)]
pub struct NumericToken {
    pub path: String,
    pub value: f64,
}

/// Parse every numeric leaf in the checked-in v0.8 token document. Keeping
/// paths makes a failing drift test identify the exact affected token.
pub fn numeric_tokens() -> Result<Vec<NumericToken>, String> {
    let value = json::parse(SOURCE)?;
    let mut tokens = Vec::new();
    collect_numbers(&value, "", &mut tokens);
    Ok(tokens)
}

fn collect_numbers(value: &Json, path: &str, tokens: &mut Vec<NumericToken>) {
    match value {
        Json::Num(number) => tokens.push(NumericToken {
            path: path.to_owned(),
            value: *number,
        }),
        Json::Arr(values) => {
            for (index, item) in values.iter().enumerate() {
                collect_numbers(item, &format!("{path}[{index}]"), tokens);
            }
        }
        Json::Obj(fields) => {
            for (key, item) in fields {
                let child = if path.is_empty() {
                    key.to_owned()
                } else {
                    format!("{path}.{key}")
                };
                collect_numbers(item, &child, tokens);
            }
        }
        Json::Null | Json::Bool(_) | Json::Str(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design;

    #[test]
    fn all_numeric_json_tokens_are_present_and_key_geometry_matches() {
        let parsed = json::parse(SOURCE).expect("design-tokens.json must be valid JSON");
        let mut expected = Vec::new();
        collect_numbers(&parsed, "", &mut expected);
        let actual = numeric_tokens().expect("checked-in token document must parse");
        assert_eq!(actual, expected, "every numeric token must be represented");
        assert_eq!(
            actual
                .iter()
                .find(|v| v.path == "fixed.PILL_W")
                .map(|v| v.value),
            Some(design::PILL_W as f64)
        );
        assert_eq!(
            actual
                .iter()
                .find(|v| v.path == "fixed.FLY_W")
                .map(|v| v.value),
            Some(design::FLY_W as f64)
        );
        assert_eq!(
            actual
                .iter()
                .find(|v| v.path == "fixed.FLY_H")
                .map(|v| v.value),
            Some(design::FLY_H as f64)
        );
        assert_eq!(
            actual
                .iter()
                .find(|v| v.path == "fixed.FLY_BOTTOM_GAP")
                .map(|v| v.value),
            Some(design::FLY_BOTTOM_GAP as f64)
        );
        assert_eq!(
            actual
                .iter()
                .find(|v| v.path == "layout.railW")
                .map(|v| v.value),
            Some(design::RAIL_W as f64)
        );
        assert_eq!(
            actual
                .iter()
                .find(|v| v.path == "layout.CARD_PAD")
                .map(|v| v.value),
            Some(design::CARD_PAD as f64)
        );
    }
}

//! What a token costs: the list-price table the report estimates with.
//!
//! Dollar figures are API list prices used as a proxy for how much of the weekly
//! limit a run weighs; compare shares and shapes, not decimals.

use std::collections::BTreeMap;

use super::ReportError;

/// Dollars per million tokens, by kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    /// Fresh input.
    pub input: f64,
    /// Cache writes.
    pub cache_write: f64,
    /// Cache reads.
    pub cache_read: f64,
    /// Output.
    pub output: f64,
}

impl Rates {
    /// The estimated dollars for a message's token counts.
    pub fn cost(&self, input: u64, cache_write: u64, cache_read: u64, output: u64) -> f64 {
        (input as f64 * self.input
            + cache_write as f64 * self.cache_write
            + cache_read as f64 * self.cache_read
            + output as f64 * self.output)
            / 1e6
    }
}

/// Rates by model name, and the rates a model the table does not know is priced at.
#[derive(Debug, Clone, PartialEq)]
pub struct PriceTable {
    known: BTreeMap<String, Rates>,
    fallback: Rates,
}

const fn rates(input: f64, cache_write: f64, cache_read: f64, output: f64) -> Rates {
    Rates {
        input,
        cache_write,
        cache_read,
        output,
    }
}

impl Default for PriceTable {
    /// The list prices of 2026-10; an unknown model is priced like the dearer mid tier.
    fn default() -> Self {
        let known = [
            ("claude-opus-5-5", rates(4.0, 5.0, 0.2, 20.0)),
            ("claude-sonnet-5-5", rates(2.0, 2.5, 0.2, 10.0)),
            ("claude-fable-5-1", rates(10.0, 12.5, 0.25, 50.0)),
            ("claude-haiku-5-5", rates(0.1, 0.125, 0.01, 0.5)),
        ]
        .into_iter()
        .map(|(model, rates)| (model.to_owned(), rates))
        .collect();
        Self {
            known,
            fallback: rates(4.0, 5.0, 0.2, 20.0),
        }
    }
}

impl PriceTable {
    /// The rates a model is priced at.
    pub fn rates(&self, model: &str) -> &Rates {
        self.known.get(model).unwrap_or(&self.fallback)
    }

    /// The defaults with the `[report.prices]` table of a `local.toml` laid over them.
    ///
    /// Each key is a model name and each value four numbers in dollars per million
    /// tokens: input, cache write, cache read, output. Text without that table is
    /// the defaults.
    pub fn from_local_toml(text: &str) -> Result<Self, ReportError> {
        let document: toml::Table = text
            .parse()
            .map_err(|error: toml::de::Error| ReportError::Prices(error.to_string()))?;
        let mut table = Self::default();
        let Some(prices) = document
            .get("report")
            .and_then(toml::Value::as_table)
            .and_then(|report| report.get("prices"))
        else {
            return Ok(table);
        };
        let prices = prices.as_table().ok_or_else(|| {
            ReportError::Prices("[report.prices] must be a table of model names".to_owned())
        })?;
        for (model, value) in prices {
            let bad = || {
                ReportError::Prices(format!(
                    "`{model}` needs four numbers: input, cache write, cache read, output"
                ))
            };
            let numbers: Vec<f64> = value
                .as_array()
                .ok_or_else(bad)?
                .iter()
                .map(|number| {
                    number
                        .as_float()
                        .or_else(|| number.as_integer().map(|whole| whole as f64))
                })
                .collect::<Option<_>>()
                .ok_or_else(bad)?;
            let [input, cache_write, cache_read, output] = numbers[..] else {
                return Err(bad());
            };
            table
                .known
                .insert(model.clone(), rates(input, cache_write, cache_read, output));
        }
        Ok(table)
    }
}

#[cfg(test)]
mod tests;

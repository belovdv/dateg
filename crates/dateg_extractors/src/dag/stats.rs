use std::{collections::BTreeMap, time::Duration};

/// A value in [`ExtractStats::custom`].
#[derive(Clone, Debug, PartialEq)]
pub enum StatValue {
    Count(u64),
    Duration(Duration),
    Text(String),
}

#[derive(Clone, Debug, Default)]
pub struct ExtractStats {
    pub total: Duration,
    pub solver: Option<Duration>,
    pub verify: Option<Duration>,
    pub custom: BTreeMap<&'static str, StatValue>,
}

impl ExtractStats {
    pub fn count(&mut self, key: &'static str, value: u64) {
        self.custom.insert(key, StatValue::Count(value));
    }

    pub fn duration(&mut self, key: &'static str, value: Duration) {
        self.custom.insert(key, StatValue::Duration(value));
    }

    pub fn text(&mut self, key: &'static str, value: impl ToString) {
        self.custom.insert(key, StatValue::Text(value.to_string()));
    }
}

/// Internal optional stats sink; methods are no-ops when stats are disabled.
pub(crate) struct StatsSink<'a>(pub(crate) Option<&'a mut ExtractStats>);

impl StatsSink<'_> {
    pub(crate) fn enabled(&self) -> bool {
        self.0.is_some()
    }

    pub(crate) fn count(&mut self, key: &'static str, value: u64) {
        if let Some(stats) = self.0.as_deref_mut() {
            stats.count(key, value);
        }
    }

    pub(crate) fn duration(&mut self, key: &'static str, value: Duration) {
        if let Some(stats) = self.0.as_deref_mut() {
            stats.duration(key, value);
        }
    }

    pub(crate) fn text(&mut self, key: &'static str, value: impl ToString) {
        if let Some(stats) = self.0.as_deref_mut() {
            stats.text(key, value);
        }
    }

    pub(crate) fn solver(&mut self, value: Duration) {
        if let Some(stats) = self.0.as_deref_mut() {
            stats.solver = Some(value);
        }
    }

    pub(crate) fn verify(&mut self, value: Duration) {
        if let Some(stats) = self.0.as_deref_mut() {
            stats.verify = Some(value);
        }
    }

    pub(crate) fn total(&mut self, value: Duration) {
        if let Some(stats) = self.0.as_deref_mut() {
            stats.total = value;
        }
    }
}

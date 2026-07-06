use egglog_bridge::RuleId;

use crate::*;

impl EGraph {
    pub fn run_single_rule(&mut self, rules: &[RuleId]) {
        self.inner.run_rules(rules).unwrap();
    }
}

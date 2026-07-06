use egglog_bridge::RuleId;

use crate::*;

impl EGraph {
    pub fn run_rules(&mut self, rules: &[RuleId]) {
        self.inner.run_rules(rules).unwrap();
    }
}

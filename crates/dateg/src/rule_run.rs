use egglog_bridge::RuleId;

use crate::*;

impl EGraph {
    pub fn run_single_rule(&mut self, rule: RuleId) {
        self.inner.run_rules(&[rule]).unwrap();
    }
}

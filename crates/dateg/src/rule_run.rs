use egglog_bridge::RuleId;

use crate::*;

impl EGraph {
    pub fn run_ruleset(&mut self, rs: &str) -> bool {
        self.inner
            .run_rules(&self.rulesets[rs][..])
            .unwrap()
            .changed()
    }

    pub fn run_rules(&mut self, rules: &[RuleId]) -> bool {
        self.inner.run_rules(rules).unwrap().changed()
    }
}

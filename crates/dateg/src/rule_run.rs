use crate::*;

macro_rules! run_rules {
    ($_self:ident, $rules:expr) => {
        $_self.inner.run_rules($rules, None).unwrap().changed()
    };
}

impl EGraph {
    pub fn set_ruleset_active(&mut self, rs: impl ToString) {
        self.ruleset_active = rs.to_string();
    }
    pub fn add_ruleset_rule(&mut self, rule: RuleId) {
        self.rulesets
            .entry(self.ruleset_active.clone())
            .or_default()
            .push(rule);
    }

    pub fn run_rules(&mut self, rules: &[RuleId]) -> bool {
        run_rules!(self, rules)
    }

    pub fn run_ruleset(&mut self, rs: &str) -> bool {
        run_rules!(self, &self.rulesets[rs][..])
    }

    pub fn run_ruleset_active(&mut self) -> bool {
        run_rules!(self, &self.rulesets[&self.ruleset_active][..])
    }
}

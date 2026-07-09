use crate::*;

macro_rules! run_rules {
    ($_self:ident, $rules:expr) => {
        $_self.inner.run_rules($rules).unwrap().changed()
    };
}

impl EGraph {
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

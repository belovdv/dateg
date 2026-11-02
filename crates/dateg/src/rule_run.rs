use crate::*;

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
        self.run_rules_report(rules)
            .map(|report| report.changed())
            .unwrap()
    }

    /// Same as [`EGraph::run_rules`], but returns the report with per-rule
    /// match counts and timings.
    pub fn run_rules_report(&mut self, rules: &[RuleId]) -> anyhow::Result<IterationReport> {
        let pool = self.thread_pool.clone();
        let inner = &mut self.inner;
        let mut run = || inner.run_rules(rules, None);
        match pool {
            Some(pool) => pool.install(run),
            None => run(),
        }
    }

    pub fn run_ruleset(&mut self, rs: &str) -> bool {
        let rules = self.rulesets[rs].clone();
        self.run_rules(&rules)
    }

    pub fn run_ruleset_active(&mut self) -> bool {
        let rules = self.rulesets[&self.ruleset_active].clone();
        self.run_rules(&rules)
    }
}

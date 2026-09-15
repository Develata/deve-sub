//! Bounded native group filtering; unsupported expressions fail explicitly.
use super::clash::invalid;
use super::error::TemplateAppError;
use fancy_regex::{Regex, RegexBuilder};
use std::time::{Duration, Instant};

pub(super) struct Budget {
    remaining: usize,
    bytes: usize,
    patterns: usize,
    deadline: Instant,
}

impl Budget {
    pub(super) fn new() -> Self {
        Self {
            remaining: 1_000_000,
            bytes: 64 * 1024 * 1024,
            patterns: 256,
            deadline: Instant::now() + Duration::from_secs(1),
        }
    }

    pub(super) fn visit(&mut self, bytes: usize) -> Result<(), TemplateAppError> {
        if self.remaining == 0 || bytes > self.bytes || Instant::now() >= self.deadline {
            return Err(invalid(
                "proxy-group filtering exceeded its bounded work budget",
            ));
        }
        self.remaining -= 1;
        self.bytes -= bytes;
        Ok(())
    }
}

pub(super) struct Matcher(Vec<Regex>);

impl Matcher {
    pub(super) fn compile(
        value: Option<&str>,
        split: bool,
        budget: &mut Budget,
    ) -> Result<Option<Self>, TemplateAppError> {
        let Some(value) = value.filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        if value.len() > 4096 {
            return Err(invalid("proxy-group regular expression exceeds 4096 bytes"));
        }
        let mut patterns = Vec::new();
        let parts = if split {
            value.split('`').collect()
        } else {
            vec![value]
        };
        for pattern in parts {
            budget.visit(pattern.len())?;
            if budget.patterns == 0 {
                return Err(invalid(
                    "proxy-group filtering exceeds 256 regular expressions",
                ));
            }
            budget.patterns -= 1;
            // WHY: look-around is common in Clash templates. Bound both the
            // backtracking VM and delegated automata; never run unbounded regex.
            let regex = RegexBuilder::new(pattern)
                .backtrack_limit(10_000)
                .delegate_size_limit(1024 * 1024)
                .delegate_dfa_size_limit(1024 * 1024)
                .build()
                .map_err(|_| invalid("invalid or unsupported proxy-group regular expression"))?;
            patterns.push(regex);
        }
        Ok(Some(Self(patterns)))
    }

    pub(super) fn matches(
        &self,
        name: &str,
        budget: &mut Budget,
    ) -> Result<bool, TemplateAppError> {
        for pattern in &self.0 {
            budget.visit(name.len())?;
            if pattern.is_match(name).map_err(|_| {
                invalid("proxy-group regular expression exceeded its execution limit")
            })? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen015_native_regex_bounds_reject_excessive_work() {
        let mut budget = Budget::new();
        let matcher = Matcher::compile(Some("(?i)(a|b|ab)*(?>c)"), true, &mut budget)
            .expect("valid backtracking expression")
            .expect("matcher");
        assert!(matcher.matches(&"ab".repeat(28), &mut budget).is_err());
        let patterns = std::iter::repeat_n("a", 257).collect::<Vec<_>>().join("`");
        assert!(Matcher::compile(Some(&patterns), true, &mut Budget::new()).is_err());
        let mut budget = Budget::new();
        budget.deadline = Instant::now();
        assert!(
            budget.visit(0).is_err(),
            "elapsed work cannot start another match or page"
        );
    }

    #[test]
    fn gen015_native_regex_supports_lookaround_and_alternative_filters() {
        let mut budget = Budget::new();
        let matcher = Matcher::compile(Some("^B$`^(?!B$)A$"), true, &mut budget)
            .expect("valid")
            .expect("matcher");
        assert!(matcher.matches("A", &mut budget).expect("A"));
        assert!(matcher.matches("B", &mut budget).expect("B"));
        assert!(!matcher.matches("C", &mut budget).expect("C"));
    }
}

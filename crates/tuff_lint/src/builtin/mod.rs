pub mod exports;
pub mod legacy;
pub mod modernization;
pub mod policy;
pub mod spacing;
pub mod style;
pub mod typing;

use crate::RuleDefinition;

pub fn definitions() -> Vec<&'static RuleDefinition> {
    policy::RULES
        .iter()
        .chain(style::RULES)
        .chain(exports::RULES)
        .chain(typing::RULES)
        .chain(modernization::RULES)
        .chain(spacing::RULES)
        .chain(legacy::RULES)
        .collect()
}

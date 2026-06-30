//! Formula Language — workflow-as-code engine.
//!
//! This module ports the Go beads formula package, providing:
//! - Core types for `.formula.json` / `.formula.toml` files
//! - Parser with caching, extends resolution, and cycle detection
//! - Validation (duplicate IDs, dependency references, variable consistency)
//! - Variable substitution for step expansion
//! - CLI integration via `br formula apply`
//!
//! Formula types: Workflow (standard steps), Expansion (macro template),
//! Aspect (cross-cutting), Convoy (multi-agent).
//!
//! See `/tmp/beads-go/internal/formula/` for the Go reference implementation.

pub mod parser;
pub mod types;

/// Re-export key types at module level.
pub use parser::Parser;
pub use types::{
    AdviceRule, AdviceStep, AroundAdvice, BondPoint, BranchRule, ComposeRules, ExpandRule, Formula,
    FormulaType, Gate, GateRule, Hook, LoopSpec, MapRule, OnCompleteSpec, Pointcut, Step, VarDef,
};

#[cfg(test)]
mod tests;

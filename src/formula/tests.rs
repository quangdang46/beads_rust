//! Tests for the formula language engine.

#[cfg(test)]
mod tests {
    use crate::formula::{parser, Formula, FormulaType, Step};
    use std::collections::HashMap;

    #[test]
    fn test_parse_minimal_json() {
        let json = r#"{"formula": "mol-test", "version": 1, "type": "workflow"}"#;
        let formula = parser::Parser::parse_json(json.as_bytes()).unwrap();
        assert_eq!(formula.formula, "mol-test");
        assert_eq!(formula.version, 1);
        assert_eq!(formula.r#type, FormulaType::Workflow);
    }

    #[test]
    fn test_parse_json_with_defaults() {
        let json = r#"{"formula": "mol-defaults"}"#;
        let formula = parser::Parser::parse_json(json.as_bytes()).unwrap();
        assert_eq!(formula.version, 1);
        assert_eq!(formula.r#type, FormulaType::Workflow);
    }

    #[test]
    fn test_parse_json_with_steps() {
        let json = r#"{
            "formula": "mol-feature",
            "description": "Standard feature workflow",
            "version": 1,
            "type": "workflow",
            "steps": [
                {"id": "design", "title": "Design", "type": "task", "priority": 2},
                {"id": "implement", "title": "Implement", "depends_on": ["design"]},
                {"id": "review", "title": "Review", "needs": ["implement"]}
            ]
        }"#;
        let formula = parser::Parser::parse_json(json.as_bytes()).unwrap();
        assert_eq!(formula.formula, "mol-feature");
        let steps = formula.steps.unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].id, "design");
        assert_eq!(steps[0].title.as_deref(), Some("Design"));
        assert_eq!(steps[0].priority, Some(2));
        assert!(steps[1].depends_on.contains(&"design".to_string()));
        assert!(steps[2].needs.contains(&"implement".to_string()));
    }

    #[test]
    fn test_validate_minimal() {
        let formula = Formula {
            formula: "mol-valid".to_string(),
            version: 1,
            r#type: FormulaType::Workflow,
            steps: Some(vec![
                Step {
                    id: "step1".to_string(),
                    title: Some("Step 1".to_string()),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };
        assert!(formula.validate().is_ok());
    }

    #[test]
    fn test_validate_missing_name() {
        let formula = Formula {
            formula: "".to_string(),
            version: 1,
            steps: Some(vec![]),
            ..Default::default()
        };
        let err = formula.validate().unwrap_err();
        assert!(err.contains("name is required"));
    }

    #[test]
    fn test_validate_duplicate_id() {
        let formula = Formula {
            formula: "mol-dup".to_string(),
            version: 1,
            steps: Some(vec![
                Step {
                    id: "step1".to_string(),
                    title: Some("Step 1".to_string()),
                    ..Default::default()
                },
                Step {
                    id: "step1".to_string(),
                    title: Some("Step 1 again".to_string()),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };
        let err = formula.validate().unwrap_err();
        assert!(err.contains("duplicate id"));
    }

    #[test]
    fn test_validate_bad_dep() {
        let formula = Formula {
            formula: "mol-bad-dep".to_string(),
            version: 1,
            steps: Some(vec![
                Step {
                    id: "step1".to_string(),
                    title: Some("Step 1".to_string()),
                    ..Default::default()
                },
                Step {
                    id: "step2".to_string(),
                    title: Some("Step 2".to_string()),
                    depends_on: vec!["nonexistent".to_string()],
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };
        let err = formula.validate().unwrap_err();
        assert!(err.contains("references unknown step"));
    }

    #[test]
    fn test_validate_priority_range() {
        let formula = Formula {
            formula: "mol-bad-prio".to_string(),
            version: 1,
            steps: Some(vec![
                Step {
                    id: "step1".to_string(),
                    title: Some("Bad".to_string()),
                    priority: Some(99),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };
        let err = formula.validate().unwrap_err();
        assert!(err.contains("priority must be 0-4"));
    }

    #[test]
    fn test_variable_substitution() {
        let mut vars = HashMap::new();
        vars.insert("component".to_string(), "auth".to_string());
        vars.insert("team".to_string(), "backend".to_string());

        let result = parser::substitute_vars("Implement {{component}} for {{team}}", &vars);
        assert_eq!(result, "Implement auth for backend");
    }

    #[test]
    fn test_variable_substitution_no_vars() {
        // Unreplaced placeholders stay as-is
        let vars = HashMap::new();
        let result = parser::substitute_vars("Hello {{name}}", &vars);
        assert_eq!(result, "Hello {{name}}");
    }

    #[test]
    fn test_parse_with_extended_types() {
        let json = r#"{
            "formula": "exp-audit",
            "version": 1,
            "type": "expansion",
            "template": [
                {"id": "audit-logs", "title": "Audit logs"},
                {"id": "audit-perms", "title": "Audit permissions"}
            ]
        }"#;
        let formula = parser::Parser::parse_json(json.as_bytes()).unwrap();
        assert_eq!(formula.r#type, FormulaType::Expansion);
        assert!(formula.template.is_some());
        assert_eq!(formula.template.unwrap().len(), 2);
    }

    #[test]
    fn test_parse_with_gate_and_loop() {
        let json = r#"{
            "formula": "mol-gated",
            "steps": [
                {
                    "id": "approve",
                    "title": "Get approval",
                    "gate": {"type": "human", "id": "approval-1", "timeout": "24h"}
                },
                {
                    "id": "build",
                    "title": "Build",
                    "loop": {"count": 3, "body": [{"id": "compile", "title": "Compile"}]}
                }
            ]
        }"#;
        let formula = parser::Parser::parse_json(json.as_bytes()).unwrap();
        let steps = formula.steps.unwrap();
        assert_eq!(steps.len(), 2);
        let gate = steps[0].gate.as_ref().unwrap();
        assert_eq!(gate.r#type, "human");
        assert_eq!(gate.id.as_deref(), Some("approval-1"));
        let loop_spec = steps[1].r#loop.as_ref().unwrap();
        assert_eq!(loop_spec.count, Some(3));
    }
}

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ResolveError {
    pub variable_name: String,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "undefined variable: ${}", self.variable_name)
    }
}

#[allow(dead_code)]
pub fn resolve_variables(
    pipeline_defaults: &HashMap<String, serde_yaml::Value>,
    run_overrides: &HashMap<String, serde_yaml::Value>,
) -> HashMap<String, serde_yaml::Value> {
    let mut resolved = pipeline_defaults.clone();
    for (k, v) in run_overrides {
        resolved.insert(k.clone(), v.clone());
    }
    resolved
}

pub fn resolve_value(
    operand: &serde_yaml::Value,
    resolved_vars: &HashMap<String, serde_yaml::Value>,
) -> Result<serde_yaml::Value, ResolveError> {
    if let Some(s) = operand.as_str() {
        if let Some(var_name) = s.strip_prefix('$') {
            return resolved_vars.get(var_name).cloned().ok_or(ResolveError {
                variable_name: var_name.to_string(),
            });
        }
    }
    if let Some(seq) = operand.as_sequence() {
        let resolved_seq: Result<Vec<serde_yaml::Value>, ResolveError> = seq
            .iter()
            .map(|item| resolve_value(item, resolved_vars))
            .collect();
        return Ok(serde_yaml::Value::Sequence(resolved_seq?));
    }
    Ok(operand.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn val_i(n: i64) -> serde_yaml::Value {
        serde_yaml::Value::Number(serde_yaml::Number::from(n))
    }

    fn val_s(s: &str) -> serde_yaml::Value {
        serde_yaml::Value::String(s.into())
    }

    fn val_f(f: f64) -> serde_yaml::Value {
        serde_yaml::Value::Number(serde_yaml::Number::from(f))
    }

    fn val_b(b: bool) -> serde_yaml::Value {
        serde_yaml::Value::Bool(b)
    }

    // --- resolve_variables: literal passthrough ---

    #[test]
    fn literal_int_passthrough() {
        let defaults: HashMap<String, serde_yaml::Value> =
            [("max_iter".into(), val_i(5))].into_iter().collect();
        let overrides = HashMap::new();
        let resolved = resolve_variables(&defaults, &overrides);
        assert_eq!(resolved["max_iter"], val_i(5));
    }

    #[test]
    fn literal_string_passthrough() {
        let defaults: HashMap<String, serde_yaml::Value> =
            [("mode".into(), val_s("strict"))].into_iter().collect();
        let overrides = HashMap::new();
        let resolved = resolve_variables(&defaults, &overrides);
        assert_eq!(resolved["mode"], val_s("strict"));
    }

    #[test]
    fn literal_float_passthrough() {
        let defaults: HashMap<String, serde_yaml::Value> =
            [("threshold".into(), val_f(0.8))].into_iter().collect();
        let overrides = HashMap::new();
        let resolved = resolve_variables(&defaults, &overrides);
        assert_eq!(resolved["threshold"], val_f(0.8));
    }

    #[test]
    fn literal_bool_passthrough() {
        let defaults: HashMap<String, serde_yaml::Value> =
            [("verbose".into(), val_b(true))].into_iter().collect();
        let overrides = HashMap::new();
        let resolved = resolve_variables(&defaults, &overrides);
        assert_eq!(resolved["verbose"], val_b(true));
    }

    // --- resolve_variables: override precedence ---

    #[test]
    fn run_override_takes_precedence_over_pipeline_default() {
        let defaults: HashMap<String, serde_yaml::Value> =
            [("max_iter".into(), val_i(5))].into_iter().collect();
        let overrides: HashMap<String, serde_yaml::Value> =
            [("max_iter".into(), val_i(10))].into_iter().collect();
        let resolved = resolve_variables(&defaults, &overrides);
        assert_eq!(resolved["max_iter"], val_i(10));
    }

    #[test]
    fn non_overridden_variable_keeps_default() {
        let defaults: HashMap<String, serde_yaml::Value> = [
            ("max_iter".into(), val_i(5)),
            ("threshold".into(), val_f(0.8)),
        ]
        .into_iter()
        .collect();
        let overrides: HashMap<String, serde_yaml::Value> =
            [("max_iter".into(), val_i(10))].into_iter().collect();
        let resolved = resolve_variables(&defaults, &overrides);
        assert_eq!(resolved["max_iter"], val_i(10));
        assert_eq!(resolved["threshold"], val_f(0.8));
    }

    #[test]
    fn override_can_change_type() {
        let defaults: HashMap<String, serde_yaml::Value> =
            [("max_iter".into(), val_i(5))].into_iter().collect();
        let overrides: HashMap<String, serde_yaml::Value> =
            [("max_iter".into(), val_s("unlimited"))]
                .into_iter()
                .collect();
        let resolved = resolve_variables(&defaults, &overrides);
        assert_eq!(resolved["max_iter"], val_s("unlimited"));
    }

    // --- resolve_value: $<var> resolution ---

    #[test]
    fn resolves_dollar_var_to_value() {
        let vars: HashMap<String, serde_yaml::Value> =
            [("max_iter".into(), val_i(5))].into_iter().collect();
        let operand = val_s("$max_iter");
        let result = resolve_value(&operand, &vars).unwrap();
        assert_eq!(result, val_i(5));
    }

    #[test]
    fn non_dollar_string_passes_through() {
        let vars = HashMap::new();
        let operand = val_s("PASS");
        let result = resolve_value(&operand, &vars).unwrap();
        assert_eq!(result, val_s("PASS"));
    }

    #[test]
    fn integer_operand_passes_through() {
        let vars = HashMap::new();
        let operand = val_i(42);
        let result = resolve_value(&operand, &vars).unwrap();
        assert_eq!(result, val_i(42));
    }

    #[test]
    fn missing_variable_returns_error() {
        let vars = HashMap::new();
        let operand = val_s("$nonexistent");
        let err = resolve_value(&operand, &vars).unwrap_err();
        assert_eq!(err.variable_name, "nonexistent");
    }

    #[test]
    fn resolves_dollar_var_to_string_value() {
        let vars: HashMap<String, serde_yaml::Value> =
            [("mode".into(), val_s("strict"))].into_iter().collect();
        let operand = val_s("$mode");
        let result = resolve_value(&operand, &vars).unwrap();
        assert_eq!(result, val_s("strict"));
    }

    // --- resolve_variables: empty inputs ---

    #[test]
    fn empty_defaults_and_overrides() {
        let resolved = resolve_variables(&HashMap::new(), &HashMap::new());
        assert!(resolved.is_empty());
    }

    #[test]
    fn override_adds_new_variable_not_in_defaults() {
        let defaults = HashMap::new();
        let overrides: HashMap<String, serde_yaml::Value> =
            [("extra".into(), val_i(99))].into_iter().collect();
        let resolved = resolve_variables(&defaults, &overrides);
        assert_eq!(resolved["extra"], val_i(99));
    }
}

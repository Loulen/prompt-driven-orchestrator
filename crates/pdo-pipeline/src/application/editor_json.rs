//! A pipeline as the editor reads it (`GET /pipelines/{id}`), and as its
//! draft holds it.

use super::queries::PipelineDetail;

/// The parsed pipeline as JSON, each node carrying the `provisioning` rules
/// its YAML declares (the parsed `PipelineDef` does not keep them).
pub fn editor_json(detail: &PipelineDetail) -> anyhow::Result<serde_json::Value> {
    let (pipeline, yaml) = (&detail.pipeline, detail.yaml.as_str());
    let mut json = serde_json::to_value(pipeline)?;
    let source: serde_yaml::Value = serde_yaml::from_str(yaml)?;
    let Some(source_nodes) = source.get("nodes").and_then(serde_yaml::Value::as_sequence) else {
        return Ok(json);
    };
    let Some(response_nodes) = json
        .get_mut("nodes")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(json);
    };

    for source_node in source_nodes {
        let Some(node_id) = source_node.get("id").and_then(serde_yaml::Value::as_str) else {
            continue;
        };
        let Some(raw_rules) = source_node.get("provisioning") else {
            continue;
        };
        let rules: pdo_workspace::domain::provisioning::ProvisioningRules =
            serde_yaml::from_value(raw_rules.clone())?;
        if let Some(response_node) = response_nodes
            .iter_mut()
            .find(|node| node.get("id").and_then(serde_json::Value::as_str) == Some(node_id))
        {
            response_node["provisioning"] = serde_json::to_value(rules)?;
        }
    }
    Ok(json)
}


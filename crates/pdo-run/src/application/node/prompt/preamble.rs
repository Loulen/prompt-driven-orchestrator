//! The preamble every agent node reads before its role prompt: who it is, what
//! it reads and writes, where it edits, how it completes.

use pdo_pipeline::domain::pipeline::{NodeType, PortType, IMAGE_EXTENSIONS};

use super::inputs::{resolve_input_paths, resolve_output_paths};
use super::sections::*;
use super::{AugmentContext, InputResolution, OutputDeclaration};

pub fn build_preamble(ctx: &AugmentContext<'_>) -> String {
    let inputs = resolve_input_paths(ctx);
    let outputs = resolve_output_paths(ctx);

    let mut preamble = String::new();

    // API 0.8: what the extensions say comes first, before anything of PDO's.
    if let Some(text) = ctx.extension_preamble.filter(|t| !t.trim().is_empty()) {
        preamble.push_str(text.trim_end());
        preamble.push_str("\n\n");
    }
    preamble.push_str("# PDO Runtime Preamble\n\n");
    preamble.push_str(&format!(
        "You are node `{}` in pipeline `{}`, iteration {}.\n\n",
        ctx.node.id, ctx.pipeline.name, ctx.iter
    ));

    push_inputs(&mut preamble, ctx, &inputs);
    push_outputs(&mut preamble, ctx, &outputs);
    push_partial_outputs(&mut preamble, ctx);
    push_secondary_repos(&mut preamble, ctx);
    push_source_edits(&mut preamble, ctx);
    push_completion(&mut preamble);
    push_variables(&mut preamble, ctx);

    preamble
}

/// What the node reads: its input ports, the images and files of the Run's
/// attachments.
pub(super) fn push_inputs(
    preamble: &mut String,
    ctx: &AugmentContext<'_>,
    inputs: &[InputResolution],
) {
    preamble.push_str("## Inputs\n\n");
    if inputs.is_empty() {
        preamble.push_str("No inputs.\n\n");
    } else {
        for input in inputs {
            // When the pipeline is prompt-optional and no prompt was supplied,
            // the node must source its own work; a supplied prompt is merely
            // additional info layered on the node's own brief.
            if input.from_start && !ctx.pipeline.prompt_required {
                // `from_start` is always a single path.
                let path = input
                    .paths
                    .first()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                if ctx.start_prompt_present {
                    preamble.push_str(&format!(
                        "- `{}` (additional info): read `{}`. \
                         This is supplementary context — your role prompt below is the primary brief.\n",
                        input.port_name, path
                    ));
                } else {
                    preamble.push_str(&format!(
                        "- `{}`: No prompt was provided for this run. \
                         Source your own work from your role prompt below \
                         (an empty `{}` is expected).\n",
                        input.port_name, path
                    ));
                }
            } else if input.repeated {
                // Enumerate one concrete path per completed source iteration: a
                // raw `iter-*` glob would re-include failed iters. An empty pool
                // gets an explicit line, never an orphan glob (ADR-0004).
                if input.paths.is_empty() {
                    preamble.push_str(&format!(
                        "- `{}` (accumulated): no completed iterations yet — nothing to read.\n",
                        input.port_name
                    ));
                } else {
                    preamble.push_str(&format!(
                        "- `{}` (accumulated): read all of these files:\n",
                        input.port_name
                    ));
                    for path in &input.paths {
                        preamble.push_str(&format!("  - {}\n", path.display()));
                    }
                }
            } else {
                let path = input
                    .paths
                    .first()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                preamble.push_str(&format!("- `{}`: read `{}`\n", input.port_name, path));
            }
        }
        preamble.push('\n');
    }

    if !ctx.input_images.is_empty() {
        preamble.push_str("## Input Images\n\n");
        preamble.push_str(
            "The following images were uploaded alongside the text prompt. \
             Use the Read tool to view them:\n",
        );
        let input_dir = ctx.artifacts_dir.join("_input");
        for filename in &ctx.input_images {
            let img_path = input_dir.join(filename);
            preamble.push_str(&format!("- `{}`\n", img_path.display()));
        }
        preamble.push('\n');
    }

    if !ctx.input_files.is_empty() {
        preamble.push_str("## Input Files\n\n");
        preamble.push_str(
            "The following files were passed to the entry node alongside the text \
             prompt (the run's attachments). Read them as part of your input:\n",
        );
        let input_dir = ctx.artifacts_dir.join("_input");
        for filename in &ctx.input_files {
            let path = input_dir.join(filename);
            preamble.push_str(&format!("- `{}`\n", path.display()));
        }
        preamble.push('\n');
    }
}

/// What the node writes: each output port, with its schema and conduct.
pub(super) fn push_outputs(
    preamble: &mut String,
    ctx: &AugmentContext<'_>,
    outputs: &[OutputDeclaration],
) {
    preamble.push_str("## Outputs\n\n");
    if outputs.is_empty() {
        preamble.push_str("No outputs declared.\n\n");
    } else {
        let ext_list = IMAGE_EXTENSIONS.join(", .");
        for output in outputs {
            let instructions = (ctx.node.node_type == NodeType::Agent)
                .then(|| {
                    ctx.node
                        .outputs
                        .iter()
                        .find(|port| port.name == output.port_name)
                        .and_then(|port| port.instructions.as_deref())
                })
                .flatten();
            match output.port_type {
                PortType::Image => {
                    preamble.push_str(&format!(
                        "- `{}` (image): drop exactly one image file in `{}`\n",
                        output.port_name,
                        output.path.display(),
                    ));
                    append_output_instructions(preamble, instructions);
                    preamble.push_str(&format!("  Accepted extensions: .{}\n", ext_list));
                }
                PortType::ImageList => {
                    preamble.push_str(&format!(
                        "- `{}` (image_list): drop one or more image files in `{}`\n",
                        output.port_name,
                        output.path.display(),
                    ));
                    append_output_instructions(preamble, instructions);
                    preamble.push_str(&format!("  Accepted extensions: .{}\n", ext_list));
                }
                PortType::Markdown => {
                    preamble.push_str(&format!(
                        "- `{}`: write to `{}`\n",
                        output.port_name,
                        output.path.display()
                    ));
                    append_output_instructions(preamble, instructions);

                    let schema = ctx
                        .node
                        .outputs
                        .iter()
                        .find(|p| p.name == output.port_name)
                        .and_then(|p| p.frontmatter.as_ref());

                    if let Some(schema) = schema {
                        preamble.push_str("  Required YAML frontmatter:\n");
                        for (field_name, field_decl) in schema {
                            if let Some(ref allowed) = field_decl.allowed {
                                preamble.push_str(&format!(
                                    "  - `{}`: {} (allowed: {})\n",
                                    field_name,
                                    field_decl.field_type,
                                    allowed.join(", ")
                                ));
                            } else {
                                preamble.push_str(&format!(
                                    "  - `{}`: {}\n",
                                    field_name, field_decl.field_type
                                ));
                            }
                        }
                    }
                }
                // No frontmatter block, and the agent must know the page renders
                // offline in a scriptless sandboxed iframe (ADR-0028).
                PortType::Html => {
                    preamble.push_str(&format!(
                        "- `{}` (html): write a single self-contained HTML file to `{}`\n",
                        output.port_name,
                        output.path.display(),
                    ));
                    append_output_instructions(preamble, instructions);
                    preamble.push_str(
                        "  Inline all CSS in a `<style>` tag and use no external network \
                         requests (no CDN links, web fonts, or remote assets) — the artifact is \
                         rendered offline.\n\
                         \x20 **JavaScript will NOT run**: the file is displayed in a sandboxed \
                         iframe with scripts disabled. Do not rely on `<script>`, inline event \
                         handlers, or any interactivity — everything must be conveyed through \
                         static HTML and CSS.\n",
                    );
                }
            }
        }

        fn append_output_instructions(preamble: &mut String, instructions: Option<&str>) {
            if let Some(instructions) = instructions {
                let mut lines = instructions.lines();
                if let Some(first_line) = lines.next() {
                    preamble.push_str(&format!("  Expected content: {first_line}\n"));
                }
                for line in lines {
                    preamble.push_str(&format!("  {line}\n"));
                }
            }
        }
        preamble.push('\n');
    }
}

//! `pdo node prompt|io|diff|transcript`: what the node panel of the run view
//! shows of a node, over `/runs/{id}/nodes/{node}/*`.

use anyhow::Result;
use clap::Subcommand;
use pdo_platform::adapters::cli::daemon::{print_json, segment, Daemon};

use super::node::iteration;
use super::run_gesture::shown;
use super::run_render::transcript_lines;

#[derive(Subcommand, Debug)]
pub enum NodeRead {
    /// The prompt an iteration of the node received.
    Prompt {
        run_id: String,
        node_id: String,
        /// The iteration (default: the node's latest).
        #[arg(long)]
        iter: Option<i64>,
    },
    /// The inputs and outputs of an iteration of the node (JSON).
    Io {
        run_id: String,
        node_id: String,
        /// The iteration (default: the node's latest).
        #[arg(long)]
        iter: Option<i64>,
    },
    /// The diff the node made on its branch.
    Diff { run_id: String, node_id: String },
    /// A page of the node's transcript, one entry a line; `--tool <index>`
    /// shows one tool call in full.
    Transcript {
        run_id: String,
        node_id: String,
        /// The iteration (default: the node's latest).
        #[arg(long)]
        iter: Option<i64>,
        #[arg(long, default_value_t = 0)]
        after: usize,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, value_name = "INDEX")]
        tool: Option<usize>,
        #[arg(long)]
        json: bool,
    },
}

pub(super) fn run_node_read(daemon: &Daemon, read: NodeRead) -> Result<()> {
    match read {
        NodeRead::Prompt {
            run_id,
            node_id,
            iter,
        } => {
            let iter = iteration(daemon, &run_id, &node_id, iter)?;
            let (r, n) = (segment(&run_id), segment(&node_id));
            let prompt = daemon
                .get(&format!("/runs/{r}/nodes/{n}/prompt?iter={iter}"))?
                .ok()?;
            print!("{}", prompt.body);
        }
        NodeRead::Io {
            run_id,
            node_id,
            iter,
        } => {
            let iter = iteration(daemon, &run_id, &node_id, iter)?;
            let (r, n) = (segment(&run_id), segment(&node_id));
            print_json(
                &daemon
                    .get(&format!("/runs/{r}/nodes/{n}/io?iter={iter}"))?
                    .ok()?
                    .json()?,
            );
        }
        NodeRead::Diff { run_id, node_id } => {
            let (r, n) = (segment(&run_id), segment(&node_id));
            print!(
                "{}",
                daemon.get(&format!("/runs/{r}/nodes/{n}/diff"))?.ok()?.body
            );
        }
        NodeRead::Transcript {
            run_id,
            node_id,
            iter,
            after,
            limit,
            tool,
            json,
        } => {
            let iter = iteration(daemon, &run_id, &node_id, iter)?;
            let (r, n) = (segment(&run_id), segment(&node_id));
            match tool {
                Some(index) => {
                    let detail = daemon
                        .get(&format!(
                            "/runs/{r}/nodes/{n}/transcript/tools/{index}?iter={iter}"
                        ))?
                        .ok()?
                        .json()?;
                    print_json(&detail);
                }
                None => {
                    let page = daemon
                        .get(&format!(
                            "/runs/{r}/nodes/{n}/transcript?iter={iter}&after={after}&limit={limit}"
                        ))?
                        .ok()?
                        .json()?;
                    shown(&page, json, transcript_lines);
                }
            }
        }
    }
    Ok(())
}

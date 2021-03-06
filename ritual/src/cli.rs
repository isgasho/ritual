//! Generator of Rust-Qt crates.
//!
//! See [README](https://github.com/rust-qt/ritual)
//! for more information.

use crate::config::GlobalConfig;
use crate::database::ItemId;
use crate::processor;
use crate::workspace::Workspace;
use flexi_logger::{Duplicate, LevelFilter, LogSpecification, Logger};
use itertools::Itertools;
use log::{error, info};
use ritual_common::errors::{bail, err_msg, Result};
use ritual_common::file_utils::{canonicalize, create_dir, load_json, path_to_str};
use ritual_common::target::current_target;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
/// Generates rust_qt crates using ritual.
/// See [ritual](https://github.com/rust-qt/ritual) for more details.
pub struct Options {
    #[structopt(parse(from_os_str))]
    /// Directory for output and temporary files
    pub workspace: PathBuf,
    #[structopt(long = "local-paths")]
    /// Write local paths to `ritual` crates in generated `Cargo.toml`
    pub local_paths: Option<bool>,
    #[structopt(short = "c", long = "crates", required = true)]
    /// Crates to process (e.g. `qt_core`)
    pub crates: Vec<String>,
    #[structopt(short = "o", long = "operations", required = true)]
    /// Operations to perform
    pub operations: Vec<String>,
    #[structopt(long = "cluster")]
    /// Cluster configuration
    pub cluster: Option<PathBuf>,
    #[structopt(long = "trace")]
    /// ID of item to trace
    pub trace: Option<String>,
}

pub fn run_from_args(config: GlobalConfig) -> Result<()> {
    run(Options::from_args(), config)
}

pub fn run(options: Options, mut config: GlobalConfig) -> Result<()> {
    if !options.workspace.exists() {
        create_dir(&options.workspace)?;
    }
    let workspace_path = canonicalize(options.workspace)?;

    let mut workspace = Workspace::new(workspace_path.clone())?;

    Logger::with(LogSpecification::default(LevelFilter::Trace).build())
        .log_to_file()
        .directory(path_to_str(&workspace.log_path())?)
        .suppress_timestamp()
        .append()
        .print_message()
        .duplicate_to_stderr(Duplicate::Info)
        .start()
        .unwrap_or_else(|e| panic!("Logger initialization failed: {}", e));

    info!("");
    info!("Workspace: {}", workspace_path.display());
    info!("Current target: {}", current_target().short_text());

    let mut was_any_action = false;

    let final_crates = if options.crates.iter().any(|x| *x == "all") {
        let all = config.all_crate_names();
        if all.is_empty() {
            bail!("\"all\" is not supported as crate name specifier");
        }
        all.to_vec()
    } else {
        options.crates.clone()
    };

    let operations = options
        .operations
        .iter()
        .map(|s| s.to_lowercase())
        .collect_vec();

    if operations.is_empty() {
        error!("No action requested. Run \"qt_generator --help\".");
        return Ok(());
    }

    let trace_item_id = if let Some(text) = options.trace {
        let mut parts = text.split('#');
        let crate_name = parts
            .next()
            .ok_or_else(|| err_msg("invalid id format for trace"))?;
        let id = parts
            .next()
            .ok_or_else(|| err_msg("invalid id format for trace"))?
            .parse()?;
        Some(ItemId::new(crate_name.to_string(), id))
    } else {
        None
    };

    for crate_name in &final_crates {
        let create_config = config
            .create_config_hook()
            .ok_or_else(|| err_msg("create_config_hook is missing"))?;

        let mut config = create_config(&crate_name)?;

        if let Some(cluster_config_path) = &options.cluster {
            config.set_cluster_config(load_json(cluster_config_path)?);
        }

        if let Some(local_paths) = options.local_paths {
            config.set_write_dependencies_local_paths(local_paths);
        }

        was_any_action = true;
        processor::process(&mut workspace, &config, &operations, trace_item_id.as_ref())?;
    }

    //workspace.save_data()?;
    if was_any_action {
        info!("ritual finished");
    } else {
        error!("No action requested. Run \"qt_generator --help\".");
    }
    Ok(())
}

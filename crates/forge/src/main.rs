use anyhow::{anyhow, bail, Context, Result};
use camino::Utf8PathBuf;
use clap::Parser;
use include_dir::{include_dir, Dir};
use scarb_metadata::{MetadataCommand, PackageMetadata};
use scarb_ui::args::PackagesFilter;
use std::path::PathBuf;
use tempfile::{tempdir, TempDir};

use forge::{pretty_printing, RunnerConfig};
use forge::{run, TestFileSummary};

use forge::scarb::{
    config_from_scarb_for_package, corelib_for_package, dependencies_for_package,
    get_contracts_map, name_for_package, paths_for_package, target_dir_for_package,
    target_name_for_package, try_get_starknet_artifacts_path,
};
use forge::test_case_summary::TestCaseSummary;
use std::process::{Command, Stdio};

mod init;

static PREDEPLOYED_CONTRACTS: Dir = include_dir!("crates/cheatnet/predeployed-contracts");

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    /// Name used to filter tests
    test_filter: Option<String>,
    /// Use exact matches for `test_filter`
    #[arg(short, long)]
    exact: bool,
    /// Create a new directory and forge project named <NAME>
    #[arg(long, value_name = "NAME")]
    init: Option<String>,
    /// Stop test execution after the first failed test
    #[arg(short = 'x', long)]
    exit_first: bool,

    #[command(flatten)]
    packages_filter: PackagesFilter,
}

fn load_predeployed_contracts() -> Result<TempDir> {
    let tmp_dir = tempdir()?;
    PREDEPLOYED_CONTRACTS
        .extract(&tmp_dir)
        .context("Failed to copy corelib to temporary directory")?;
    Ok(tmp_dir)
}

fn extract_failed_tests(tests_summaries: Vec<TestFileSummary>) -> Vec<TestCaseSummary> {
    tests_summaries
        .into_iter()
        .flat_map(|test_file_summary| test_file_summary.test_case_summaries)
        .filter(|test_case_summary| matches!(test_case_summary, TestCaseSummary::Failed { .. }))
        .collect()
}

fn main_execution() -> Result<bool> {
    let args = Args::parse();
    if let Some(project_name) = args.init {
        init::run(project_name.as_str())?;
        return Ok(true);
    }

    let predeployed_contracts_dir = load_predeployed_contracts()?;
    let predeployed_contracts_path: PathBuf = predeployed_contracts_dir.path().into();
    let predeployed_contracts = Utf8PathBuf::try_from(predeployed_contracts_path.clone())
        .context("Failed to convert path to predeployed contracts to Utf8PathBuf")?;

    which::which("scarb")
        .context("Cannot find `scarb` binary in PATH. Make sure you have Scarb installed https://github.com/software-mansion/scarb")?;

    let scarb_metadata = MetadataCommand::new().inherit_stderr().exec()?;

    let packages: Vec<PackageMetadata> = args
        .packages_filter
        .match_many(&scarb_metadata)
        .context("Failed to find any packages matching the specified filter")?;

    let mut all_failed_tests = vec![];
    for package in &packages {
        let forge_config = config_from_scarb_for_package(&scarb_metadata, &package.id)?;
        let (package_path, lib_path) = paths_for_package(&scarb_metadata, &package.id)?;
        std::env::set_current_dir(package_path.clone())?;

        // TODO(#671)
        let target_dir = target_dir_for_package(&scarb_metadata.workspace.root)?;

        let build_output = Command::new("scarb")
            .arg("build")
            .stderr(Stdio::inherit())
            .stdout(Stdio::inherit())
            .output()
            .context("Failed to build contracts with Scarb")?;
        if !build_output.status.success() {
            bail!("Scarb build did not succeed")
        }

        let package_name = name_for_package(&scarb_metadata, &package.id)?;
        let dependencies = dependencies_for_package(&scarb_metadata, &package.id)?;
        let target_name = target_name_for_package(&scarb_metadata, &package.id)?;
        let corelib_path = corelib_for_package(&scarb_metadata, &package.id)?;
        let runner_config = RunnerConfig::new(
            args.test_filter.clone(),
            args.exact,
            args.exit_first,
            &forge_config,
        );

        let contracts_path = try_get_starknet_artifacts_path(&target_dir, &target_name)?;
        let contracts = contracts_path
            .map(|path| get_contracts_map(&path))
            .transpose()?
            .unwrap_or_default();

        let tests_file_summaries = run(
            &package_path,
            &package_name,
            &lib_path,
            &Some(dependencies.clone()),
            &runner_config,
            &corelib_path,
            &contracts,
            &predeployed_contracts,
        )?;

        let mut failed_tests = extract_failed_tests(tests_file_summaries);
        all_failed_tests.append(&mut failed_tests);
    }

    // Explicitly close the temporary directories so we can handle the errors
    predeployed_contracts_dir.close().with_context(|| {
        anyhow!(
            "Failed to close temporary directory = {} with predeployed contracts. Predeployed contract files might have not been released from filesystem",
            predeployed_contracts_path.display()
        )
    })?;

    pretty_printing::print_failures(&all_failed_tests);

    Ok(all_failed_tests.is_empty())
}

fn main() {
    match main_execution() {
        Ok(true) => std::process::exit(0),
        Ok(false) => std::process::exit(1),
        Err(error) => {
            pretty_printing::print_error_message(&error);
            std::process::exit(2);
        }
    };
}

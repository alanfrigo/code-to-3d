mod contract;

use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use contract::{Fixture, Profile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const PROFILE: &str = include_str!("../profiles/godot-glb.json");
const TOOLCHAIN: &str = include_str!("../toolchain.lock.json");
const EXPORTER: &str = include_str!("../executors/blender/export_fixture.py");
const GODOT_PROJECT: &str = include_str!("../engine-probe/project.godot");
const GODOT_PROBE: &str = include_str!("../engine-probe/probe.gd");

#[derive(Parser)]
#[command(version, about = "Code-to-3D fixture verification")]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Export the bundled rigged fixture and verify its Godot editor import.
    Roundtrip {
        /// Official Blender executable from toolchain.lock.json.
        #[arg(long)]
        blender: PathBuf,
        /// Official Godot executable from toolchain.lock.json.
        #[arg(long)]
        godot: PathBuf,
        /// New directory for the GLB, stage logs and verification report.
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Toolchain {
    platform: String,
    blender: ToolPin,
    godot: ToolPin,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ToolPin {
    version: String,
    executable_sha256: String,
    archive_url: String,
    archive_sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Verdict {
    Pass,
    Fail,
}

#[derive(Serialize)]
struct Report<'a> {
    schema_version: u32,
    profile: &'a str,
    verdict: Verdict,
    toolchain: Toolchain,
    glb_sha256: String,
    observed: Fixture,
    failures: Vec<String>,
}

fn main() -> ExitCode {
    let Action::Roundtrip {
        blender,
        godot,
        out,
    } = Cli::parse().command;
    match roundtrip(&blender, &godot, &out) {
        Ok(Verdict::Pass) => ExitCode::SUCCESS,
        Ok(Verdict::Fail) => ExitCode::from(3),
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn roundtrip(blender: &Path, godot: &Path, out: &Path) -> Result<Verdict> {
    let toolchain: Toolchain =
        serde_json::from_str(TOOLCHAIN).context("invalid bundled toolchain lock")?;
    let profile: Profile = serde_json::from_str(PROFILE).context("invalid bundled profile")?;
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    ensure!(
        platform == toolchain.platform,
        "profile requires {}, not {platform}",
        toolchain.platform
    );
    let blender = pinned_binary(blender, &toolchain.blender)?;
    let godot = pinned_binary(godot, &toolchain.godot)?;
    fs::create_dir(out).with_context(|| format!("create new run directory {}", out.display()))?;
    let out = out.canonicalize()?;
    let project = out.join("probe");
    let home = out.join("home");
    fs::create_dir(&project)?;
    fs::create_dir(&home)?;
    let exporter = out.join("export_fixture.py");
    let glb = project.join("fixture.glb");
    let observed_path = out.join("observed.json");
    fs::write(&exporter, EXPORTER)?;
    fs::write(project.join("project.godot"), GODOT_PROJECT)?;
    fs::write(project.join("probe.gd"), GODOT_PROBE)?;

    stage(
        Command::new(blender)
            .args([
                "--background",
                "--factory-startup",
                "--disable-autoexec",
                "--python-exit-code",
                "1",
                "--python",
            ])
            .arg(exporter)
            .arg("--")
            .arg(&glb),
        &out,
        &home,
        "blender-export",
    )?;
    let glb_sha256 = sha256(&glb).context("Blender did not produce the fixture GLB")?;
    stage(
        godot_command(&godot, &project).args(["--editor", "--import"]),
        &out,
        &home,
        "godot-import",
    )?;
    stage(
        godot_command(&godot, &project)
            .args(["--script", "res://probe.gd", "--"])
            .arg(&observed_path),
        &out,
        &home,
        "godot-probe",
    )?;
    let observed: Fixture = serde_json::from_reader(
        File::open(&observed_path)
            .with_context(|| format!("open {} (see godot-probe.log)", observed_path.display()))?,
    )
    .context("invalid Godot observation JSON")?;
    let failures = contract::compare(&profile, &observed);
    let report = Report {
        schema_version: 1,
        profile: &profile.id,
        verdict: if failures.is_empty() {
            Verdict::Pass
        } else {
            Verdict::Fail
        },
        toolchain,
        glb_sha256,
        observed,
        failures,
    };
    let report_path = out.join("report.json");
    serde_json::to_writer_pretty(File::create(&report_path)?, &report)?;
    match report.verdict {
        Verdict::Pass => println!("PASS {} — {}", profile.id, report_path.display()),
        Verdict::Fail => {
            eprintln!("FAIL {} — {}", profile.id, report_path.display());
            for failure in &report.failures {
                eprintln!("  {failure}");
            }
        }
    }
    Ok(report.verdict)
}

fn godot_command(binary: &Path, project: &Path) -> Command {
    let mut command = Command::new(binary);
    command.args(["--headless", "--path"]).arg(project);
    command
}

fn stage(command: &mut Command, out: &Path, home: &Path, name: &str) -> Result<()> {
    let log_path = out.join(format!("{name}.log"));
    let log = File::create(&log_path)?;
    let status = command
        .current_dir(out)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .status()
        .with_context(|| format!("launch {name} (log: {})", log_path.display()))?;
    ensure!(
        status.success(),
        "{name} exited with {status} (log: {})",
        log_path.display()
    );
    Ok(())
}

fn pinned_binary(path: &Path, pin: &ToolPin) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .with_context(|| format!("resolve executable {}", path.display()))?;
    let actual = sha256(&path)?;
    ensure!(
        actual == pin.executable_sha256,
        "{}: SHA-256 {actual} does not match the pinned {} ({})",
        path.display(),
        pin.version,
        pin.executable_sha256
    );
    let result = Command::new(&path)
        .arg("--version")
        .output()
        .with_context(|| format!("read version of {}", path.display()))?;
    if !result.status.success() {
        bail!(
            "{} --version exited with {}: {}",
            path.display(),
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let output = String::from_utf8_lossy(&result.stdout);
    let version = output.lines().next().unwrap_or_default().trim();
    ensure!(
        version == pin.version,
        "{}: expected version {}, got {version:?}",
        path.display(),
        pin.version
    );
    Ok(path)
}

fn sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).with_context(|| format!("hash {}", path.display()))?;
    Ok(format!("{:x}", hasher.finalize()))
}

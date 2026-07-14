mod json;
mod verify;

use serde::Serialize;
use std::env;
use std::path::PathBuf;

const VERSION: &str = "0.1.0";

#[derive(Debug)]
struct Args {
    bundle: PathBuf,
    policy: PathBuf,
    policy_sha256: String,
    artifact_tree: PathBuf,
    artifact_root: PathBuf,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut bundle = None;
        let mut policy = None;
        let mut policy_sha256 = None;
        let mut artifact_tree = None;
        let mut artifact_root = None;
        let mut args = env::args().skip(1);
        while let Some(flag) = args.next() {
            if flag == "--help" || flag == "-h" {
                return Err(usage());
            }
            if flag == "--version" || flag == "-V" {
                return Err(format!("genesis-evidence-verifier {VERSION}"));
            }
            let value = args
                .next()
                .ok_or_else(|| format!("missing value for {flag}\n{}", usage()))?;
            match flag.as_str() {
                "--bundle" => set_once(&mut bundle, PathBuf::from(value), &flag)?,
                "--policy" => set_once(&mut policy, PathBuf::from(value), &flag)?,
                "--policy-sha256" => set_once(&mut policy_sha256, value, &flag)?,
                "--artifact-tree" => set_once(&mut artifact_tree, PathBuf::from(value), &flag)?,
                "--artifact-root" => set_once(&mut artifact_root, PathBuf::from(value), &flag)?,
                _ => return Err(format!("unknown option: {flag}\n{}", usage())),
            }
        }
        Ok(Self {
            bundle: bundle.ok_or_else(|| format!("missing --bundle\n{}", usage()))?,
            policy: policy.ok_or_else(|| format!("missing --policy\n{}", usage()))?,
            policy_sha256: policy_sha256
                .ok_or_else(|| format!("missing --policy-sha256\n{}", usage()))?,
            artifact_tree: artifact_tree
                .ok_or_else(|| format!("missing --artifact-tree\n{}", usage()))?,
            artifact_root: artifact_root
                .ok_or_else(|| format!("missing --artifact-root\n{}", usage()))?,
        })
    }
}

fn set_once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("duplicate option: {flag}"));
    }
    *slot = Some(value);
    Ok(())
}

fn usage() -> String {
    "usage: genesis-evidence-verifier --bundle FILE --policy FILE \
--policy-sha256 HEX --artifact-tree FILE --artifact-root DIR"
        .to_owned()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Failure<'a> {
    kind: &'a str,
    ok: bool,
    verifier_version: &'a str,
    diagnostic: &'a str,
}

fn main() {
    let args = match Args::parse() {
        Ok(args) => args,
        Err(error) => {
            emit_failure(&error);
            std::process::exit(2);
        }
    };
    match verify::verify(&args, VERSION) {
        Ok(report) => match serde_json::to_string(&report) {
            Ok(output) => println!("{output}"),
            Err(error) => {
                emit_failure(&format!("verification report encoding failed: {error}"));
                std::process::exit(1);
            }
        },
        Err(error) => {
            emit_failure(&error);
            std::process::exit(1);
        }
    }
}

fn emit_failure(diagnostic: &str) {
    let failure = Failure {
        kind: "genesis/evidence-verification-result-v0.1",
        ok: false,
        verifier_version: VERSION,
        diagnostic,
    };
    match serde_json::to_string(&failure) {
        Ok(output) => eprintln!("{output}"),
        Err(_) => eprintln!("genesis-evidence-verifier: verification failed"),
    }
}

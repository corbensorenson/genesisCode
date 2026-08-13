use super::*;
use ed25519_dalek::{Signer, Verifier};

fn signing_authority(
    cli: &Cli,
    context: &str,
) -> Result<gc_obligations::SigningAuthority, CliError> {
    let artifact = require_explicit_selfhost_artifact(cli, context)?;
    gc_obligations::SigningAuthority::load(resolved_selfhost_bootstrap_mode(cli), Some(&artifact))
        .map_err(|error| cli_err(EX_VERIFY, "sign/authority", format!("{error}")))
}

pub(super) fn cmd_keygen(cli: &Cli, out: &Path) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let out_buf = if frontend_is_rust(&frontend) {
        out.to_path_buf()
    } else {
        let req = Term::Map(
            [(
                TermOrdKey(Term::symbol(":out")),
                Term::Str(out.display().to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let planned = selfhost_plan_request_map(cli, "core/cli::keygen-request", req, "keygen")?;
        PathBuf::from(planned_required_str(&planned, ":out", "keygen")?)
    };
    let out = out_buf.as_path();

    let k = gc_obligations::KeyFile::generate_ed25519();
    let signing = k
        .signing_key()
        .map_err(|error| cli_err(EX_INTERNAL, "keygen/mechanism", format!("{error}")))?;
    let verifying = k
        .verifying_key()
        .map_err(|error| cli_err(EX_INTERNAL, "keygen/mechanism", format!("{error}")))?;
    let keypair_valid = signing.verifying_key() == verifying;
    signing_authority(cli, "key generation authority")?
        .keygen(verifying.to_bytes(), keypair_valid)
        .map_err(|error| cli_err(EX_VERIFY, "keygen/authority", format!("{error}")))?;
    k.write_secure(out)
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/keygen-v0.2",
        data: Some(serde_json::json!({
            "out": out.display().to_string(),
            "alg": k.alg,
            "pk_b64": k.pk_b64,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", out.display())
        },
        json: json_envelope_value(env)?,
    })
}

pub(super) fn cmd_sign(
    cli: &Cli,
    pkg: &Path,
    key_path: &Path,
    acceptance: Option<&str>,
    signatures: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let (pkg_buf, key_path_buf, acceptance_buf, signatures_buf) = if frontend_is_rust(&frontend) {
        (
            pkg.to_path_buf(),
            key_path.to_path_buf(),
            acceptance.map(|s| s.to_string()),
            signatures.map(Path::to_path_buf),
        )
    } else {
        let req = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":pkg")),
                    Term::Str(pkg.display().to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":key")),
                    Term::Str(key_path.display().to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":acceptance")),
                    acceptance
                        .map(|s| Term::Str(s.to_string()))
                        .unwrap_or(Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":signatures")),
                    signatures
                        .map(|p| Term::Str(p.display().to_string()))
                        .unwrap_or(Term::Nil),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let planned = selfhost_plan_request_map(cli, "core/cli::sign-request", req, "sign")?;
        (
            PathBuf::from(planned_required_str(&planned, ":pkg", "sign")?),
            PathBuf::from(planned_required_str(&planned, ":key", "sign")?),
            planned_optional_str(&planned, ":acceptance", "sign")?,
            planned_optional_str(&planned, ":signatures", "sign")?.map(PathBuf::from),
        )
    };
    let pkg = pkg_buf.as_path();
    let key_path = key_path_buf.as_path();
    let acceptance = acceptance_buf.as_deref();
    let signatures = signatures_buf.as_deref();

    let (_manifest, pkg_dir) = PackageManifest::load(pkg).map_err(|e| {
        let context = structured_failures::manifest_context("package/sign", &e);
        cli_err_with_context(EX_PARSE, "manifest/parse", format!("{e}"), context)
    })?;
    let store = gc_obligations::EvidenceStore::open(&pkg_dir).map_err(obligation_err)?;

    let acc_hex = match acceptance {
        Some(s) => s.trim().to_string(),
        None => gc_obligations::read_acceptance_hash_from_last(&pkg_dir).map_err(|e| match e {
            gc_obligations::SigningError::Io(_) => cli_err(EX_IO, "io/read", format!("{e}")),
            _ => cli_err(EX_PARSE, "sign/acceptance", format!("{e}")),
        })?,
    };

    let k = gc_obligations::KeyFile::load(key_path)
        .map_err(|e| cli_err(EX_PARSE, "sign/key", format!("{e}")))?;
    let sk = k
        .signing_key()
        .map_err(|e| cli_err(EX_PARSE, "sign/key", format!("{e}")))?;

    let acceptance_hash = gc_obligations::parse_hash32(&acc_hex)
        .map_err(|error| cli_err(EX_PARSE, "sign/acceptance", format!("{error}")))?;
    let canonical_acc_hex = acceptance_hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if acc_hex != canonical_acc_hex {
        return Err(cli_err(
            EX_PARSE,
            "sign/acceptance",
            "acceptance artifact must use canonical lowercase hexadecimal",
        ));
    }
    let public_key = sk.verifying_key().to_bytes();
    let mut authority = signing_authority(cli, "package signing authority")?;
    let message = authority
        .acceptance_message(acceptance_hash)
        .map_err(|error| cli_err(EX_VERIFY, "sign/authority", format!("{error}")))?;
    let signature = sk.sign(&message).to_bytes();
    let signature_valid = sk
        .verifying_key()
        .verify(&message, &ed25519_dalek::Signature::from_bytes(&signature))
        .is_ok();
    let signature_term = authority
        .acceptance_artifact(acceptance_hash, public_key, signature, signature_valid)
        .map_err(|error| cli_err(EX_VERIFY, "sign/authority", format!("{error}")))?;
    let sig_artifact = store.put_term(&signature_term).map_err(obligation_err)?;

    let genesis_dir = pkg_dir.join(".genesis");
    std::fs::create_dir_all(&genesis_dir)
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let sigset_path = signatures
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| gc_obligations::signatures_file_path(&pkg_dir));
    let set = match gc_obligations::load_signature_set(&sigset_path) {
        Ok(set) => set,
        Err(gc_obligations::SigningError::Io(error))
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            Vec::new()
        }
        Err(error) => {
            return Err(cli_err(EX_PARSE, "sign/signature-set", format!("{error}")));
        }
    };

    let pkg_artifact = gc_obligations::package_artifact_hash(pkg).map_err(obligation_err)?;
    let transparency_head_path = gc_obligations::transparency_head_path(&pkg_dir);
    let previous_head = match std::fs::read_to_string(&transparency_head_path) {
        Ok(text) => Some(
            gc_obligations::parse_hash32(text.trim())
                .map_err(|error| cli_err(EX_PARSE, "sign/transparency-head", format!("{error}")))?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(cli_err(
                EX_IO,
                "io/read",
                format!("{}: {error}", transparency_head_path.display()),
            ));
        }
    };
    let commit = authority
        .commit(
            &pkg_artifact,
            &acc_hex,
            &sig_artifact,
            &k.pk_b64,
            &set,
            previous_head,
        )
        .map_err(|error| cli_err(EX_VERIFY, "sign/authority", format!("{error}")))?;
    let transparency_entry = store
        .put_term(&commit.transparency_entry)
        .map_err(obligation_err)?;

    std::fs::write(
        genesis_dir.join("last_signature"),
        format!("{sig_artifact}\n"),
    )
    .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    let signature_set_term = Term::Vector(
        commit
            .signature_set
            .iter()
            .cloned()
            .map(Term::Str)
            .collect(),
    );
    if let Some(parent) = sigset_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    }
    std::fs::write(&sigset_path, print_term(&signature_set_term).as_bytes())
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    std::fs::write(&transparency_head_path, format!("{transparency_entry}\n"))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/sign-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "key": key_path.display().to_string(),
            "package_artifact": pkg_artifact,
            "acceptance_artifact": acc_hex,
            "signature_artifact": sig_artifact,
            "sigset": sigset_path.display().to_string(),
            "transparency_entry": transparency_entry,
            "pk_b64": k.pk_b64,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{sig_artifact}\n")
        },
        json: json_envelope_value(env)?,
    })
}

// SIGNING_AUTHORITY_ROUTES_END: static verifiers use this boundary rather than file EOF.

use super::*;

pub(super) fn driver_args(cli: &Cli, cmd: &BenchCmd) -> Result<Option<Vec<String>>, CliError> {
    let mut args = Vec::new();
    match cmd {
        BenchCmd::AgentCampaignPlan {
            campaign,
            phase,
            case,
            runner,
            agent_executable,
            model,
            model_revision,
            immutable_revision,
            reasoning_effort,
            timeout_ms,
            local_provider,
            model_artifact_sha256,
            hardware_class,
            out,
        } => {
            args.extend([
                "campaign-plan".to_string(),
                "--campaign".to_string(),
                campaign.clone(),
                "--phase".to_string(),
                phase.clone(),
                "--runner".to_string(),
                runner.clone(),
            ]);
            for case_id in case {
                args.extend(["--case".to_string(), case_id.clone()]);
            }
            cmd_bench::push_path(&mut args, "--agent-executable", agent_executable);
            args.extend([
                "--model".to_string(),
                model.clone(),
                "--model-revision".to_string(),
                model_revision.clone(),
                "--reasoning-effort".to_string(),
                reasoning_effort.clone(),
                "--timeout-ms".to_string(),
                timeout_ms.to_string(),
            ]);
            if *immutable_revision {
                args.push("--immutable-revision".to_string());
            }
            if let Some(provider) = local_provider {
                args.extend(["--local-provider".to_string(), provider.clone()]);
            }
            if let Some(digest) = model_artifact_sha256 {
                args.extend(["--model-artifact-sha256".to_string(), digest.clone()]);
            }
            args.extend(["--hardware-class".to_string(), hardware_class.clone()]);
            cmd_bench::push_path(&mut args, "--out", out);
        }
        BenchCmd::AgentPlan {
            case,
            campaign_predeclaration,
            out,
        } => {
            args.extend(["plan".to_string(), "--case".to_string(), case.clone()]);
            cmd_bench::push_path(
                &mut args,
                "--campaign-predeclaration",
                campaign_predeclaration,
            );
            cmd_bench::push_path(&mut args, "--out", out);
        }
        BenchCmd::AgentRun {
            campaign_predeclaration,
            predeclaration,
            agent_executable,
            out,
        } => {
            args.push("run".to_string());
            cmd_bench::push_path(
                &mut args,
                "--campaign-predeclaration",
                campaign_predeclaration,
            );
            cmd_bench::push_path(&mut args, "--predeclaration", predeclaration);
            cmd_bench::push_path(&mut args, "--agent-executable", agent_executable);
            cmd_bench::push_path(&mut args, "--out", out);
            let (genesis_bin, artifact) = cmd_bench::runtime_paths(cli, "bench agent-run")?;
            cmd_bench::push_path(&mut args, "--genesis-bin", &genesis_bin);
            cmd_bench::push_path(&mut args, "--selfhost-artifact", &artifact);
        }
        BenchCmd::AgentValidate { run } => {
            args.push("validate".to_string());
            cmd_bench::push_path(&mut args, "--run", run);
        }
        BenchCmd::AgentReplay { run } => {
            args.push("replay".to_string());
            cmd_bench::push_path(&mut args, "--run", run);
            let (genesis_bin, artifact) = cmd_bench::runtime_paths(cli, "bench agent-replay")?;
            cmd_bench::push_path(&mut args, "--genesis-bin", &genesis_bin);
            cmd_bench::push_path(&mut args, "--selfhost-artifact", &artifact);
        }
        _ => return Ok(None),
    }
    Ok(Some(args))
}

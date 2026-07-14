use assert_cmd::cargo::cargo_bin_cmd;

fn strip_ansi(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if code.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[test]
fn non_json_failure_is_catalog_derived_redacted_and_width_bounded() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("missing.gc");

    let output = cargo_bin_cmd!("genesis")
        .env("COLUMNS", "40")
        .env("NO_COLOR", "1")
        .env("CLICOLOR_FORCE", "1")
        .args(["parse", source.to_str().expect("utf-8 fixture path")])
        .assert()
        .failure()
        .get_output()
        .clone();

    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("utf-8 human diagnostic");
    assert!(stderr.starts_with("error[io/read]: io/read failed"));
    assert!(stderr.contains("missing.gc"));
    assert!(stderr.contains("  cause:"));
    assert!(stderr.contains("  next:"));
    assert!(!stderr.contains(temp.path().to_str().expect("utf-8 temp path")));
    assert!(
        !stderr.contains('\u{1b}'),
        "NO_COLOR must override forced color"
    );
    assert!(stderr.lines().all(|line| line.chars().count() <= 40));
}

#[test]
fn forced_color_changes_only_terminal_styling() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("missing.gc");
    let path = source.to_str().expect("utf-8 fixture path");

    let plain = cargo_bin_cmd!("genesis")
        .env("COLUMNS", "80")
        .env("NO_COLOR", "1")
        .args(["parse", path])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let colored = cargo_bin_cmd!("genesis")
        .env("COLUMNS", "80")
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .args(["parse", path])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();

    let plain = String::from_utf8(plain).expect("plain utf-8");
    let colored = String::from_utf8(colored).expect("colored utf-8");
    assert!(colored.contains("\u{1b}[1;31merror[io/read]"));
    assert!(colored.contains("\u{1b}[1mcause:\u{1b}[0m"));
    assert!(colored.contains("\u{1b}[1mnext:\u{1b}[0m"));
    assert_eq!(strip_ansi(&colored), plain);
}

#[test]
fn command_result_failure_uses_the_same_stderr_renderer() {
    let temp = tempfile::tempdir().expect("tempdir");
    let manifest = temp.path().join("missing-package.toml");
    let output = cargo_bin_cmd!("genesis")
        .env("COLUMNS", "60")
        .env("NO_COLOR", "1")
        .args([
            "test",
            "--pkg",
            manifest.to_str().expect("utf-8 fixture path"),
        ])
        .assert()
        .failure()
        .get_output()
        .clone();

    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("utf-8 human diagnostic");
    assert!(stderr.starts_with("error[manifest/error]: obligation/run failed"));
    assert!(stderr.contains("  cause:"));
    assert!(stderr.contains("  next:"));
    assert!(!stderr.contains(temp.path().to_str().expect("utf-8 temp path")));
    assert!(stderr.lines().all(|line| line.chars().count() <= 60));
}

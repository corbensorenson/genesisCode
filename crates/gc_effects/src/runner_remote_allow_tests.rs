use super::remote_allow_matches;

#[test]
fn remote_allow_rejects_host_confusion() {
    assert!(
        !remote_allow_matches(
            "https://trusted.example.com.evil",
            "https://trusted.example.com"
        )
        .expect("allow check")
    );
}

#[test]
fn remote_allow_accepts_exact_origin_and_path_prefix() {
    assert!(
        remote_allow_matches(
            "https://registry.example.com",
            "https://registry.example.com"
        )
        .expect("allow check")
    );
}

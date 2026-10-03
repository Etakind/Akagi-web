const WORKFLOW: &str = include_str!("../.github/workflows/pr-build.yml");

#[test]
fn pr_build_requires_admin_and_pins_the_authorized_head() {
    for checkout in [WORKFLOW.to_owned(), WORKFLOW.replace('\n', "\r\n")] {
        let normalized = checkout.replace("\r\n", "\n");
        assert!(normalized.contains("access.permission !== 'admin'"));
        assert!(normalized.contains("github.event.comment.body == '/build-artifacts'"));
        assert!(normalized.contains("core.setOutput('head_sha', pull.head.sha)"));
        assert!(normalized.contains("if: needs.authorize.outputs.authorized == 'true'"));
        assert!(normalized.contains("uses: ./.github/workflows/build-packages.yml"));
        assert!(normalized.contains("ref: ${{ needs.authorize.outputs.head_sha }}"));
        assert!(normalized.contains("repository: ${{ needs.authorize.outputs.head_repository }}"));
        assert!(
            !normalized.contains(": write"),
            "build requests need no write permission"
        );
    }
}

#[test]
fn pr_build_avoids_actions_with_node20_runtimes() {
    for deprecated in [
        "actions/github-script@v7",
        "actions/checkout@v4",
        "actions/setup-node@v4",
        "actions/cache/restore@v4",
        "actions/upload-artifact@v4",
        "arduino/setup-protoc@v3",
    ] {
        assert!(
            !WORKFLOW.contains(deprecated),
            "{deprecated} still uses the deprecated Node 20 action runtime"
        );
    }
}

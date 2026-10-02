const WORKFLOW: &str = include_str!("../.github/workflows/pr-build.yml");

#[test]
fn pr_build_comment_job_uses_pull_request_write_permission() {
    let lf = WORKFLOW.replace("\r\n", "\n");
    for checkout in [lf.clone(), lf.replace('\n', "\r\n")] {
        let normalized = checkout.replace("\r\n", "\n");
        let (_, comment_job) = normalized
            .split_once("\n  comment:\n")
            .expect("PR build workflow must contain a comment job");

        assert!(
            comment_job.contains("\n      pull-requests: write\n"),
            "the PR comment job must request pull-requests: write"
        );
        assert!(
            !comment_job.contains("\n      issues: write\n"),
            "the PR comment job must not rely on issues: write"
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

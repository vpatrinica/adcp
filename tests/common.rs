use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

pub fn replay_fixture_and_collect(fixture: &str) -> (TempDir, Vec<String>) {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let data_dir = tmp.path().join("data");
    // Write a minimal config pointing at the temp data dir
    let cfg_path = tmp.path().join("adcp.toml");
    let cfg = format!(
        "service_name = \"e2e-test\"\nserial_port = \"/dev/null\"\nlog_level = \"info\"\ndata_directory = \"{}\"\n",
        data_dir.display()
    );
    fs::write(&cfg_path, cfg).expect("write config");

    // Run the binary with the replay flag
    Command::new(assert_cmd::cargo::cargo_bin!("adcp"))
        .arg(&cfg_path)
        .arg("--replay")
        .arg(fixture)
        .assert()
        .success();

    let mut entries: Vec<String> = fs::read_dir(&data_dir)
        .unwrap_or_else(|_| panic!("failed to read data dir {}", data_dir.display()))
        .filter_map(|res| res.ok().and_then(|e| e.file_name().into_string().ok()))
        .collect();
    entries.sort();
    (tmp, entries)
}

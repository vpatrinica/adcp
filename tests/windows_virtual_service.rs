#[cfg(target_os = "windows")]
mod windows_tests {
    use adcp::platform;

    #[test]
    fn windows_template_mentions_windows_service() {
        let template = platform::platform_template();
        assert!(template.contains("ExecStart=adcp.exe"));
        assert!(template.contains("Restart=always"));
    }
}

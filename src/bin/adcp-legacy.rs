use adcp::{logging, platform, AppConfig, Service, simulator, config::ServiceMode};
use anyhow::{bail, Context, Result};

#[derive(Debug)]
struct Cli {
    config_path: String,
    replay: Option<String>,
}

impl Cli {
    fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let mut config_path: Option<String> = None;
        let mut replay: Option<String> = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    let value = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--config requires a path"))?;
                    config_path = Some(value);
                }
                "--replay" => {
                    let value = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--replay requires a path"))?;
                    replay = Some(value);
                }
                "--help" | "-h" => {
                    println!(
                        "Usage: adcp [--config <path>] [--replay <sample>]\n\
                         --config <path>   Path to TOML configuration (default: config/adcp.toml)\n\
                         --replay <path>   Replay a capture file through the pipeline and exit"
                    );
                    std::process::exit(0);
                }
                other => {
                    if config_path.is_none() {
                        config_path = Some(other.to_string());
                    } else {
                        bail!("unknown argument '{other}'");
                    }
                }
            }
        }

        Ok(Self {
            config_path: config_path.unwrap_or_else(|| AppConfig::default_path().into()),
            replay,
        })
    }
}

async fn cleanup_orphans(tmp_dir: &str) {
    if let Ok(rd) = std::fs::read_dir(tmp_dir) {
        let my_pid = std::process::id();
        for entry in rd.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".pid") {
                    let path = entry.path();
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(pid) = content.trim().parse::<u32>() {
                            if pid != my_pid {
                                tracing::info!(pid = pid, "cleaning up orphaned process");
                                #[cfg(unix)]
                                {
                                    unsafe { libc::kill(pid as i32, 9) };
                                }
                                #[cfg(windows)]
                                {
                                    let _ = std::process::Command::new("taskkill")
                                        .arg("/F")
                                        .arg("/PID")
                                        .arg(pid.to_string())
                                        .spawn()
                                        .and_then(|mut c| c.wait());
                                }
                                let _ = std::fs::remove_file(path);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse()?;

    let config = AppConfig::load(&cli.config_path)
        .with_context(|| format!("unable to load configuration from {}", cli.config_path))?;

    let guard = logging::init(&config)?;
    platform::log_platform_guidance();

    // Ensure deployment tmp exists and write PID file for this service
    let tmp_dir = "./deployment/tmp";
    std::fs::create_dir_all(tmp_dir).with_context(|| format!("failed to create tmp dir {}", tmp_dir))?;

    // Cleanup any orphaned processes from previous runs
    // ONLY if we are the orchestrator or in replay mode
    if matches!(config.mode, ServiceMode::Orchestrator) || cli.replay.is_some() {
        cleanup_orphans(tmp_dir).await;
    }

    // On Unix, make this process the leader of a new process group so we can signal children
    #[cfg(unix)]
    {
        unsafe { libc::setpgid(0, 0) }; // ignore errors; best-effort
    }
    let safe_name = config.service_name.replace(' ', "_");
    let pid_path = format!("{}/{}.pid", tmp_dir, safe_name);
    std::fs::write(&pid_path, format!("{}", std::process::id()))
        .with_context(|| format!("failed to write pid file {}", pid_path))?;

    // Spawn a task to remove the pid file on SIGINT/SIGTERM (Unix) or ctrl-c (Windows)
    // and attempt to gracefully shut down child processes by signaling the process group.
    let pid_path_clone = pid_path.clone();
    let tmp_dir_clone = tmp_dir.to_string();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            use tokio::time::{sleep, Duration};
            let mut sigint = signal(SignalKind::interrupt()).expect("signal handler");
            let mut sigterm = signal(SignalKind::terminate()).expect("signal handler");
            tokio::select! {
                _ = sigint.recv() => {},
                _ = sigterm.recv() => {},
            }
            // Remove pid file immediately (best-effort) so tests won't see stale PID
            let _ = std::fs::remove_file(&pid_path_clone);
            // Attempt graceful shutdown: send SIGINT to process group
            let pgid = -(std::process::id() as i32);
            unsafe { libc::kill(pgid, libc::SIGINT) }; // best-effort
            // Wait a short while for children to exit
            sleep(Duration::from_secs(3)).await;
            // Force kill any remaining processes in the group
            unsafe { libc::kill(pgid, libc::SIGKILL) };

            // Best-effort: cleanup any leftover adcp-*.pid files in deployment/tmp
            cleanup_orphans(&tmp_dir_clone).await;
        }
        #[cfg(windows)]
        {
            // On Windows, best-effort: trigger ctrl-c handler
            tokio::signal::ctrl_c().await.ok();
            cleanup_orphans(&tmp_dir_clone).await;
        }
        let _ = std::fs::remove_file(&pid_path_clone);
    });

    if let Some(sample) = cli.replay {
        let result = simulator::replay_sample(sample, &config).await?;
        if !result.failures.is_empty() {
            tracing::warn!("replay encountered {} failures", result.failures.len());
        }
        let _ = std::fs::remove_file(&pid_path);
        return Ok(());
    }

    let res = Service::new(config).run().await;
    // Attempt to remove pid file on exit (best-effort)
    let _ = std::fs::remove_file(&pid_path);
    // Drop the tracing_appender guard to flush logs
    drop(guard);
    res
}

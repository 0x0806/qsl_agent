use anyhow::Result;
use mimalloc::MiMalloc;
use std::sync::Arc;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use tokio_util::sync::CancellationToken;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod agent;
mod config;
mod context;
mod cost;
mod error;
mod events;
mod health;
mod memory;
mod metrics;
mod provider;
mod security;
mod session;
mod session_registry;
mod tools;
mod transport;
mod turn_budget;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("qsl_agent=info,warn"));

    let cfg = config::Config::load()?;
    let log_dir = cfg.state_dir.join("logs");
    std::fs::create_dir_all(&log_dir)?;
    let file_appender = tracing_appender::rolling::daily(&log_dir, "qsl-agent.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().json())
        .with(fmt::layer().json().with_writer(non_blocking));

    registry.init();

    let _prom = metrics_exporter_prometheus::PrometheusBuilder::new()
        .with_http_listener(([127, 0, 0, 1], 9999))
        .install_recorder()?;
    metrics::init();
    tracing::info!("config loaded; metrics on :9999");

    let shutdown = CancellationToken::new();
    {
        let token = shutdown.clone();
        tokio::spawn(async move {
            // ⚠️ VERIFY: SignalKind is unix-only; guard for Windows if you target it.
            let mut sigint = tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::interrupt()).unwrap();
            let mut sigterm = tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::terminate()).unwrap();
            tokio::select! {
                _ = sigint.recv()  => tracing::info!("SIGINT"),
                _ = sigterm.recv() => tracing::info!("SIGTERM"),
            }
            token.cancel();
        });
    }

    // SIGHUP: config hot-reload attempt (logs new config, partial adoption)
    {
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            let Ok(mut hup) = signal(SignalKind::hangup()) else { return };
            while hup.recv().await.is_some() {
                match config::Config::load() {
                    Ok(c) => tracing::info!(model = %c.provider.model, "SIGHUP: reloaded config"),
                    Err(e) => tracing::warn!(?e, "SIGHUP reload failed; keeping old config"),
                }
            }
        });
    }

    let provider = Arc::new(provider::Provider::new(&cfg)?);
    if let Err(e) = provider.probe().await {
        tracing::warn!(?e, "startup provider probe failed — continuing");
    }

    let security = Arc::new(security::SecurityGuard::new(&cfg)?);
    let memory   = Arc::new(memory::Memory::new(&cfg).await?);
    let events   = events::EventLog::open(&cfg.state_dir).await?;
    let cost     = Arc::new(cost::CostTracker::new(
        cfg.cost.input_usd_per_million,
        cfg.cost.output_usd_per_million,
    ));
    let registry = Arc::new(session_registry::SessionRegistry::new(&cfg.state_dir));

    let health_handle = health::HealthMonitor::spawn(cfg.health.clone(), shutdown.clone());
    let session = session::Session::open_or_create(&cfg, events.clone(), memory.clone()).await?;
    tracing::info!(id = %session.id, "session ready");

    let tools = tools::default_registry(&security, &cfg)?;
    let (agent, handle) = agent::Agent::new(
        provider, tools, session, memory.clone(), security, events,
        cfg.agent.clone(), health_handle, cost, cfg.provider.tier,
    );

    let transport = match std::env::var("QSL_TRANSPORT").as_deref() {
        Ok("http") => transport::Transport::Http("127.0.0.1:8765".parse().unwrap()),
        Ok("unix") => transport::Transport::Unix(cfg.state_dir.join("qsl.sock")),
        _ => transport::Transport::Stdio,
    };

    let agent_task = tokio::spawn(agent.run(shutdown.clone()));
    transport::serve(transport, handle, registry, shutdown.clone()).await?;
    let _ = agent_task.await;

    memory.flush().await?;
    tracing::info!("clean shutdown");
    Ok(())
}

const DEFAULT_LOG_FILTER: &str = concat!(
    "modde_ui=debug,",
    "modde_core=debug,",
    "modde_games=debug,",
    "modde_sources=debug,",
    "modde=debug,",
    "iced=warn,",
    "iced_wgpu=warn,",
    "wgpu=warn,",
    "naga=warn"
);

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER)),
        )
        .init();

    tracing::info!("starting modde-ui");

    modde_ui::app::run()
}

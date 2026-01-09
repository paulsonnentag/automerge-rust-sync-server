use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

pub fn initialize_tracing() {
    let stdout_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_filter(EnvFilter::new("info")
            .add_directive("samod=trace".parse().unwrap())
            .add_directive("samod_core=trace".parse().unwrap()));
        
    if let Err(e) = tracing_subscriber::registry()
        .with(stdout_layer)
        .try_init()
    {
        tracing::error!("Failed to initialize tracing subscriber: {:?}", e);
    } else {
        tracing::info!("Tracing subscriber initialized");
    }
}

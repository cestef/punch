use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_thread_ids(false)
                .with_thread_names(false),
        )
        .with({
            let mut filter =
                EnvFilter::from_env(format!("{}_LOG", env!("CARGO_PKG_NAME").to_uppercase()));
            for directive in &[
                "rustls",
                "hyper_util",
                "reqwest",
                "iroh",
                "acto",
                "portmapper",
                "swarm_discovery",
                "hickory_proto",
                "hickory_resolver",
                "events",
                "igd_next",
            ] as &[&str]
            {
                let directive = format!("{}=off", directive)
                    .parse()
                    .map_err(|e| anyhow::anyhow!("Failed to parse directive: {}", e))?;
                filter = filter.add_directive(directive);
            }
            filter
        })
        .init();
    Ok(())
}

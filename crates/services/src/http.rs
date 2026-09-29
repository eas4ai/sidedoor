//! HTTP clients that go through the proxy the environment names.

/// An agent for requests to `url`, through the environment's proxy unless
/// `NO_PROXY` or a loopback address says to connect directly.
pub fn agent(url: &str, builder: ureq::AgentBuilder) -> ureq::Agent {
    match platform::proxy::for_url(url).and_then(|proxy| ureq::Proxy::new(proxy).ok()) {
        Some(proxy) => builder.proxy(proxy),
        None => builder,
    }
    .build()
}

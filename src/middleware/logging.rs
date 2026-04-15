use tower_http::classify::ServerErrorsAsFailures;
use tower_http::classify::SharedClassifier;
use tower_http::trace::TraceLayer;

/// Create the tracing layer for request/response logging.
pub fn trace_layer() -> TraceLayer<SharedClassifier<ServerErrorsAsFailures>> {
    TraceLayer::new_for_http()
}

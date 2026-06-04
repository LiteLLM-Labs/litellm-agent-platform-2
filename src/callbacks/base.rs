use crate::callbacks::standard_logging::StandardLoggingPayload;

pub trait BaseCallback: Send + Sync + 'static {
    fn on_success(&self, payload: StandardLoggingPayload);
    fn on_error(&self, payload: StandardLoggingPayload);
}

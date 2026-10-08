//! DeepSeek completions through the shared Chat Completions wire protocol.
use bindings::exports::enco::plugin::{completion, lifecycle};
use provider_protocol::{Completion, Request, Settings, types::Failure};

mod bindings {
    #![expect(
        unsafe_code,
        reason = "the export macro expands to the generated canonical ABI glue"
    )]
    wit_bindgen::generate!({
        path: "../../wit",
        world: "completion-plugin",
        with: {
            "enco:plugin/host@0.2.1": provider_protocol::host,
            "enco:plugin/types@0.2.1": provider_protocol::types,
            "enco:plugin/completion@0.2.1/request": provider_protocol::Request,
            "enco:plugin/completion@0.2.1/completion": provider_protocol::Completion,
            "enco:plugin/completion@0.2.1/usage": provider_protocol::Usage,
            "enco:plugin/completion@0.2.1/stop-reason": provider_protocol::StopReason,
        },
    });
    use super::Plugin;
    export!(Plugin with_types_in self);
}

struct Plugin;

impl lifecycle::Guest for Plugin {
    fn describe(_config: String) -> Result<lifecycle::Description, Failure> {
        Ok(lifecycle::Description {
            summary: "DeepSeek Chat Completions".into(),
        })
    }

    async fn probe() -> Result<(), Failure> {
        Ok(())
    }
}

impl completion::Guest for Plugin {
    async fn complete(settings: Settings, request: Request) -> Result<Completion, Failure> {
        provider_protocol::complete(settings, request).await
    }
}

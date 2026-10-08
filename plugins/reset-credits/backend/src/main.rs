use codex_proxy_plugin_reset_credits::{PLUGIN_ID, manifest, plugin};
use gateway_plugin_sdk::client::{PluginSession, SessionConfig, SessionError};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = PluginSession::accept(
        tokio::io::stdin(),
        tokio::io::stdout(),
        SessionConfig::default(),
    )
    .await?;
    let manifest = manifest()?;
    if session.handshake().plugin_id != PLUGIN_ID
        || session.handshake().contributes != manifest.contributes
        || !session
            .handshake()
            .configuration
            .as_object()
            .is_some_and(serde_json::Map::is_empty)
    {
        return Err(SessionError::Handshake.into());
    }
    session.run(plugin()?).await?;
    Ok(())
}

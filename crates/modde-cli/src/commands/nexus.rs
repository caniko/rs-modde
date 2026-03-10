use anyhow::Result;

use crate::NexusAction;

pub async fn handle(action: NexusAction) -> Result<()> {
    match action {
        NexusAction::Auth => {
            // TODO: prompt for API key, store in keyring
            println!("Enter your Nexus API key:");
            println!("(API key storage not yet implemented)");
        }
        NexusAction::Status => {
            // TODO: load key, validate, show premium status
            println!("Checking Nexus API status...");
            println!("(Not yet implemented)");
        }
    }
    Ok(())
}

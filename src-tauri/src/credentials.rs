use anyhow::{anyhow, Result};
use keyring::Entry;

const SERVICE: &str = "ToddlerClaude";

pub fn set(key: &str, value: &str) -> Result<()> {
    let entry = Entry::new(SERVICE, key)?;
    entry.set_password(value)?;
    Ok(())
}

pub fn get(key: &str) -> Result<Option<String>> {
    let entry = Entry::new(SERVICE, key)?;
    match entry.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow!(e)),
    }
}

pub fn delete(key: &str) -> Result<()> {
    let entry = Entry::new(SERVICE, key)?;
    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow!(e)),
    }
}

pub mod keys {
    pub const CLAUDE_TOKEN: &str = "claude_oauth_token";
    pub const GITHUB_TOKEN: &str = "github_pat";
    pub const FLY_TOKEN: &str = "fly_api_token";
}

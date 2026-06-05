use std::{path::PathBuf, sync::OnceLock};

use chrono::{DateTime, Utc};
use discordrs::Snowflake;
use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use log::debug;
use serde::Deserialize;

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn load() -> &'static Config {
    debug!("loading config");
    let config: Config = Figment::new()
        .merge(Toml::file("discord-mail-bridge.toml"))
        .merge(Env::prefixed("DMB_"))
        .extract()
        .unwrap();
    debug!("got config: {config:?}");
    CONFIG.set(config).unwrap();

    CONFIG.get().unwrap()
}

#[derive(Deserialize, Clone)]
pub struct BridgeConfig {
    pub discord_channel_id: Snowflake,

    pub email_to_name: Option<String>,
    pub email_to_address: String,
    pub email_from_name: Option<String>,
    pub email_from_address: String,
    pub extra_header: Option<String>,

    pub smtp_url: String,
    pub smtp_port: Option<u16>,
    #[serde(default = "def_false")]
    pub smtp_insecure: bool,
    pub smtp_username: Option<String>,
    pub smtp_password: Option<String>,
}

const fn def_false() -> bool {
    false
}

#[derive(Deserialize)]
pub struct Config {
    pub discord_token: String,
    pub discord_app_id: u64,
    pub bridges: Vec<BridgeConfig>,
    pub state_dir: PathBuf,

    pub debug_fake_last_success_time: Option<DateTime<Utc>>,
}

impl std::fmt::Debug for BridgeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BridgeConfig")
            .field("discord_channel_id", &self.discord_channel_id)
            .field("email_to_name", &self.email_to_name)
            .field("email_to_address", &self.email_to_address)
            .field("email_from_name", &self.email_from_name)
            .field("email_from_address", &self.email_from_address)
            .field("smtp_url", &self.smtp_url)
            .field("smtp_username", &self.smtp_username)
            .field("smtp_password", &"SECRET")
            .field("smtp_insecure", &self.smtp_insecure)
            .field("smtp_port", &self.smtp_port)
            .field("extra_header", &self.extra_header)
            .finish()
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("discord_token", &"SECRET")
            .field("discord_app_id", &self.discord_app_id)
            .field("bridges", &self.bridges)
            .finish_non_exhaustive()
    }
}

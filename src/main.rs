#![deny(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::must_use_candidate, clippy::missing_errors_doc)]

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use discordrs::{
    Attachment as DiscordAttachment, DiscordHttpClient, Message as DiscordMessage, RestClient,
};
use futures_util::stream::FuturesUnordered;
use lettre::{
    AsyncSmtpTransport, AsyncTransport,
    message::{
        Attachment, Mailbox, Message as EmailMessage, MultiPart, SinglePart, header::ContentType,
    },
    transport::smtp::authentication::Credentials,
};
use log::{debug, info, warn};
use tokio::fs;
use tokio_stream::StreamExt;

use crate::config::{BridgeConfig, Config};

mod config;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // Boring setup bits
    env_logger::init();
    let config = config::load();

    // Race all of the bridging tasks
    let mut bridges = Vec::with_capacity(config.bridges.len());
    for bridge in &config.bridges {
        bridges.push(DiscordToEmailBridger::new(config, bridge).await?);
    }

    // Race all tasks until they finish, exiting on first error
    let mut all = bridges
        .iter_mut()
        .map(DiscordToEmailBridger::try_send_digest)
        .collect::<FuturesUnordered<_>>();
    while let Some(result) = all.next().await {
        result?;
    }

    Ok(())
}

/// Responsible for bridging discord messages to email, by sending a periodic digest
struct DiscordToEmailBridger {
    bridge: BridgeConfig,
    discord_client: RestClient,
    channel_name: String,
    state_file: PathBuf,
    last_successful_digest: DateTime<Utc>,
}

impl DiscordToEmailBridger {
    pub async fn new(config: &Config, bridge: &BridgeConfig) -> Result<Self> {
        let state_file = Self::state_file_path(&config.state_dir, bridge);

        let state_file_modified: Option<DateTime<Utc>> = match fs::metadata(&state_file).await {
            Ok(metadata) => metadata.modified().ok().map(Into::into),
            Err(e) => {
                warn!(
                    "failed to read metadata file {state_file:?}: {e}. using now as last successful digest instead"
                );
                None
            }
        };

        let last_successful_digest = config
            .debug_fake_last_success_time
            .or(state_file_modified)
            .unwrap_or_else(Utc::now);

        // Ensure state file exists, and has correct modification time
        fs::create_dir_all(state_file.parent().unwrap()).await?;
        fs::File::create(&state_file)
            .await?
            .try_into_std() // tokio is missing this api :(
            .unwrap()
            .set_modified(last_successful_digest.into())?;

        let client = DiscordHttpClient::new(&config.discord_token, config.discord_app_id);

        // Cache the channel name, for using in email subjects
        let channel = client
            .get_channel(bridge.discord_channel_id.clone())
            .await?;

        Ok(Self {
            bridge: bridge.clone(),
            discord_client: client,
            last_successful_digest,
            state_file,
            channel_name: channel.name.unwrap(),
        })
    }

    async fn try_send_digest(&mut self) -> Result<()> {
        debug!("trying to do a digest");

        // get new messages since last_successful_digest
        let messages = self.get_new_messages().await?;
        debug!("found {:?} messages", messages.len());

        if !messages.is_empty() {
            // generate and send the email
            let email = self.build_digest_email(&messages).await?;
            info!("sending email with {} discord messages", messages.len());
            debug!("full email: {email:?}");

            let mailer = AsyncSmtpTransport::<lettre::Tokio1Executor>::relay(&self.bridge.smtp_url)
                .unwrap()
                .credentials(Credentials::new(
                    self.bridge.smtp_username.clone(),
                    self.bridge.smtp_password.clone(),
                ))
                .build();

            mailer.send(email).await?;
            info!("sent successfully");
        }

        self.last_successful_digest = Utc::now();
        fs::File::create(&self.state_file)
            .await?
            .try_into_std() // tokio is missing this api :(
            .unwrap()
            .set_modified(self.last_successful_digest.into())?;
        Ok(())
    }

    async fn get_new_messages(&self) -> Result<Vec<DiscordMessage>> {
        const MAX_PER_PAGE: u64 = 10;
        let mut messages = Vec::new();

        let mut response = self
            .discord_client
            .get_channel_messages(self.bridge.discord_channel_id.clone(), Some(MAX_PER_PAGE))
            .await?;

        let get_message_timestamp =
            |x: &DiscordMessage| -> Result<DateTime<Utc>, chrono::ParseError> {
                chrono::DateTime::parse_from_rfc3339(
                    x.edited_timestamp
                        .as_deref()
                        .or(x.timestamp.as_deref())
                        .unwrap(),
                )
                .map(DateTime::from)
            };

        loop {
            if response.is_empty() {
                break;
            }

            let count_before = messages.len();
            let count_in_page = response.len();
            messages.extend(response.into_iter().filter(|x| {
                get_message_timestamp(x).is_ok_and(|ts| ts > self.last_successful_digest)
            }));

            let num_added = messages.len() - count_before;
            if num_added < count_in_page {
                break;
            }

            response = self
                .discord_client
                .get_channel_messages_paginated(
                    self.bridge.discord_channel_id.clone(),
                    Some(MAX_PER_PAGE),
                    messages.last().map(|m| m.id.clone()),
                    None,
                    None,
                )
                .await?;
        }

        Ok(messages)
    }

    async fn build_digest_email(&self, messages: &[DiscordMessage]) -> Result<EmailMessage> {
        const MAX_ATTACHMENT_SIZE_BYTES: u64 = 5 * 1024 * 1024; // 5 MiB
        EmailMessage::builder()
            .from(Mailbox::new(
                self.bridge.email_from_name.clone(),
                self.bridge.email_from_address.parse().unwrap(),
            ))
            .to(Mailbox::new(
                self.bridge.email_to_name.clone(),
                self.bridge.email_to_address.parse().unwrap(),
            ))
            .subject(format!(
                "[discord-bridge] {} message{} in #{}",
                messages.len(),
                if messages.len() > 1 { "s" } else { "" },
                self.channel_name
            ))
            .header(ContentType::TEXT_PLAIN)
            .multipart(async {
                let mut parts = MultiPart::mixed()
                       .singlepart(
                           SinglePart::plain({
                               let mut body = format!(
                                   "This is an automatic email with new messages sent to #{} since {}.\nThe below text is from users of that channel, be cautious about clicking links, etc.\nReplies will currently not be bridged back to Discord.\n{}\n\n",
                                   self.channel_name,
                                   self.last_successful_digest.format("%Y-%m-%d %H:%M:%S"),
                                   self.bridge.extra_header.as_deref().unwrap_or("")
                               );

                               for message in messages {
                                   body.push_str(&message.author.as_ref().unwrap().username);
                                   body.push_str(": ");
                                   body.push_str(&message.content);
                                   for attachment in &message.attachments {
                                       body.push_str("<Attachment named '");
                                       body.push_str(&attachment.filename);
                                       body.push('\'');
                                       if attachment.size.is_some_and(|s| s > MAX_ATTACHMENT_SIZE_BYTES) {
                                           body.push_str(" which is bigger than 5MiB, so is not attached to this email");
                                       }
                                       body.push('>');
                                   }

                                   body.push('\n');
                                   body.push('\n');
                               }

                               body
                            }));


                for attachment in messages.iter().flat_map(|m| m.attachments.iter()) {
                    if attachment.size.is_some_and(|s| s > MAX_ATTACHMENT_SIZE_BYTES) {
                        continue;
                    }

                    if let Ok(at) = Self::fetch_attachment(attachment).await {
                        parts = parts.singlepart(at);
                    }
                }

                parts
            }.await)
            .map_err(Into::into)
    }

    async fn fetch_attachment(attachment: &DiscordAttachment) -> Result<SinglePart> {
        let url = attachment
            .url
            .clone()
            .ok_or_else(|| anyhow!("attachment has no url"))?;
        let filename = &attachment.filename;
        let content_type = attachment
            .content_type
            .clone()
            .ok_or_else(|| anyhow!("attachment has no content_type"))?;

        let content = reqwest::get(url).await?.bytes().await?.to_vec();
        Ok(Attachment::new(filename.clone()).body(content, content_type.parse()?))
    }

    fn state_file_path(state_dir: &Path, bridge: &BridgeConfig) -> PathBuf {
        let mut pb = state_dir.to_path_buf();
        pb.push(format!(
            "{}-to-{}",
            bridge.discord_channel_id, bridge.email_to_address
        ));

        pb
    }
}

use anyhow::anyhow;
use chrono::{DateTime, FixedOffset, Utc};
use errors::WakeBotError;
use rand::Rng;
use regex::Regex;
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::prelude::GuildChannel;
use serenity::prelude::*;
use shuttle_persist::PersistInstance;
use shuttle_secrets::SecretStore;
use std::time::Duration;
use youtube::VideoResult;

mod errors;
mod youtube;

const DEFAULT_TIMESTAMP: &str = "2023-02-21T00:00:00Z";

struct Handler {
    youtube_api_key: String,
    persist: PersistInstance,
    allowed_channels: Vec<String>,
}

impl Handler {
    async fn retrieve(&self) -> Result<VideoResult, Box<dyn std::error::Error>> {
        let timestamp = if let Ok(stamp) = self.persist.load::<DateTime<FixedOffset>>("timestamp") {
            stamp
        } else {
            DateTime::parse_from_rfc3339(DEFAULT_TIMESTAMP)
                .expect("Issue parsing default timestamp")
        };
        let new_videos = match youtube::get_new_videos(&self.youtube_api_key, timestamp).await {
            Ok(data) => data,
            Err(e) => {
                println!("Big ol' err: {}", e);
                return Err(anyhow!("There was a problem fetching new youtube videos").into());
            }
        };
        Ok(new_videos)
    }

    async fn send_update_message(
        &self,
        ctx: Context,
        triggering_msg: serenity::model::channel::Message,
    ) {
        let video_result = if let Ok(result) = self.retrieve().await {
            result
        } else {
            return ();
        };
        let timestamp = if let Ok(stamp) = self.persist.load::<DateTime<FixedOffset>>("timestamp") {
            stamp
        } else {
            return ();
        };
        let timestamp = timestamp.format("%Y/%m/%d %H:%M:%S");
        let mut video_list = video_result.list.to_vec();
        if video_list.is_empty() {
            match triggering_msg
                .channel_id
                .say(
                    &ctx.http,
                    format!("No videos added to Bael's playlist since {}", timestamp),
                )
                .await
            {
                Ok(msg) => println!("Message sent: {:#?}", msg),
                Err(e) => println!("Error sending message: {}", e),
            }
            self.persist
                .save::<DateTime<FixedOffset>>(
                    "timestamp",
                    DateTime::parse_from_rfc3339(&Utc::now().to_rfc3339()).unwrap(),
                )
                .expect("Problem persisting timestamp.");
            return ();
        }
        video_list.sort_by(|a, b| {
            DateTime::parse_from_rfc3339(&a.timestamp)
                .unwrap()
                .cmp(&DateTime::parse_from_rfc3339(&b.timestamp).unwrap())
        });
        let updated_timestamp = DateTime::parse_from_rfc3339(&video_list.last().unwrap().timestamp);
        let video_string = video_result
            .list
            .to_vec()
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                format!(
                    "{}. {} - https://www.youtube.com/watch?v={}",
                    idx + 1,
                    item.title,
                    item.id
                )
            })
            .fold(String::new(), |a, b| a + &b + "\n");
        let msg = if video_result.overflow {
            format!("More than five videos have been added to Bael's playlist since {} UTC.\nDisplaying the last five.\n\n{}", timestamp, video_string)
        } else {
            let has_one_video = video_list.len() == 1;
            format!(
                "{} video{} {} been added to Bael's playlist since {} UTC.\n\n{}",
                video_list.len(),
                if has_one_video { "" } else { "s" },
                if has_one_video { "has" } else { "have" },
                timestamp,
                video_string
            )
        };
        match triggering_msg.channel_id.say(&ctx.http, msg).await {
            Ok(msg) => println!("Message sent: {:#?}", msg),
            Err(e) => println!("Error sending message: {}", e),
        }
        self.persist
            .save::<DateTime<FixedOffset>>("timestamp", updated_timestamp.unwrap())
            .expect("Problem persisting timestamp.");
        tokio::time::sleep(Duration::from_secs(60 * 60 * 24)).await; // One day
        let _ = self.send_update_message(ctx, triggering_msg);
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        let content = msg.content.trim();
        let creator_message = msg.author.name == "Roren" && msg.author.discriminator == 5950;
        let dice_regex = Regex::new(r"^!(\d+)?d(\d+)(?:\s?([+-]\s?\d+))?$").unwrap();
        if self.allowed_channels.contains(&msg.channel_id.to_string())
            && dice_regex.is_match(content)
        {
            let get_captured_nums =
                || -> Result<(u32, u32, Option<i32>), Box<dyn std::error::Error>> {
                    if let Some(cap) = dice_regex.captures_iter(content).next() {
                        let quantity_re = Regex::new(r"^!\d+").unwrap();
                        let quantity_str = if quantity_re.is_match(content) {
                            &cap[1]
                        } else {
                            "1"
                        };
                        let bonus_re = Regex::new(r"^!(\d+)?d(\d+)(?:\s?([+-]\s?\d+))$").unwrap();
                        let bonus_str = if bonus_re.is_match(content) {
                            &cap[3]
                        } else {
                            "0"
                        };
                        let (quantity, max, bonus) = (quantity_str, &cap[2], bonus_str);
                        let quantity: u32 = quantity.parse().or::<u32>(Ok(1)).unwrap();
                        let max: u32 = max.parse()?;
                        let bonus = bonus.replace(" ", "").parse::<i32>().ok();
                        Ok((quantity, max, bonus))
                    } else {
                        Err(Box::new(WakeBotError::new("Problem parsing dice input")))
                    }
                };

            let (quantity, max, bonus) = if let Ok(vals) = get_captured_nums() {
                (vals.0, vals.1, vals.2.or(Some(0)).unwrap())
            } else {
                return ();
            };

            if max == 0 {
                return ();
            }
            let mut total: i32 = 0;
            let mut rolls = vec![];
            for _ in 0..quantity {
                let roll_result: i32 = rand::thread_rng()
                    .gen_range(1..=max)
                    .try_into()
                    .expect("Too big for signed integer to hold");
                rolls.push(roll_result);
                total += roll_result;
            }
            let roll_total = total;
            total += bonus;
            let rolls_string = rolls
                .iter()
                .fold(String::new(), |a, b| a + &b.to_string() + " + ");
            let rolls_string = rolls_string.trim_end_matches(" + ");
            match msg
                .reply(
                    &ctx.http,
                    format!(
                        "{}d{} ({})\n{}{}{} = {}",
                        quantity,
                        max,
                        if rolls.len() == 1 {
                            roll_total.to_string()
                        } else {
                            format!("{} = {}", rolls_string, roll_total)
                        },
                        roll_total,
                        if bonus >= 0 { "+" } else { "" },
                        bonus,
                        total
                    ),
                )
                .await
            {
                Ok(_) => println!("Reply sent with result"),
                Err(e) => println!("There was a problem sending result: {}", e),
            };
            return ();
        }

        if creator_message {
            if content.eq("!wakebot init") {
                self.send_update_message(ctx, msg).await;
            } else if content.eq("!wakebot reset") {
                std::process::Command::new("ping");
                self.persist
                    .save(
                        "timestamp",
                        DateTime::parse_from_rfc3339(DEFAULT_TIMESTAMP).unwrap(),
                    )
                    .unwrap();
                msg.channel_id.say(&ctx.http, "Bot reset").await.unwrap();
            }
        }
    }
    async fn ready(&self, _ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
    async fn channel_create(&self, _ctx: Context, _channel: &GuildChannel) {
        println!("Channel create");
    }
}

#[shuttle_service::main]
pub async fn serenity(
    #[shuttle_secrets::Secrets] secret_store: SecretStore,
    #[shuttle_persist::Persist] persist: PersistInstance,
) -> shuttle_service::ShuttleSerenity {
    let discord_token = if let Some(token) = secret_store.get("DISCORD_TOKEN") {
        token
    } else {
        return Err(anyhow!("'DISCORD_TOKEN' was not found").into());
    };
    let youtube_api_key = if let Some(key) = secret_store.get("YOUTUBE_API_KEY") {
        key
    } else {
        return Err(anyhow!("'YOUTUBE_API_KEY' was not found").into());
    };
    let test_channel_id = if let Some(id) = secret_store.get("TEST_CHANNEL_ID") {
        id
    } else {
        return Err(anyhow!("'TEST_CHANNEL_ID' was not found").into());
    };

    let outsiders_channel_id = if let Some(id) = secret_store.get("OUTSIDERS_CHANNEL_ID") {
        id
    } else {
        return Err(anyhow!("'OUTSIDERS_CHANNEL_ID' was not found").into());
    };

    let intents =
        GatewayIntents::GUILDS | GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(&discord_token, intents)
        .event_handler(Handler {
            youtube_api_key,
            persist,
            allowed_channels: vec![outsiders_channel_id, test_channel_id],
        })
        .await
        .expect("Err creating client");

    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
    Ok(client)
}

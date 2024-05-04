use anyhow::anyhow;
use aws::{
    add_or_update_action, create_aws_client, create_credentials_provider, delete_action,
    get_action_roll, increment_hehs, Action, WakeBotDbError,
};
use chrono::{DateTime, FixedOffset, Utc};
use fancy_regex::Regex;
use rolls_v2::{format_rolls_result_new, interpret_rolls, DICE_COMMAND_REGEX};
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::prelude::GuildChannel;
use serenity::prelude::*;
use shunting::{MathContext, ShuntingParser};
use shuttle_persist::PersistInstance;
use std::{collections::HashMap, time::Duration};
use youtube::VideoResult;

mod aws;
mod errors;
mod rolls_v2;
mod youtube;

const DEFAULT_TIMESTAMP: &str = "2023-02-21T00:00:00Z";

struct Handler {
    aws_client: aws_sdk_dynamodb::Client,
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
        if msg.author.bot {
            return;
        }
        if self.allowed_channels.contains(&msg.channel_id.to_string()) {
            if content.starts_with("!action ") {
                let args = content.split(" ").collect::<Vec<&str>>();
                if args.len() < 2 {
                    msg.reply(&ctx.http, "Invalid request sent for action.\nTo add, format like: !action <name> <roll>\nTo use, format like: !action <name>").await.expect("Failed to reply");
                }
                let action_name = String::from(args[1]);
                if action_name.eq("heh") {
                    msg.reply(&ctx.http, "Cannot use action 'heh' due to Ed's laziness.")
                        .await
                        .expect("Failed to reply");
                    return;
                }
                let valid_action_regex = Regex::new(r"^[a-zA-Z0-9_-]+$").unwrap();
                if !valid_action_regex.is_match(&action_name).unwrap_or(false) {
                    msg.reply(&ctx.http, "Invalid action name")
                        .await
                        .expect("Failed to reply");
                    return;
                }
                if args.len() == 2 {
                    let roll = match get_action_roll(&self.aws_client, &action_name).await {
                        Ok(r) => r,
                        Err(WakeBotDbError::NotFound(_)) => {
                            msg.reply(
                                &ctx.http,
                                format!("No action named '{}' found.", action_name),
                            )
                            .await
                            .expect("Problem sending response");
                            return;
                        }
                        _ => {
                            msg.reply(
                                &ctx.http,
                                String::from("There was a problem while fetching action."),
                            )
                            .await
                            .expect("Problem sending response");
                            return;
                        }
                    };
                    let rolls_result = interpret_rolls(&roll, 0);
                    if let Ok(result) = rolls_result {
                        match msg.reply(&ctx.http, format_rolls_result_new(result)).await {
                            Ok(_) => println!("Reply sent with result"),
                            Err(e) => println!("There was a problem sending result: {}", e),
                        };
                    }
                } else if args[1].eq("delete") {
                    if args.len() > 3 {
                        msg.reply(
                            &ctx.http,
                            "Invalid delete request.\nFormat should be '!action delete <name>'",
                        )
                        .await
                        .expect("Failed to reply");
                        return;
                    }
                    if let Some(name) = args.get(2) {
                        let item_existed = get_action_roll(&self.aws_client, name).await.is_ok();
                        if !item_existed {
                            msg.reply(&ctx.http, format!("Action '{}' does not exist.", name))
                                .await
                                .expect("Failed to reply");
                            return;
                        }
                        if let Ok(_) = delete_action(&self.aws_client, name).await {
                            msg.reply(&ctx.http, "Action deleted.")
                                .await
                                .expect("Failed to reply");
                            return;
                        } else {
                            msg.reply(&ctx.http, "Failed to delete action.")
                                .await
                                .expect("Failed to reply");
                            return;
                        }
                    } else {
                        msg.reply(
                            &ctx.http,
                            "Invalid delete request.\nFormat should be '!action delete <name>'",
                        )
                        .await
                        .expect("Failed to reply");
                        return;
                    }
                } else {
                    let roll_input = args[2..].join(" ");
                    // Use regex to validate roll string
                    let roll_regex = Regex::new(DICE_COMMAND_REGEX).unwrap();
                    if !roll_regex.is_match(&roll_input).unwrap_or(false) {
                        msg.reply(&ctx.http, "Invalid roll string")
                            .await
                            .expect("Failed to reply");
                        return;
                    }
                    let item_existed = get_action_roll(&self.aws_client, &action_name)
                        .await
                        .is_ok();

                    if let Ok(_) = add_or_update_action(
                        &self.aws_client,
                        Action {
                            name: String::from(&action_name),
                            roll: roll_input,
                        },
                    )
                    .await
                    {
                        // Send msg
                        msg.reply(
                            &ctx.http,
                            format!(
                                "Action '{}' {}.",
                                action_name,
                                if item_existed { "updated" } else { "created" }
                            ),
                        )
                        .await
                        .expect("Failed to reply");
                        return;
                    } else {
                        msg.reply(&ctx.http, "Failed to add action.")
                            .await
                            .expect("Failed to reply");
                        return;
                    }
                }
            }
            let dice_command_regex = Regex::new(DICE_COMMAND_REGEX).unwrap();
            let commands_regex = Regex::new(r"( ((--)|—)(\w+))+$").unwrap();
            let command_regex = Regex::new(r" ((--)|—)(\w+)").unwrap();
            if dice_command_regex.is_match(content).unwrap_or(false) {
                let mut commands_start = content.len();
                let command_str = commands_regex.find(content);
                let commands = if let Ok(Some(mat)) = command_str {
                    commands_start = mat.start();
                    let command_capture = command_regex
                        .captures_iter(mat.as_str())
                        .filter_map(|result| result.ok())
                        .filter_map(|cap| cap.get(3))
                        .fold(HashMap::new(), |mut a, b| {
                            a.insert(b.as_str(), true);
                            a
                        });
                    command_capture
                } else {
                    HashMap::new()
                };
                let is_private = *commands.get("private").or(Some(&false)).unwrap();

                let response_str = match interpret_rolls(&content[1..commands_start], 0) {
                    Ok(result) => format_rolls_result_new(result),
                    Err(e) => format!("Err: {}", e),
                };
                if is_private {
                    let link = msg.link();
                    println!("Sent to {}:\n{}", msg.author.name, response_str);
                    msg.author
                        .direct_message(&ctx.http, |m| {
                            m.content(format!("{}\n{}", link, response_str))
                        })
                        .await
                        .expect("Failed to direct message.");
                } else {
                    msg.reply(&ctx.http, response_str)
                        .await
                        .expect("Failed to reply.");
                }
                return;
            }

            if content.starts_with("!") {
                let exp = ShuntingParser::parse_str(&content[1..]);
                let res = MathContext::new().eval(&exp.unwrap());
                if res.is_ok() {
                    msg.reply(
                        &ctx.http,
                        format!(
                            "{} = **{}**",
                            content[1..].replace("*", r"\*"),
                            res.unwrap()
                        ),
                    )
                    .await
                    .expect("Failed to reply");
                    return;
                }
            }

            // TODO: Determine why code below this caused future across threads error

            // Check if is math equation
            // if let Ok(expr) = ShuntingParser::parse_str(content) {
            //     if let Ok(result) = MathContext::new().eval(&expr) {
            // msg.reply(
            //     &ctx.http,
            //     format!(
            //         "**{}\n{}**",
            //         content.replace("*", r"\*"),
            //         format!("**{}**", result)
            //     ),
            // );
            // return;
            //     } else {
            // msg.reply(&ctx.http, "Failed to successfully evaluate math.");
            // return;
            //     }
            // }

            if content.eq("!heh") {
                let heh_count = if let Ok(n) = increment_hehs(&self.aws_client).await {
                    n
                } else {
                    // Throw error
                    msg.reply(&ctx.http, "Heh, failed to get 'heh' count.")
                        .await
                        .expect("Failed to reply");
                    return;
                };
                msg.reply(
                    &ctx.http,
                    format!("Heh, we've counted {} 'heh's.", heh_count),
                )
                .await
                .expect("Failed to reply");
                return;
            }

            if content.to_lowercase().eq("!wakebotsucks") {
                msg.reply(
                    &ctx.http,
                    "https://y.yarn.co/ac2e41da-773a-4ae9-8012-b8c235994f9c_text.gif",
                )
                .await
                .expect("Failed to reply");
                return;
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
    }
    async fn ready(&self, _ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
    async fn channel_create(&self, _ctx: Context, _channel: &GuildChannel) {
        println!("Channel create");
    }
}

#[shuttle_runtime::main]
pub async fn serenity(
    #[shuttle_runtime::Secrets] secret_store: shuttle_runtime::SecretStore,
    #[shuttle_persist::Persist] persist: PersistInstance,
) -> shuttle_serenity::ShuttleSerenity {
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

    let aws_access_key = if let Some(id) = secret_store.get("AWS_ACCESS_KEY_ID") {
        id
    } else {
        return Err(anyhow!("'AWS_ACCESS_KEY_ID' was not found").into());
    };

    let aws_secret_access_key = if let Some(id) = secret_store.get("AWS_SECRET_ACCESS_KEY") {
        id
    } else {
        return Err(anyhow!("'AWS_SECRET_ACCESS_KEY' was not found").into());
    };

    let aws_creds = create_credentials_provider(&aws_access_key, &aws_secret_access_key);
    let aws_client = create_aws_client(aws_creds).await;

    let mut client = Client::builder(&discord_token, intents)
        .event_handler(Handler {
            aws_client,
            youtube_api_key,
            persist,
            allowed_channels: vec![outsiders_channel_id, test_channel_id],
        })
        .await
        .expect("Err creating client");

    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
    Ok(client.into())
}

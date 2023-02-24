#![allow(non_snake_case)]
use chrono::{DateTime, FixedOffset};
use reqwest::header::ACCEPT;

#[derive(serde::Deserialize, std::fmt::Debug, std::clone::Clone)]
struct PlaylistItemResourceId {
    videoId: String,
}

#[derive(serde::Deserialize, std::fmt::Debug, std::clone::Clone)]
pub struct PlaylistItemSnippet {
    description: String,
    pub publishedAt: String,
    resourceId: PlaylistItemResourceId,
    title: String,
}

#[derive(serde::Deserialize, std::fmt::Debug, std::clone::Clone)]
pub struct PlaylistItem {
    pub snippet: PlaylistItemSnippet,
}

#[derive(serde::Deserialize, std::fmt::Debug, std::clone::Clone)]
pub struct YoutubeResponse {
    pub items: Vec<PlaylistItem>,
}

#[derive(std::fmt::Debug, std::clone::Clone)]
pub struct YoutubeVideoOverview {
    pub title: String,
    #[allow(dead_code)] // Not yet used
    description: String,
    pub id: String,
    pub timestamp: String,
}

pub struct VideoResult {
    pub list: Vec<YoutubeVideoOverview>,
    pub overflow: bool,
}

pub async fn get_new_videos(
    api_key: &str,
    last_timestamp: DateTime<FixedOffset>,
) -> Result<VideoResult, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let list = client.get(format!("https://www.googleapis.com/youtube/v3/playlistItems?part=snippet&playlistId=PLrBG-2LsZMEWWoyEJKsQ3kbom9MFJ1n7e&maxResults=50&key={}", api_key))
        .header(ACCEPT, "application/json")
        .send()
        .await?
        .json::<YoutubeResponse>()
        .await?;
    let list = list
        .items
        .iter()
        .filter_map(|item| {
            let datetime =
                if let Ok(datetime) = DateTime::parse_from_rfc3339(&item.snippet.publishedAt) {
                    datetime
                } else {
                    return None;
                };
            if last_timestamp.lt(&datetime) {
                Some(YoutubeVideoOverview {
                    title: item.snippet.title.to_string(),
                    description: item.snippet.description.to_string(),
                    id: item.snippet.resourceId.videoId.to_string(),
                    timestamp: item.snippet.publishedAt.to_string(),
                })
            } else {
                None
            }
        })
        .collect::<Vec<YoutubeVideoOverview>>();
    if list.len() > 5 {
        Ok(VideoResult {
            list: list[list.len() - 5..].to_vec(),
            overflow: true,
        })
    } else {
        Ok(VideoResult {
            list,
            overflow: false,
        })
    }
}

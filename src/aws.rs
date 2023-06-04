use crate::errors::WakeBotError;
use aws_sdk_dynamodb::{
    config::Credentials,
    error::SdkError,
    operation::{
        delete_item::{DeleteItemError, DeleteItemOutput},
        get_item::GetItemError,
        put_item::{PutItemError, PutItemOutput},
    },
    types::AttributeValue,
    Client,
};

pub async fn create_aws_client(credentials: Credentials) -> Client {
    let config = aws_config::from_env()
        .region("us-east-1")
        .credentials_provider(credentials)
        .load()
        .await;
    Client::new(&config)
}

pub fn create_credentials_provider(access_key: &str, secret_key: &str) -> Credentials {
    Credentials::new(access_key, secret_key, None, None, "actions-provider")
}

pub struct Action {
    pub name: String,
    pub roll: String,
}

pub async fn add_or_update_action(
    client: &Client,
    action: Action,
) -> Result<PutItemOutput, SdkError<PutItemError>> {
    let name_av = AttributeValue::S(action.name);
    let roll_av = AttributeValue::S(action.roll);
    let request = client
        .put_item()
        .table_name("actions")
        .item("name", name_av)
        .item("roll", roll_av);
    request.send().await
}

pub async fn delete_action(
    client: &Client,
    action_name: &str,
) -> Result<DeleteItemOutput, SdkError<DeleteItemError>> {
    client
        .delete_item()
        .table_name("actions")
        .key("name", AttributeValue::S(action_name.into()))
        .send()
        .await
}

#[derive(std::fmt::Debug)]
pub enum WakeBotGetError {
    AWSError(SdkError<GetItemError>),
    NotFound(WakeBotError),
}

pub async fn get_action_roll(
    client: &Client,
    action_name: &str,
) -> Result<String, WakeBotGetError> {
    let str = client
        .get_item()
        .table_name("actions")
        .key("name", AttributeValue::S(action_name.into()))
        .send()
        .await
        .map_err(|e| WakeBotGetError::AWSError(e))?;
    let str = if let Some(val) = str.item() {
        val
    } else {
        return Err(WakeBotGetError::NotFound(WakeBotError::new(
            "Action does not exist.",
        )));
    };
    Ok(String::from(str.get("roll").unwrap().as_s().unwrap()))
}

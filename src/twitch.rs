use http::StatusCode;
use serde::{Deserialize, Serialize};

const API_TOKEN_URL: &'static str = "https://id.twitch.tv/oauth2/token";

#[derive(Serialize, Deserialize)]
pub struct TwitchAuthorization {
    access_token: String,
    expires_in: i64,
    token_type: String,
}

#[derive(Serialize, Deserialize)]
pub struct TwitchConfiguration {
    client_id: String,
    client_secret: String,
    grant_type: String,
}

#[derive(Serialize, Deserialize)]
pub struct TwitchUser {
    pub id: String,
    pub login: String,
    pub display_name: String,
    pub r#type: String,
    pub broadcaster_type: String,
    pub description: String,
    pub profile_image_url: String,
    pub offline_image_url: String,
    pub view_count: i64,
}

impl PartialEq for TwitchUser {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Serialize, Deserialize)]
pub struct TwitchUserResponse {
    data: Vec<TwitchUser>,
}

#[derive(Serialize, Deserialize)]
pub struct TwitchStream {
    pub id: String,
    pub user_id: String,
    pub user_name: String,
    pub game_id: String,
    pub r#type: String,
    pub title: String,
    pub viewer_count: i64,
    pub started_at: String,
    pub language: String,
    pub thumbnail_url: String,
    pub tag_ids: Vec<String>,
}

impl PartialEq for TwitchStream {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Serialize, Deserialize)]
pub struct TwitchStreamResponse {
    data: Vec<TwitchStream>,
}

pub struct TwitchHelixAPI {
    client: reqwest::blocking::Client,
    config: TwitchConfiguration,
    authorization: TwitchAuthorization,
}

impl TwitchHelixAPI {
    pub fn new(client_id: String, client_secret: String) -> TwitchHelixAPI {
        let config = TwitchConfiguration {
            client_id: client_id,
            client_secret: client_secret,
            grant_type: String::from("client_credentials"),
        };

        // Refactor out - basically duplicating update_access_token
        let client = reqwest::blocking::Client::new();
        let res = client
            .post(API_TOKEN_URL)
            .form(&config)
            .send()
            .expect("Failed to request a proper Twitch authorization.");

        let text = res
            .text()
            .expect("Failed to process the Twitch authorization response as text.");

        let authorization: TwitchAuthorization = serde_json::from_str(&text)
            .expect("Failed to parse Twitch authorization response to data structure.");

        TwitchHelixAPI {
            client: client,
            config: config,
            authorization: authorization,
        }
    }

    fn update_access_token(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        print!("Reauthorizing with Twitch....");

        let res = self.client.post(API_TOKEN_URL).form(&self.config).send()?;

        let text = res.text()?;

        let authorization: TwitchAuthorization = serde_json::from_str(&text)?;

        self.authorization = authorization;

        println!("OK!");
        Ok(())
    }

    pub fn retrieve_users(
        &mut self,
        usernames: &[String],
    ) -> Result<Vec<TwitchUser>, Box<dyn std::error::Error>> {
        let query = String::from("login");
        let param: Vec<(&String, &String)> =
            usernames.into_iter().map(|entry| (&query, entry)).collect();
        let chunks = param.chunks(100);
        let mut data: Vec<TwitchUser> = Vec::new();

        for chunk in chunks {
            let res = self
                .client
                .get("https://api.twitch.tv/helix/users")
                .header("Client-ID", &self.config.client_id)
                .bearer_auth(&self.authorization.access_token)
                .query(&chunk)
                .send()?;

            if res.status() == StatusCode::UNAUTHORIZED {
                self.update_access_token()?;
                return self.retrieve_users(usernames);
            }

            let text = res.text()?;

            let parse: TwitchUserResponse = serde_json::from_str(&text)?;
            data.extend(parse.data);
        }

        Ok(data)
    }

    pub fn retrieve_streams<'a, I>(
        &mut self,
        users: I,
    ) -> Result<Vec<TwitchStream>, Box<dyn std::error::Error>>
    where
        I: Iterator<Item = &'a TwitchUser>,
    {
        let collection: Vec<&TwitchUser> = users.collect();
        let query = String::from("user_id");
        let param: Vec<(&String, &String)> =
            collection.iter().map(|entry| (&query, &entry.id)).collect();
        let chunks = param.chunks(100);
        let mut data: Vec<TwitchStream> = Vec::new();

        for chunk in chunks {
            let res = self
                .client
                .get("https://api.twitch.tv/helix/streams")
                .header("Client-ID", &self.config.client_id)
                .bearer_auth(&self.authorization.access_token)
                .query(&chunk)
                .send()?;

            if res.status() == StatusCode::UNAUTHORIZED {
                self.update_access_token()?;
                return self.retrieve_streams(collection.into_iter());
            }

            let text = res.text()?;

            let parse: TwitchStreamResponse = serde_json::from_str(&text)?;

            data.extend(parse.data);
        }

        Ok(data)
    }
}

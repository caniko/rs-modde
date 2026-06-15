#![allow(clippy::wildcard_imports)]
use super::*;
use crate::nexus::graphql_url;

impl NexusApi {
    /// Fetch the full image gallery for a mod via the unofficial v2 GraphQL
    /// endpoint. Returns a list of image URLs (the main `picture_url` will
    /// typically be the first entry, but this is not guaranteed — the caller
    /// should merge with `picture_url` as a fallback).
    ///
    /// The GraphQL schema is undocumented and may change; on any error this
    /// function returns an `Err` and the caller should fall back to the
    /// single `picture_url` from the v1 `get_mod` response.
    pub async fn get_mod_media(
        &self,
        game_domain: &str,
        mod_id: NexusModId,
    ) -> Result<Vec<String>> {
        let query = r"query ModMedia($modId: Int!, $gameDomain: String!) {
  mod(modId: $modId, gameDomain: $gameDomain) {
    modImages { url }
  }
}";
        let body = serde_json::json!({
            "query": query,
            "variables": {
                "modId": mod_id.get(),
                "gameDomain": game_domain,
            },
        });

        let resp = self
            .client
            .post(graphql_url())
            .header("apikey", &self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        let payload: serde_json::Value = resp.json().await?;
        if let Some(errors) = payload.get("errors") {
            bail!("Nexus GraphQL errors: {errors}");
        }
        let images = payload
            .get("data")
            .and_then(|d| d.get("mod"))
            .and_then(|m| m.get("modImages"))
            .and_then(|a| a.as_array())
            .ok_or_else(|| anyhow::anyhow!("unexpected GraphQL response shape"))?;

        let urls: Vec<String> = images
            .iter()
            .filter_map(|img| {
                img.get("url")
                    .and_then(|u| u.as_str())
                    .map(std::string::ToString::to_string)
            })
            .collect();
        Ok(urls)
    }
}

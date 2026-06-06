use secrecy::{ExposeSecret, SecretString};

use crate::domain::PromoteError;
use crate::domain::traits::Forge;

/// Adapter: Gitea forge operations via its REST API.
pub struct GiteaForge {
    base_url: String,
    owner: String,
    repo: String,
    token: SecretString,
}

impl GiteaForge {
    pub fn new(base_url: String, owner: String, repo: String, token: SecretString) -> Self {
        Self {
            base_url,
            owner,
            repo,
            token,
        }
    }

    // qual:allow(srp) reason: "URL builder helper — used by all Forge methods"
    fn api_base(&self) -> String {
        format!(
            "{}/api/v1/repos/{}/{}",
            self.base_url, self.owner, self.repo
        )
    }

    // qual:allow(srp) reason: "auth helper — used by all Forge methods"
    fn auth_header(&self) -> String {
        format!("token {}", self.token.expose_secret())
    }
}

impl Forge for GiteaForge {
    fn create_release(&self, tag: &str, name: &str, body: &str) -> Result<(), PromoteError> {
        let url = format!("{}/releases", self.api_base());
        let payload = serde_json::json!({
            "tag_name": tag,
            "name": name,
            "body": body,
        });
        ureq::post(&url)
            .set("Authorization", &self.auth_header())
            .send_json(payload)
            .map_err(|e| PromoteError::Other(anyhow::anyhow!("create_release failed: {e}")))?;
        Ok(())
    }

    fn create_pr(
        &self,
        title: &str,
        body: &str,
        head: &str,
        base: &str,
    ) -> Result<u64, PromoteError> {
        let url = format!("{}/pulls", self.api_base());
        let payload = serde_json::json!({
            "title": title,
            "body": body,
            "head": head,
            "base": base,
        });
        let resp = ureq::post(&url)
            .set("Authorization", &self.auth_header())
            .send_json(payload)
            .map_err(|e| PromoteError::Other(anyhow::anyhow!("create_pr failed: {e}")))?;
        let json: serde_json::Value = resp
            .into_json()
            .map_err(|e| PromoteError::Other(anyhow::anyhow!("create_pr parse failed: {e}")))?;
        let number = json["number"]
            .as_u64()
            .ok_or_else(|| PromoteError::Other(anyhow::anyhow!("create_pr: missing number")))?;
        Ok(number)
    }

    fn comment_pr(&self, pr_number: u64, body: &str) -> Result<(), PromoteError> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/issues/{}/comments",
            self.base_url, self.owner, self.repo, pr_number
        );
        let payload = serde_json::json!({ "body": body });
        ureq::post(&url)
            .set("Authorization", &self.auth_header())
            .send_json(payload)
            .map_err(|e| PromoteError::Other(anyhow::anyhow!("comment_pr failed: {e}")))?;
        Ok(())
    }

    fn close_pr(&self, pr_number: u64) -> Result<(), PromoteError> {
        let url = format!("{}/pulls/{}", self.api_base(), pr_number);
        let payload = serde_json::json!({ "state": "closed" });
        ureq::patch(&url)
            .set("Authorization", &self.auth_header())
            .send_json(payload)
            .map_err(|e| PromoteError::Other(anyhow::anyhow!("close_pr failed: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitea_forge_new_constructs_correctly() {
        let forge = GiteaForge::new(
            "http://localhost:3000".to_string(),
            "joe".to_string(),
            "myrepo".to_string(),
            SecretString::from("test-token".to_string()),
        );
        assert_eq!(forge.base_url, "http://localhost:3000");
        assert_eq!(forge.owner, "joe");
        assert_eq!(forge.repo, "myrepo");
        assert_eq!(
            forge.api_base(),
            "http://localhost:3000/api/v1/repos/joe/myrepo"
        );
    }
}

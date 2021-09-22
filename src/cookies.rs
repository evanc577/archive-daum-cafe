use crate::error::DownloaderError::AuthenticationError;

use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::fs;

pub struct Auth {
    client: reqwest::Client,
}

impl Auth {
    pub async fn get_cookies(cookies_file: &str) -> Result<String> {
        println!("Authenticating...");

        let auth = Auth {
            // Force native TLS because logins.daum.net doesn't support forward secrecy ciphers,
            // which rustls requires
            client: reqwest::Client::builder()
                .use_native_tls()
                .build()
                .context("Error building authentication client")?,
        };

        let kakao_cookies = auth
            .read_cookies_file(cookies_file)
            .context(format!("Error reading {}", cookies_file))?;
        let sso_token = auth
            .get_sso_token(kakao_cookies.as_str())
            .await
            .context("Error getting SSO token")?;
        let daum_cookies = auth
            .get_daum_cookies(sso_token.as_str())
            .await
            .context("Error getting Daum cookies")?;

        println!("Authentication done");
        Ok(daum_cookies)
    }

    async fn get_daum_cookies(&self, sso_token: &str) -> Result<String> {
        // Get daum.net cookies
        let resp = self
            .client
            .get("https://logins.daum.net/accounts/kakaossotokenlogin.do")
            .query(&[("ssotoken", sso_token)])
            .header(reqwest::header::HOST, "logins.daum.net")
            .send()
            .await?;

        // Extract daum.net cookies
        let cookies = resp
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect::<Vec<_>>()
            .join("; ");

        Ok(cookies)
    }

    fn read_cookies_file(&self, cookies_file: &str) -> Result<String> {
        lazy_static! {
            static ref RE: Regex = Regex::new(
                r"(?m)^(?P<domain>\.kakao\.com)\t.+?\t.+?\t.+?\t.+?\t(?P<name>.+?)\t(?P<value>.+?)$"
            )
            .unwrap();
        }

        let cookies_contents = fs::read_to_string(&cookies_file)?;

        let cookie: String = RE
            .captures_iter(&cookies_contents)
            .filter_map(|c| {
                let name = c.name("name")?.as_str();
                let value = c.name("value")?.as_str();
                Some(format!("{}={}", name, value))
            })
            .collect::<Vec<_>>()
            .join("; ");

        Ok(cookie)
    }

    async fn get_sso_token(&self, kakao_cookies: &str) -> Result<String> {
        // Get SSO token
        let response = self
            .client
            .get("https://accounts.kakao.com/weblogin/sso_token/daum.js?callback=loginByToken")
            .header(reqwest::header::COOKIE, kakao_cookies)
            .header(reqwest::header::REFERER, "https://logins.daum.net/")
            .send()
            .await?
            .text()
            .await?;

        // Parse token
        lazy_static! {
            static ref RE: Regex = Regex::new(r#""token":.*?"(?P<token>[0-9a-f]+?)""#).unwrap();
        }
        let token = RE
            .captures(&response)
            .ok_or(AuthenticationError)?
            .name("token")
            .ok_or(AuthenticationError)?
            .as_str();

        Ok(token.into())
    }
}

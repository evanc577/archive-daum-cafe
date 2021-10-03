use crate::error::DownloaderError;

use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::io::Write;

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

        let current_cookies_file = format!("{}.current", cookies_file);

        let kakao_cookies = auth
            .read_cookies_file(cookies_file, &current_cookies_file)
            .context(format!("Error reading {}", cookies_file))?;
        let sso_token = match auth.get_sso_token(&kakao_cookies).await {
            Ok(t) => t,
            Err(_) => {
                let new_kakao_cookies = auth
                    .update_kakao_coookies(&deserialize_cookies(&kakao_cookies), &current_cookies_file)
                    .await?;
                auth.get_sso_token(&new_kakao_cookies)
                    .await
                    .context("Error getting SSO token")?
            }
        };
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

    fn read_cookies_file(&self, cookies_file: &str, current_cookies_file: &str) -> Result<String> {
        lazy_static! {
            static ref RE: Regex = Regex::new(
                r"(?m)^(?P<domain>\.kakao\.com)\t.+?\t.+?\t.+?\t.+?\t(?P<name>.+?)\t(?P<value>.+?)$"
            )
            .unwrap();
        }

        if let Ok(current_cookies) = fs::read_to_string(current_cookies_file) {
            return Ok(current_cookies);
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
        let resp = self
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
            .captures(&resp)
            .ok_or(DownloaderError::Authentication)?
            .name("token")
            .ok_or(DownloaderError::Authentication)?
            .as_str();

        Ok(token.into())
    }

    async fn update_kakao_coookies(
        &self,
        kakao_cookies: &HashMap<String, String>,
        current_cookies_file: &str,
    ) -> Result<String> {
        println!("Updating cookies");

        let resp = self
            .client
            .get("https://accounts.kakao.com/weblogin/account/info")
            .header(reqwest::header::COOKIE, serialize_cookies(kakao_cookies))
            .header(reqwest::header::REFERER, "https://accounts.kakao.com/")
            .send()
            .await?;

        let new_cookies = resp
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| {
                let mut cookie = v.to_str().ok()?.split(';');
                let name = cookie.next()?.to_owned();
                let value = cookie.next().unwrap_or("").to_owned();
                Some((name, value))
            })
            .collect::<HashMap<_, _>>();

        let mut final_cookies: HashMap<String, String> = kakao_cookies.clone();
        for (new_k, new_v) in new_cookies {
            if let Some(old_v) = final_cookies.get_mut(&new_k) {
                if new_v.is_empty() {
                    final_cookies.remove(&new_k);
                } else {
                    *old_v = new_v;
                }
            }
        }

        let cookies = serialize_cookies(&final_cookies);

        let mut file = fs::File::create(current_cookies_file)
            .context("Unable to create current cookies file")?;
        file.write_all(cookies.as_bytes())
            .context("Unable to write to current cookies file")?;

        Ok(cookies)
    }
}

fn serialize_cookies(map: &HashMap<String, String>) -> String {
    let cookies = map
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("; ");
    cookies
}

fn deserialize_cookies(cookies: &str) -> HashMap<String, String> {
    cookies
        .split(';')
        .filter_map(|s| {
            let mut cookie = s.split('=');
            let name = cookie.next()?.trim().to_owned();
            let value = cookie.next()?.trim().to_owned();
            Some((name, value))
        })
        .collect()
}

use crate::error::DownloaderError::AuthenticationError;

use anyhow::Result;
use lazy_static::lazy_static;
use regex::Regex;
use std::fs;

pub async fn get_daum_cookies(cookies_file: &str) -> Result<String> {
    let kakao_cookies = read_cookies_file(cookies_file)?;
    let sso_token = get_sso_token(kakao_cookies.as_str()).await?;

    // Get daum.net cookies
    let client = reqwest::Client::builder().build()?;
    let resp = client
        .get("https://logins.daum.net/accounts/kakaossotokenlogin.do")
        .query(&[("ssotoken", sso_token.as_str())])
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

fn read_cookies_file(cookies_file: &str) -> Result<String> {
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

async fn get_sso_token(kakao_cookies: &str) -> Result<String> {
    // Add headers
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::COOKIE, kakao_cookies.parse().unwrap());
    headers.insert(
        reqwest::header::REFERER,
        "https://logins.daum.net/".parse().unwrap(),
    );
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap();

    // Get SSO token
    let response = client
        .get("https://accounts.kakao.com/weblogin/sso_token/daum.js?callback=loginByToken")
        .send()
        .await?
        .text()
        .await?;
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

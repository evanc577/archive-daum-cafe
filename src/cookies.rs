use anyhow::Result;
use lazy_static::lazy_static;
use regex::Regex;
use std::fs;

pub fn read_cookies(cookies_file: &str) -> Result<String> {
    lazy_static! {
        static ref RE: Regex = Regex::new(
            r"(?m)^(?P<domain>\.daum\.net)\t.+?\t.+?\t.+?\t.+?\t(?P<name>.+?)\t(?P<value>.+?)$"
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
